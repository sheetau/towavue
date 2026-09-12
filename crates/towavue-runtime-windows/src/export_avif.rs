use super::*;
use std::io::{Seek, SeekFrom};

fn invalid(error: impl std::fmt::Display) -> ExportError {
    ExportError::Failed(format!("AVIF export: {error}"))
}

pub(super) fn avif_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("avif"))
}

#[derive(Clone, Copy)]
struct BoxRange {
    kind: [u8; 4],
    header: u64,
    start: u64,
    end: u64,
}

fn boxes(
    file: &mut fs::File,
    start: u64,
    end: u64,
    cancelled: &AtomicBool,
) -> Result<Vec<BoxRange>, ExportError> {
    let mut result = Vec::new();
    let mut position = start;
    while position < end {
        check_cancelled(cancelled)?;
        if end - position < 8 || result.len() == 65536 {
            return Err(invalid("invalid box boundary or too many boxes"));
        }
        file.seek(SeekFrom::Start(position))
            .map_err(ExportError::Output)?;
        let mut header = [0; 8];
        file.read_exact(&mut header).map_err(ExportError::Output)?;
        let size = u32::from_be_bytes(header[..4].try_into().expect("box size"));
        let (size, header_size) = match size {
            0 => (end - position, 8),
            1 => {
                let mut size = [0; 8];
                if end - position < 16 {
                    return Err(invalid("incomplete large box"));
                }
                file.read_exact(&mut size).map_err(ExportError::Output)?;
                (u64::from_be_bytes(size), 16)
            }
            size => (u64::from(size), 8),
        };
        if size < header_size || size > end - position {
            return Err(invalid("box exceeds parent bounds"));
        }
        result.push(BoxRange {
            kind: header[4..].try_into().expect("box kind"),
            header: position,
            start: position + header_size,
            end: position + size,
        });
        position += size;
    }
    Ok(result)
}

fn one(boxes: &[BoxRange], kind: &[u8; 4]) -> Result<Option<BoxRange>, ExportError> {
    let mut found = boxes.iter().filter(|item| &item.kind == kind);
    let result = found.next().copied();
    if found.next().is_some() {
        return Err(invalid(format!(
            "duplicate {} box",
            String::from_utf8_lossy(kind)
        )));
    }
    Ok(result)
}

fn bytes(file: &mut fs::File, item: BoxRange, limit: usize) -> Result<Vec<u8>, ExportError> {
    if item.end - item.start > limit as u64 {
        return Err(invalid("oversized control box"));
    }
    file.seek(SeekFrom::Start(item.start))
        .map_err(ExportError::Output)?;
    let mut bytes = vec![0; (item.end - item.start) as usize];
    file.read_exact(&mut bytes).map_err(ExportError::Output)?;
    Ok(bytes)
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, ExportError> {
    bytes
        .get(offset..offset + 4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_be_bytes)
        .ok_or_else(|| invalid("truncated control field"))
}
fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, ExportError> {
    bytes
        .get(offset..offset + 8)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u64::from_be_bytes)
        .ok_or_else(|| invalid("truncated control field"))
}

#[derive(Clone, Debug, PartialEq)]
struct Track {
    id: u32,
    timescale: u32,
    duration: u64,
    // None means that no edit list declares repetition; 0 means infinite.
    loops: Option<u32>,
    alpha_for: Option<u32>,
    premultiplied_with: Option<u32>,
}

