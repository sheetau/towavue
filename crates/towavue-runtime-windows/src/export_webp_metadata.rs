use super::*;
use std::io::{Seek, SeekFrom, Write};

fn invalid(message: &str) -> ExportError {
    ExportError::Failed(format!("WebP metadata: {message}"))
}

pub(super) fn webp_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("webp"))
}

fn copy(
    input: &mut dyn Read,
    output: &mut dyn Write,
    cancelled: &AtomicBool,
) -> Result<(), ExportError> {
    let mut buffer = [0; 65536];
    loop {
        check_cancelled(cancelled)?;
        let count = input.read(&mut buffer).map_err(ExportError::Output)?;
        if count == 0 {
            return Ok(());
        }
        output
            .write_all(&buffer[..count])
            .map_err(ExportError::Output)?;
    }
}

fn walk(
    mut input: impl Read,
    mut visit: impl FnMut([u8; 4], u32, &mut dyn Read) -> Result<(), ExportError>,
    cancelled: &AtomicBool,
) -> Result<u64, ExportError> {
    let mut header = [0; 12];
    input.read_exact(&mut header).map_err(ExportError::Output)?;
    let size = u32::from_le_bytes(header[4..8].try_into().expect("RIFF size"));
    if &header[..4] != b"RIFF"
        || &header[8..] != b"WEBP"
        || !(4..=u32::MAX - 9).contains(&size)
        || size % 2 != 0
    {
        return Err(invalid("invalid RIFF/WEBP header or length"));
    }
    let mut remaining = u64::from(size) - 4;
    let mut count = 0;
    while remaining != 0 {
        check_cancelled(cancelled)?;
        count += 1;
        if remaining < 8 || count > 65536 {
            return Err(invalid("invalid chunk boundary or too many chunks"));
        }
        let mut chunk = [0; 8];
        input.read_exact(&mut chunk).map_err(ExportError::Output)?;
        let length = u32::from_le_bytes(chunk[4..].try_into().expect("chunk size"));
        remaining = remaining
            .checked_sub(8 + u64::from(length) + u64::from(length % 2))
            .ok_or_else(|| invalid("chunk exceeds RIFF length"))?;
        let mut payload = (&mut input).take(u64::from(length));
        visit(chunk[..4].try_into().expect("FourCC"), length, &mut payload)?;
        copy(&mut payload, &mut std::io::sink(), cancelled)?;
        if payload.limit() != 0 {
            return Err(invalid("truncated chunk"));
        }
        if length % 2 != 0 {
            let mut pad = [0];
            input.read_exact(&mut pad).map_err(ExportError::Output)?;
            if pad != [0] {
                return Err(invalid("nonzero RIFF padding"));
            }
        }
    }
    if input.read(&mut [0]).map_err(ExportError::Output)? != 0 {
        return Err(invalid("trailing data after RIFF"));
    }
    Ok(u64::from(size) + 8)
}

struct Container {
    length: u64,
    extended: bool,
    width: u32,
    height: u32,
    alpha: bool,
    packet: Option<Vec<u8>>,
}

fn u24(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0])
}

