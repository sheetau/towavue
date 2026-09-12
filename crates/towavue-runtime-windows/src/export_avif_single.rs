//! Specialize FFmpeg's owned MP4 tables to an AVIF sequence, without
//! duplicating frames or changing encoded data/chunk offsets. Only owned output
//! is accepted: nonfragmented, tail moov, matching AV1 samples per color/alpha track.
use super::*;
use std::io::Write;

// Two tracks of up to 65536 timing/size/chunk entries plus fixed headers.
const LIMIT: usize = 4 * 1024 * 1024;

fn atom(kind: &[u8; 4], payload: &[u8]) -> Result<Vec<u8>, ExportError> {
    if payload.len() > LIMIT {
        return Err(invalid("sequence movie metadata exceeds limit"));
    }
    Ok([
        &((payload.len() + 8) as u32).to_be_bytes()[..],
        kind,
        payload,
    ]
    .concat())
}

struct Movie<'a> {
    timescale: u32,
    duration: u64,
    delays: &'a [u32],
    total: u64,
    loops: u32,
}

pub(super) fn finish(
    path: &Path,
    timing: SequenceTiming<'_>,
    alpha: bool,
    cancelled: &AtomicBool,
) -> Result<(), ExportError> {
    check_cancelled(cancelled)?;
    if timing.time_base.numerator() != 1 || timing.time_base.denominator() <= 0 {
        return Err(invalid("invalid sequence time base"));
    }
    let delays = timing
        .packet_durations
        .ok_or_else(|| invalid("missing sample durations"))?;
    if delays.is_empty() || delays.len() > 65536 || delays.contains(&0) {
        return Err(invalid("AVIF requires 1–65536 positive sample durations"));
    }
    let duration: u64 = delays.iter().map(|delay| u64::from(*delay)).sum();
    let movie = Movie {
        timescale: timing.time_base.denominator() as u32,
        duration,
        delays,
        total: if timing.loops == 0 {
            u64::MAX
        } else {
            duration
                .checked_mul(u64::from(timing.loops))
                .ok_or_else(|| invalid("repeated sequence duration exceeds 64 bits"))?
        },
        loops: timing.loops,
    };
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(ExportError::Output)?;
    let length = file.metadata().map_err(ExportError::Output)?.len();
    let root = boxes(&mut file, 0, length, cancelled)?;
    let moov = one(&root, b"moov")?.ok_or_else(|| invalid("missing sequence movie"))?;
    let ftyp = one(&root, b"ftyp")?.ok_or_else(|| invalid("missing sequence brands"))?;
    let mdat = one(&root, b"mdat")?.ok_or_else(|| invalid("missing sequence media"))?;
    if moov.end != length
        || moov.end - moov.header > LIMIT as u64
        || mdat.end > moov.header
        || root
            .iter()
            .any(|item| !matches!(&item.kind, b"ftyp" | b"free" | b"mdat" | b"moov"))
    {
        return Err(invalid("unexpected sequence muxer layout"));
    }
    let mut brands = bytes(&mut file, ftyp, 4096)?;
    if brands.len() < 24 || brands.len() % 4 != 0 {
        return Err(invalid("insufficient sequence brand space"));
    }
    brands[..8].copy_from_slice(b"avis\0\0\0\0");
    for (index, brand) in brands[8..].as_chunks_mut::<4>().0.iter_mut().enumerate() {
        brand.copy_from_slice([b"avis", b"msf1", b"iso8", b"miaf"][index.min(3)]);
    }
    let children = boxes(&mut file, moov.start, moov.end, cancelled)?;
    let tracks: Vec<_> = children
        .iter()
        .filter(|item| item.kind == *b"trak")
        .collect();
    if tracks.len() != if alpha { 2 } else { 1 } {
        return Err(invalid("unexpected sequence track count"));
    }
    let mut data = Vec::new();
    let mut track_index = 0;
    for item in children {
        check_cancelled(cancelled)?;
        if item.kind == *b"trak" {
            data.extend(movie.rewrite(&mut file, item, track_index, cancelled)?);
            track_index += 1;
        } else if item.kind == *b"mvhd" {
            data.extend(movie.rewrite(&mut file, item, 0, cancelled)?);
        }
        // No inherited encoder metadata or MP4-only movie extensions.
    }
    let data = atom(b"moov", &data)?;
    check_cancelled(cancelled)?;
    file.seek(SeekFrom::Start(moov.header))
        .map_err(ExportError::Output)?;
    file.write_all(&data).map_err(ExportError::Output)?;
    file.set_len(moov.header + data.len() as u64)
        .map_err(ExportError::Output)?;
    file.seek(SeekFrom::Start(ftyp.start))
        .map_err(ExportError::Output)?;
    file.write_all(&brands).map_err(ExportError::Output)?;
    check_cancelled(cancelled)
}