fn track(
    file: &mut fs::File,
    item: BoxRange,
    cancelled: &AtomicBool,
) -> Result<Track, ExportError> {
    let children = boxes(file, item.start, item.end, cancelled)?;
    let header = one(&children, b"tkhd")?.ok_or_else(|| invalid("track has no tkhd"))?;
    let header = bytes(file, header, 128)?;
    let (id, duration) = match header.first() {
        Some(0) => (u32_at(&header, 12)?, u64::from(u32_at(&header, 20)?)),
        Some(1) => (u32_at(&header, 20)?, u64_at(&header, 28)?),
        _ => return Err(invalid("unsupported tkhd version")),
    };
    if id == 0 {
        return Err(invalid("invalid track ID"));
    }
    let mut loops = None;
    if let Some(edts) = one(&children, b"edts")? {
        let edits = boxes(file, edts.start, edts.end, cancelled)?;
        let edit = one(&edits, b"elst")?.ok_or_else(|| invalid("edts has no elst"))?;
        let edit = bytes(file, edit, 64)?;
        if u32_at(&edit, 4)? != 1 {
            return Err(invalid("multiple edit-list segments are not supported"));
        }
        let (segment, time, rate) = match edit.first() {
            Some(0) if edit.len() == 20 => (
                u64::from(u32_at(&edit, 8)?),
                u64::from(u32_at(&edit, 12)?),
                u32_at(&edit, 16)?,
            ),
            Some(1) if edit.len() == 28 => {
                (u64_at(&edit, 8)?, u64_at(&edit, 16)?, u32_at(&edit, 24)?)
            }
            _ => return Err(invalid("invalid elst version or length")),
        };
        if segment == 0 || time != 0 || rate != 65536 || edit[1..3] != [0, 0] || edit[3] & !1 != 0 {
            return Err(invalid("unsupported edit-list time, rate or flags"));
        }
        loops = Some(if edit[3] & 1 == 0 {
            1
        } else {
            if duration == 0 {
                return Err(invalid("repeating track has zero duration"));
            }
            let count = duration.div_ceil(segment);
            if count > i32::MAX as u64 {
                0
            } else {
                count as u32
            }
        });
    }
    let (mut alpha_for, mut premultiplied_with) = (None, None);
    if let Some(references) = one(&children, b"tref")? {
        let references = boxes(file, references.start, references.end, cancelled)?;
        for (kind, value) in [
            (b"auxl", &mut alpha_for),
            (b"prem", &mut premultiplied_with),
        ] {
            if let Some(reference) = one(&references, kind)? {
                let reference = bytes(file, reference, 4)?;
                *value = Some(u32_at(&reference, 0)?);
            }
        }
    }
    let media = one(&children, b"mdia")?.ok_or_else(|| invalid("track has no mdia"))?;
    let media = boxes(file, media.start, media.end, cancelled)?;
    if alpha_for.is_some() {
        // An auxiliary reference alone does not establish that the samples are alpha.
        let mut parent = media.clone();
        for kind in [b"minf", b"stbl"] {
            let item = one(&parent, kind)?.ok_or_else(|| invalid("missing alpha sample table"))?;
            parent = boxes(file, item.start, item.end, cancelled)?;
        }
        let descriptions = one(&parent, b"stsd")?.ok_or_else(|| invalid("missing alpha stsd"))?;
        if descriptions.end - descriptions.start < 8 {
            return Err(invalid("truncated sample descriptions"));
        }
        let descriptions = boxes(file, descriptions.start + 8, descriptions.end, cancelled)?;
        if descriptions.len() != 1 || descriptions[0].kind != *b"av01" {
            return Err(invalid("unsupported alpha sample description"));
        }
        let description = descriptions[0];
        if description.end - description.start < 78 {
            return Err(invalid("truncated visual sample entry"));
        }
        let properties = boxes(file, description.start + 78, description.end, cancelled)?;
        let alpha = one(&properties, b"auxi")?.ok_or_else(|| invalid("missing alpha type"))?;
        if bytes(file, alpha, 48)? != b"\0\0\0\0urn:mpeg:mpegB:cicp:systems:auxiliary:alpha\0" {
            return Err(invalid("unsupported auxiliary type"));
        }
    }
    let media = one(&media, b"mdhd")?.ok_or_else(|| invalid("track has no mdhd"))?;
    let media = bytes(file, media, 64)?;
    let (timescale, media_duration) = match media.first() {
        Some(0) => (u32_at(&media, 12)?, u64::from(u32_at(&media, 16)?)),
        Some(1) => (u32_at(&media, 20)?, u64_at(&media, 24)?),
        _ => return Err(invalid("unsupported mdhd version")),
    };
    if timescale == 0 || timescale > i32::MAX as u32 || media_duration == 0 {
        return Err(invalid("invalid media time base or duration"));
    }
    Ok(Track {
        id,
        timescale,
        duration: media_duration,
        loops,
        alpha_for,
        premultiplied_with,
    })
}