fn container(input: impl Read, cancelled: &AtomicBool) -> Result<Container, ExportError> {
    let mut extended = None;
    let mut image = None;
    let (mut alpha, mut icc, mut exif) = (false, false, false);
    let mut packet = None;
    let mut index = 0;
    let length = walk(
        input,
        |kind, size, input| {
            match &kind {
                b"VP8X" => {
                    if index != 0 || size < 10 {
                        return Err(invalid(
                            "VP8X must be the first chunk with at least 10 bytes",
                        ));
                    }
                    let mut bytes = [0; 10];
                    input.read_exact(&mut bytes).map_err(ExportError::Output)?;
                    if bytes[0] & 2 != 0 {
                        return Err(invalid(
                            "animated WebP requires animation-preserving export; metadata editing is not supported yet",
                        ));
                    }
                    extended = Some(bytes);
                }
                b"ANIM" | b"ANMF" => {
                    return Err(invalid(
                        "animated WebP requires animation-preserving export; metadata editing is not supported yet",
                    ));
                }
                b"VP8 " => {
                    if image.is_some() || size < 10 {
                        return Err(invalid("expected one complete image bitstream"));
                    }
                    let mut bytes = [0; 10];
                    input.read_exact(&mut bytes).map_err(ExportError::Output)?;
                    if bytes[0] & 1 != 0 || bytes[3..6] != [0x9d, 1, 0x2a] {
                        return Err(invalid("invalid VP8 keyframe header"));
                    }
                    image = Some((
                        u32::from(u16::from_le_bytes([bytes[6], bytes[7]]) & 0x3fff),
                        u32::from(u16::from_le_bytes([bytes[8], bytes[9]]) & 0x3fff),
                        false,
                    ));
                }
                b"VP8L" => {
                    if image.is_some() || alpha || size < 5 {
                        return Err(invalid("invalid lossless image layout"));
                    }
                    let mut bytes = [0; 5];
                    input.read_exact(&mut bytes).map_err(ExportError::Output)?;
                    let packed = u32::from_le_bytes(bytes[1..].try_into().expect("VP8L header"));
                    if bytes[0] != 0x2f || packed >> 29 != 0 {
                        return Err(invalid("invalid VP8L header"));
                    }
                    image = Some((
                        (packed & 0x3fff) + 1,
                        ((packed >> 14) & 0x3fff) + 1,
                        packed & (1 << 28) != 0,
                    ));
                }
                b"ALPH" => {
                    if alpha || image.is_some() || extended.is_none() || size == 0 {
                        return Err(invalid("invalid ALPH order or layout"));
                    }
                    alpha = true;
                }
                b"ICCP" => {
                    if icc || image.is_some() {
                        return Err(invalid("invalid ICCP order or duplication"));
                    }
                    icc = true;
                }
                b"EXIF" => {
                    if exif {
                        return Err(invalid("duplicate EXIF chunk"));
                    }
                    exif = true;
                }
                b"XMP " => {
                    if packet.is_some() || size as usize > xmp::LIMIT {
                        return Err(invalid("multiple or oversized XMP packets"));
                    }
                    let mut bytes = vec![0; size as usize];
                    input.read_exact(&mut bytes).map_err(ExportError::Output)?;
                    packet = Some(bytes);
                }
                _ => {}
            }
            index += 1;
            Ok(())
        },
        cancelled,
    )?;
    let (width, height, lossless_alpha) =
        image.ok_or_else(|| invalid("missing still image bitstream"))?;
    if width == 0 || height == 0 {
        return Err(invalid("invalid canvas dimensions"));
    }
    if let Some(bytes) = extended {
        if u24(&bytes[4..7]) + 1 != width || u24(&bytes[7..10]) + 1 != height {
            return Err(invalid("canvas and bitstream dimensions differ"));
        }
        if (bytes[0] & 4 != 0) != packet.is_some()
            || (bytes[0] & 8 != 0) != exif
            || (bytes[0] & 32 != 0) != icc
            || (alpha && bytes[0] & 16 == 0)
        {
            return Err(invalid("feature flags and chunks disagree"));
        }
    } else if packet.is_some() || alpha || icc || exif {
        return Err(invalid("extended chunks require VP8X"));
    }
    Ok(Container {
        length,
        extended: extended.is_some(),
        width,
        height,
        alpha: lossless_alpha || alpha,
        packet,
    })
}

fn read(path: &Path, cancelled: &AtomicBool) -> Result<Vec<xmp::Value>, ExportError> {
    container(
        BufReader::new(fs::File::open(path).map_err(ExportError::Output)?),
        cancelled,
    )?
    .packet
    .map(|packet| xmp::parse(&packet, cancelled))
    .transpose()
    .map(Option::unwrap_or_default)
}

pub(super) fn inspect(path: &Path) -> Result<Vec<MetadataSourceValue>, ExportError> {
    Ok(xmp::display_values(
        read(path, &AtomicBool::new(false))?,
        ImageMetadataFormat::Webp,
    ))
}