impl Movie<'_> {
    fn rewrite(
        &self,
        file: &mut fs::File,
        item: BoxRange,
        track: usize,
        cancelled: &AtomicBool,
    ) -> Result<Vec<u8>, ExportError> {
        check_cancelled(cancelled)?;
        let prefix = match &item.kind {
            b"trak" | b"mdia" | b"minf" | b"stbl" => Some(0),
            b"stsd" => Some(8),
            b"av01" => Some(78),
            _ => None,
        };
        let mut data = bytes(file, item, LIMIT)?;
        if let Some(prefix) = prefix {
            if data.len() < prefix
                || (item.kind == *b"stsd" && data[..8] != [0, 0, 0, 0, 0, 0, 0, 1])
            {
                return Err(invalid("invalid sequence sample description"));
            }
            data.truncate(prefix);
            for child in boxes(file, item.start + prefix as u64, item.end, cancelled)? {
                let allowed = match &item.kind {
                    b"trak" => matches!(&child.kind, b"tkhd" | b"mdia"),
                    b"mdia" => matches!(&child.kind, b"mdhd" | b"hdlr" | b"minf"),
                    b"minf" => matches!(&child.kind, b"vmhd" | b"dinf" | b"stbl"),
                    b"stbl" => matches!(
                        &child.kind,
                        b"stsd" | b"stts" | b"stss" | b"stsc" | b"stsz" | b"stco" | b"co64"
                    ),
                    b"stsd" => child.kind == *b"av01",
                    b"av01" => {
                        matches!(&child.kind, b"av1C" | b"colr" | b"pasp" | b"btrt" | b"fiel")
                    }
                    _ => false,
                };
                if !allowed {
                    return Err(invalid(format!(
                        "unexpected sequence child {} in {}",
                        String::from_utf8_lossy(&child.kind),
                        String::from_utf8_lossy(&item.kind)
                    )));
                }
                if child.kind == *b"btrt" {
                    // The placeholder packet duration is not the final bitrate.
                    continue;
                }
                if child.kind == *b"fiel" {
                    if bytes(file, child, 2)? != [1, 0] {
                        return Err(invalid("non-progressive sequence sample"));
                    }
                    continue; // QuickTime field-order extension is unnecessary for AVIF.
                }
                data.extend(self.rewrite(file, child, track, cancelled)?);
            }
            if item.kind == *b"av01" && track == 1 {
                data.extend(atom(
                    b"auxi",
                    b"\0\0\0\0urn:mpeg:mpegB:cicp:systems:auxiliary:alpha\0",
                )?);
            }
            if item.kind == *b"trak" {
                // AVIF elst repetition counts whole display cycles; movie duration
                // includes repetitions while mdhd/stts describe exactly one cycle.
                let mut edit = vec![1, 0, 0, u8::from(self.loops != 1)];
                edit.extend(1u32.to_be_bytes());
                edit.extend(self.duration.to_be_bytes());
                edit.extend(0u64.to_be_bytes());
                edit.extend(65536u32.to_be_bytes());
                data.extend(atom(b"edts", &atom(b"elst", &edit)?)?);
                if track == 1 {
                    data.extend(atom(b"tref", &atom(b"auxl", &1u32.to_be_bytes())?)?);
                }
            }
        } else {
            match &item.kind {
                b"mvhd" | b"mdhd" | b"tkhd" => {
                    let wide = match data.first() {
                        Some(0) => false,
                        Some(1) => true,
                        _ => return Err(invalid("invalid generated header version")),
                    };
                    let track_header = item.kind == *b"tkhd";
                    let offset = if wide { 20 } else { 12 };
                    let suffix =
                        offset + if track_header { 8 } else { 4 } + if wide { 8 } else { 4 };
                    if data.len() < suffix {
                        return Err(invalid("truncated generated header"));
                    }
                    let mut header = data[..4].to_vec();
                    header[0] = 1;
                    header.extend([0; 16]); // creation/modification times are not source metadata
                    if track_header {
                        if data[offset..offset + 4] != ((track + 1) as u32).to_be_bytes() {
                            return Err(invalid("unexpected generated track ID"));
                        }
                        header.extend(&data[offset..offset + 8]);
                    } else {
                        header.extend(self.timescale.to_be_bytes());
                    }
                    header.extend(
                        if item.kind == *b"mdhd" {
                            self.duration
                        } else {
                            self.total
                        }
                        .to_be_bytes(),
                    );
                    header.extend(&data[suffix..]);
                    data = header;
                }
                b"hdlr" => {
                    if data.len() < 12 || data[8..12] != *b"vide" {
                        return Err(invalid("unexpected generated handler"));
                    }
                    data[8..12].copy_from_slice(if track == 0 { b"pict" } else { b"auxv" });
                }
                b"stts" => {
                    if data.len() < 8 || data[..4] != [0; 4] {
                        return Err(invalid("invalid generated sample times"));
                    }
                    let entries =
                        u32::from_be_bytes(data[4..8].try_into().expect("count")) as usize;
                    if data.len() != 8 + entries * 8
                        || data[8..]
                            .as_chunks::<8>()
                            .0
                            .iter()
                            .map(|entry| {
                                u64::from(u32::from_be_bytes(
                                    entry[..4].try_into().expect("samples"),
                                ))
                            })
                            .sum::<u64>()
                            != self.delays.len() as u64
                    {
                        return Err(invalid("generated sample time count differs"));
                    }
                    data = vec![0; 4];
                    data.extend((self.delays.len() as u32).to_be_bytes());
                    for delay in self.delays {
                        data.extend(1u32.to_be_bytes());
                        data.extend(delay.to_be_bytes());
                    }
                }
                b"stsz"
                    if data.len() < 12
                        || data[8..12] != (self.delays.len() as u32).to_be_bytes() =>
                {
                    return Err(invalid("generated sample count differs"));
                }
                _ => {}
            }
        }
        atom(&item.kind, &data)
    }
}