#[derive(Clone, Debug)]
struct Samples {
    index: usize,
    id: u32,
    size: (u32, u32),
    time_base: ffmpeg::Rational,
    times: Vec<(i64, i64)>,
}

#[derive(Clone, Debug)]
pub(super) struct Animation {
    tracks: Vec<Track>,
    samples: Vec<Samples>,
}

impl Animation {
    pub(super) fn read(path: &Path, cancelled: &AtomicBool) -> Result<Option<Self>, ExportError> {
        check_cancelled(cancelled)?;
        let mut file = fs::File::open(path).map_err(ExportError::Output)?;
        let length = file.metadata().map_err(ExportError::Output)?.len();
        let root = boxes(&mut file, 0, length, cancelled)?;
        let ftyp = one(&root, b"ftyp")?.ok_or_else(|| invalid("missing ftyp"))?;
        let brands = bytes(&mut file, ftyp, 4096)?;
        if brands.len() < 8
            || brands.len() % 4 != 0
            || !brands
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .any(|(i, brand)| i != 1 && matches!(brand, b"avif" | b"avis"))
        {
            return Err(invalid("not an AVIF container"));
        }
        let Some(moov) = one(&root, b"moov")? else {
            return Ok(None);
        };
        let movie = boxes(&mut file, moov.start, moov.end, cancelled)?;
        let tracks = movie
            .iter()
            .filter(|item| &item.kind == b"trak")
            .map(|item| track(&mut file, *item, cancelled))
            .collect::<Result<Vec<_>, _>>()?;
        if tracks.is_empty() || tracks.len() > 2 {
            return Err(invalid("expected a color track and optional alpha track"));
        }
        if tracks
            .iter()
            .enumerate()
            .any(|(i, track)| tracks[..i].iter().any(|old| old.id == track.id))
        {
            return Err(invalid("duplicate track IDs"));
        }
        let color = tracks
            .iter()
            .find(|track| track.alpha_for.is_none())
            .ok_or_else(|| invalid("missing color track"))?;
        if tracks.len() == 2 && !tracks.iter().any(|track| track.alpha_for == Some(color.id)) {
            return Err(invalid("second track is not linked alpha"));
        }
        ffmpeg::init().map_err(invalid)?;
        let mut input = ffmpeg::format::input(path).map_err(invalid)?;
        let mut samples = Vec::new();
        for track in &tracks {
            // The demuxer also exposes primary still items, often with the same ID.
            // Select the timed track, not its single-frame cover image.
            let mut matching = input.streams().filter(|stream| {
                stream.id() as u32 == track.id
                    && stream.time_base() == ffmpeg::Rational(1, track.timescale as i32)
                    && stream.duration() > 0
                    && stream.duration() as u64 == track.duration
            });
            let stream = matching
                .next()
                .ok_or_else(|| invalid("missing timed AVIF track"))?;
            if matching.next().is_some() {
                return Err(invalid("ambiguous timed AVIF track"));
            }
            if stream.parameters().medium() != ffmpeg::media::Type::Video
                || stream.parameters().id() != ffmpeg::codec::Id::AV1
            {
                return Err(invalid("AVIF sequence contains a non-AV1 video stream"));
            }
            let id = u32::try_from(stream.id()).map_err(invalid)?;
            let decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
                .and_then(|context| context.decoder().video())
                .map_err(invalid)?;
            let size = (decoder.width(), decoder.height());
            crate::image_edits::output_size(size, &[]).map_err(invalid)?;
            samples.push(Samples {
                index: stream.index(),
                id,
                size,
                time_base: stream.time_base(),
                times: Vec::new(),
            });
        }
        loop {
            check_cancelled(cancelled)?;
            let mut packet = ffmpeg::Packet::empty();
            match packet.read(&mut input) {
                Ok(()) => {}
                Err(ffmpeg::Error::Eof) => break,
                Err(error) => return Err(invalid(error)),
            }
            let Some(samples) = samples
                .iter_mut()
                .find(|sample| sample.index == packet.stream())
            else {
                continue;
            };
            if packet.is_corrupt() || packet.size() == 0 {
                return Err(invalid("corrupt or empty AVIF sample"));
            }
            if samples.times.len() == 65536 {
                return Err(invalid("animation exceeds 65536 frames"));
            }
            samples.times.push((
                packet.pts().ok_or_else(|| invalid("missing sample PTS"))?,
                packet.duration(),
            ));
        }
        for samples in &mut samples {
            samples.times.sort_unstable_by_key(|(time, _)| *time);
            let declared = input
                .stream(samples.index)
                .expect("selected stream")
                .frames();
            if samples.times.is_empty()
                || (declared > 0 && declared as u64 != samples.times.len() as u64)
                || samples.times.iter().any(|(_, duration)| *duration <= 0)
                || samples.times.windows(2).any(|pair| pair[0].0 >= pair[1].0)
            {
                return Err(invalid("missing or nonpositive sample timing"));
            }
        }
        samples.sort_by_key(|sample| sample.id != color.id);
        if samples.get(1).is_some_and(|alpha| {
            alpha.size != samples[0].size
                || !same_timing(&samples[0], alpha)
                || i128::from(alpha.times[0].0) * i128::from(samples[0].time_base.denominator())
                    != i128::from(samples[0].times[0].0) * i128::from(alpha.time_base.denominator())
        }) {
            return Err(invalid(
                "alpha dimensions or sample timing differ from color",
            ));
        }
        Ok(Some(Self { tracks, samples }))
    }