fn chunk(output: &mut dyn Write, kind: &[u8; 4], bytes: &[u8]) -> Result<(), ExportError> {
    output.write_all(kind).map_err(ExportError::Output)?;
    output
        .write_all(&(bytes.len() as u32).to_le_bytes())
        .map_err(ExportError::Output)?;
    output.write_all(bytes).map_err(ExportError::Output)?;
    if !bytes.len().is_multiple_of(2) {
        output.write_all(&[0]).map_err(ExportError::Output)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "export_webp_metadata_tests.rs"]
mod tests;

fn rewrite(
    input: impl Read,
    output: &mut (impl Write + Seek),
    info: &Container,
    packet: &[u8],
    cancelled: &AtomicBool,
) -> Result<(), ExportError> {
    let old_chunk = info
        .packet
        .as_ref()
        .map_or(0, |value| 8 + value.len() as u64 + value.len() as u64 % 2);
    let new_chunk = if packet.is_empty() {
        0
    } else {
        8 + packet.len() as u64 + packet.len() as u64 % 2
    };
    let add_extended = !info.extended && !packet.is_empty();
    let length = info.length - old_chunk + new_chunk + if add_extended { 18 } else { 0 };
    if length > u64::from(u32::MAX) - 1 {
        return Err(invalid("metadata would exceed RIFF length limit"));
    }
    output
        .write_all(b"RIFF\0\0\0\0WEBP")
        .map_err(ExportError::Output)?;
    if add_extended {
        let mut extended = [0; 10];
        extended[0] = 4 | if info.alpha { 16 } else { 0 };
        extended[4..7].copy_from_slice(&(info.width - 1).to_le_bytes()[..3]);
        extended[7..10].copy_from_slice(&(info.height - 1).to_le_bytes()[..3]);
        chunk(output, b"VP8X", &extended)?;
    }
    walk(
        input,
        |kind, size, input| {
            if &kind == b"XMP " {
                return Ok(());
            }
            output.write_all(&kind).map_err(ExportError::Output)?;
            output
                .write_all(&size.to_le_bytes())
                .map_err(ExportError::Output)?;
            if &kind == b"VP8X" {
                let mut flags = [0];
                input.read_exact(&mut flags).map_err(ExportError::Output)?;
                flags[0] = (flags[0] & !4) | if packet.is_empty() { 0 } else { 4 };
                output.write_all(&flags).map_err(ExportError::Output)?;
            }
            copy(input, output, cancelled)?;
            if size % 2 != 0 {
                output.write_all(&[0]).map_err(ExportError::Output)?;
            }
            Ok(())
        },
        cancelled,
    )?;
    if !packet.is_empty() {
        chunk(output, b"XMP ", packet)?;
    }
    if output.stream_position().map_err(ExportError::Output)? != length {
        return Err(invalid("rewritten length differs from expected size"));
    }
    output
        .seek(SeekFrom::Start(4))
        .map_err(ExportError::Output)?;
    output
        .write_all(&((length - 8) as u32).to_le_bytes())
        .map_err(ExportError::Output)?;
    output.flush().map_err(ExportError::Output)?;
    Ok(())
}

pub(super) struct WebpMetadata {
    values: Vec<xmp::Value>,
    packet: Vec<u8>,
}

impl WebpMetadata {
    pub(super) fn prepare(
        request: &ExportRequest,
        options: &MetadataExportOptions,
        cancelled: &AtomicBool,
    ) -> Result<Self, ExportError> {
        if !webp_path(&request.source) || !webp_path(&request.target) {
            return Err(invalid(
                "XMP export requires static WebP input and WebP output",
            ));
        }
        let mut values = read(&request.source, cancelled)?;
        xmp::apply(&mut values, options)?;
        let packet = if values.is_empty() {
            Vec::new()
        } else {
            xmp::encode(&values)?
        };
        Ok(Self { values, packet })
    }

    pub(super) fn apply(
        &self,
        staging: &StagedExport,
        cancelled: &AtomicBool,
    ) -> Result<(), ExportError> {
        let info = container(
            BufReader::new(fs::File::open(&staging.output).map_err(ExportError::Output)?),
            cancelled,
        )?;
        let temporary = staging.directory.join("metadata.webp");
        {
            let input =
                BufReader::new(fs::File::open(&staging.output).map_err(ExportError::Output)?);
            let mut output = std::io::BufWriter::new(
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)
                    .map_err(ExportError::Output)?,
            );
            rewrite(input, &mut output, &info, &self.packet, cancelled)?;
        }
        if read(&temporary, cancelled)? != self.values {
            return Err(invalid("staged XMP did not retain requested values"));
        }
        check_cancelled(cancelled)?;
        fs::rename(&temporary, &staging.output).map_err(ExportError::Output)
    }
}