    fn color(&self) -> &Track {
        self.tracks
            .iter()
            .find(|track| track.id == self.samples[0].id)
            .expect("color track")
    }

    pub(super) fn export(
        &self,
        request: &ExportRequest,
        staging: &StagedExport,
        cancelled: &AtomicBool,
        progress: &(impl Fn(Duration) + Sync),
    ) -> Result<(), ExportError> {
        use std::io::Write;
        if self.samples[0].times.len() < 2 {
            return Err(invalid(
                "single-frame AVIF sequences need a sequence-preserving muxer",
            ));
        }
        let source_alpha = self.samples.get(1);
        let output_size =
            crate::image_edits::output_size(self.samples[0].size, &request.operations)
                .map_err(invalid)?;
        if let Some(alpha_id) = self.color().premultiplied_with
            && source_alpha.is_none_or(|sample| sample.id != alpha_id)
        {
            return Err(invalid("invalid premultiplied alpha reference"));
        }
        let alpha_output = source_alpha.is_some()
            || request
                .operations
                .iter()
                .any(|operation| matches!(operation, EditOperation::RotateImage(_)));
        let mut filters = if let Some(alpha) = source_alpha {
            format!(
                "[0:{}]scale=flags=bilinear,format=rgba[color];[0:{}]format=gray[alpha];[color][alpha]alphamerge",
                self.samples[0].index, alpha.index
            )
        } else {
            format!(
                "[0:{}]scale=flags=bilinear,format=rgba",
                self.samples[0].index
            )
        };
        if self.color().premultiplied_with.is_some() {
            filters.push_str(",unpremultiply=inplace=1");
        }
        for filter in visual_filters(&request.operations) {
            filters.push(',');
            filters.push_str(&filter);
        }
        if alpha_output {
            filters.push_str(",split[color][alpha];[color]format=gbrp[outv];[alpha]alphaextract,format=gray,setparams=colorspace=bt709[outa]");
        } else {
            filters.push_str(",format=gbrp[outv]");
        }
        let graph = staging.directory.join("timeline-filter.txt");
        fs::write(&graph, filters).map_err(ExportError::Output)?;
        let loops = self.color().loops.unwrap_or(1);
        let time_base = self.samples[0].time_base;
        let mut args: Vec<String> = [
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-nostats",
            "-progress",
            "pipe:1",
            "-xerror",
            "-err_detect",
            "explode",
            "-i",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        args.extend([
            request.source.display().to_string(),
            "-/filter_complex".into(),
            graph.display().to_string(),
            "-map".into(),
            "[outv]".into(),
        ]);
        if alpha_output {
            args.extend(["-map", "[outa]"].map(String::from));
            // Alpha extraction retains RGB frame properties, but monochrome AV1
            // cannot signal the identity RGB matrix used by the color track.
            args.extend(["-colorspace:v:1", "bt709"].map(String::from));
        }
        args.extend(
            [
                "-map_metadata",
                "-1",
                "-c:v",
                "libaom-av1",
                "-crf",
                "0",
                "-cpu-used",
                "6",
                "-threads",
                "1",
                "-fps_mode",
                "passthrough",
                "-enc_time_base",
            ]
            .map(String::from),
        );
        args.extend([
            format!("{}/{}", time_base.numerator(), time_base.denominator()),
            "-loop".into(),
            loops.to_string(),
            staging.output.display().to_string(),
        ]);
        let executable = crate::media_tools::tool_path("ffmpeg.exe").map_err(ExportError::Start)?;
        let result = run_ffmpeg(&executable, args, cancelled, progress)?;
        if !result.status.success() {
            return Err(invalid(String::from_utf8_lossy(&result.stderr)));
        }
        if self.color().loops.is_none() {
            // Preserve an undeclared repetition policy without changing box offsets.
            let mut file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&staging.output)
                .map_err(ExportError::Output)?;
            let length = file.metadata().map_err(ExportError::Output)?.len();
            let root = boxes(&mut file, 0, length, cancelled)?;
            let moov = one(&root, b"moov")?.ok_or_else(|| invalid("saved sequence has no moov"))?;
            for item in boxes(&mut file, moov.start, moov.end, cancelled)?
                .into_iter()
                .filter(|item| &item.kind == b"trak")
            {
                if let Some(edit) =
                    one(&boxes(&mut file, item.start, item.end, cancelled)?, b"edts")?
                {
                    file.seek(SeekFrom::Start(edit.header + 4))
                        .map_err(ExportError::Output)?;
                    file.write_all(b"free").map_err(ExportError::Output)?;
                }
            }
        }
        let output = Self::read(&staging.output, cancelled)?
            .ok_or_else(|| invalid("encoder flattened animation"))?;
        if output.color().loops != self.color().loops
            || output.samples.len() != if alpha_output { 2 } else { 1 }
            || output.samples[0].size != output_size
        {
            return Err(invalid("saved loop or alpha controls differ"));
        }
        for (index, actual) in output.samples.iter().enumerate() {
            let expected = self.samples.get(index).unwrap_or(&self.samples[0]);
            if !same_timing(expected, actual) {
                return Err(invalid("saved frame count or timing differs"));
            }
        }
        check_cancelled(cancelled)
    }
}

fn same_timing(left: &Samples, right: &Samples) -> bool {
    let same = |a: i128, b: i128| {
        a * i128::from(left.time_base.numerator()) * i128::from(right.time_base.denominator())
            == b * i128::from(right.time_base.numerator())
                * i128::from(left.time_base.denominator())
    };
    left.times.len() == right.times.len()
        && left
            .times
            .iter()
            .zip(&right.times)
            .all(|(left_time, right_time)| {
                same(
                    i128::from(left_time.0) - i128::from(left.times[0].0),
                    i128::from(right_time.0) - i128::from(right.times[0].0),
                ) && same(i128::from(left_time.1), i128::from(right_time.1))
            })
}

#[cfg(test)]
#[path = "export_avif_tests.rs"]
mod tests;
