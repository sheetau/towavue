use super::*;

#[derive(Debug, PartialEq)]
pub(super) struct Animation {
    pub(super) control: [u8; 6],
    pub(super) delays: Vec<u32>,
}

impl Animation {
    pub(super) fn observe(
        &mut self,
        input: &mut dyn Read,
        size: u32,
        canvas: (u32, u32),
        cancelled: &AtomicBool,
    ) -> Result<bool, ExportError> {
        if size > 512 * 1024 * 1024 {
            return Err(invalid("encoded animation frame exceeds 512 MiB"));
        }
        if size < 24 || self.delays.len() == 65536 {
            return Err(invalid("incomplete ANMF or animation exceeds 65536 frames"));
        }
        let mut frame = [0; 16];
        input.read_exact(&mut frame).map_err(ExportError::Output)?;
        if u24(&frame[..3]) * 2 + u24(&frame[6..9]) + 1 > canvas.0
            || u24(&frame[3..6]) * 2 + u24(&frame[9..12]) + 1 > canvas.1
        {
            return Err(invalid("animation frame exceeds canvas"));
        }
        if u64::from(canvas.0) * u64::from(canvas.1) * 4 > 512 * 1024 * 1024 {
            return Err(invalid("animation canvas exceeds 512 MiB"));
        }
        let alpha = validate_frame(
            input,
            size - 16,
            (u24(&frame[6..9]) + 1, u24(&frame[9..12]) + 1),
            cancelled,
        )?;
        // Pixel bitstreams are decoded before publication; this scan only retains controls.
        self.delays.push(u24(&frame[12..15]));
        Ok(alpha)
    }

    pub(super) fn export(
        &self,
        request: &ExportRequest,
        staging: &StagedExport,
        cancelled: &AtomicBool,
        progress: &(impl Fn(Duration) + Sync),
    ) -> Result<(), ExportError> {
        check_cancelled(cancelled)?;
        progress(Duration::ZERO);
        let input = BufReader::new(Cancellable {
            file: fs::File::open(&request.source).map_err(ExportError::Output)?,
            cancelled,
        });
        // This is the same decoder and default compositing policy as image's display wrapper.
        let mut decoder = image_webp::WebPDecoder::new(input).map_err(failed)?;
        decoder.set_memory_limit(512 * 1024 * 1024);
        if !decoder.is_animated() || decoder.num_frames() as usize != self.delays.len() {
            return Err(invalid("decoded animation frame count differs"));
        }
        let (width, height) = decoder.dimensions();
        let rgba_bytes = u64::from(width) * u64::from(height) * 4;
        if rgba_bytes > 512 * 1024 * 1024 {
            return Err(invalid("animation canvas exceeds 512 MiB"));
        }
        let mut pixels = vec![
            0;
            decoder
                .output_buffer_size()
                .ok_or_else(|| invalid("invalid frame size"))?
        ];
        let mut next_frame = |delay| {
            if decoder.read_frame(&mut pixels).map_err(failed)? != delay {
                return Err(invalid("decoded animation timing differs"));
            }
            let rgba = if decoder.has_alpha() {
                std::mem::take(&mut pixels)
            } else {
                pixels
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
                    .collect()
            };
            let source = crate::DecodedImageFrame {
                width,
                height,
                rgba,
                delay: Duration::from_millis(u64::from(delay)),
            };
            let edited =
                crate::image_edits::render_frame_cancellable(&source, &request.operations, &|| {
                    cancelled.load(Ordering::Relaxed)
                })
                .map_err(failed)?;
            if decoder.has_alpha() {
                pixels = source.rgba;
            } else {
                drop(source);
            }
            Ok(edited)
        };
        if png_metadata::png_path(&request.target) {
            use image::ImageEncoder;
            let mut output = std::io::BufWriter::new(
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&staging.output)
                    .map_err(ExportError::Output)?,
            );
            let mut elapsed = Duration::ZERO;
            for delay in &self.delays {
                check_cancelled(cancelled)?;
                let frame = next_frame(*delay)?;
                image::codecs::png::PngEncoder::new(&mut output)
                    .write_image(
                        &frame.rgba,
                        frame.width,
                        frame.height,
                        image::ExtendedColorType::Rgba8,
                    )
                    .map_err(failed)?;
                elapsed += Duration::from_millis(u64::from((*delay).max(10)));
                progress(elapsed);
            }
            output.flush().map_err(ExportError::Output)?;
            check_cancelled(cancelled)
        } else {
            self.write(&staging.output, next_frame, cancelled, progress)
        }
    }

    pub(super) fn write(
        &self,
        path: &Path,
        mut next_frame: impl FnMut(u32) -> Result<crate::DecodedImageFrame, ExportError>,
        cancelled: &AtomicBool,
        progress: &(impl Fn(Duration) + Sync),
    ) -> Result<(), ExportError> {
        check_cancelled(cancelled)?;
        let mut output = std::io::BufWriter::new(
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(ExportError::Output)?,
        );
        output
            .write_all(b"RIFF\0\0\0\0WEBP")
            .map_err(ExportError::Output)?;
        let mut canvas = None;
        let mut elapsed = Duration::ZERO;
        for delay in &self.delays {
            check_cancelled(cancelled)?;
            let edited = next_frame(*delay)?;
            let size = (edited.width, edited.height);
            if size.0 > 16384 || size.1 > 16384 {
                return Err(invalid("WebP frame dimensions exceed 16384"));
            }
            if let Some(canvas) = canvas {
                if canvas != size {
                    return Err(invalid("edited frame dimensions differ"));
                }
            } else {
                canvas = Some(size);
                let mut extended = [0; 10];
                extended[0] = 2 | 16;
                extended[4..7].copy_from_slice(&(size.0 - 1).to_le_bytes()[..3]);
                extended[7..10].copy_from_slice(&(size.1 - 1).to_le_bytes()[..3]);
                chunk(&mut output, b"VP8X", &extended)?;
                chunk(&mut output, b"ANIM", &self.control)?;
            }
            let mut encoded = Vec::new();
            image::codecs::webp::WebPEncoder::new_lossless(&mut encoded)
                .encode(
                    &edited.rgba,
                    size.0,
                    size.1,
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(failed)?;
            check_cancelled(cancelled)?;
            // The pinned lossless encoder emits one simple VP8L chunk. Embed its padded
            // bitstream unchanged, with full-canvas NO_BLEND/NO_DISPOSE snapshots.
            if encoded.get(12..16) != Some(b"VP8L") {
                return Err(invalid("expected lossless VP8L frame"));
            }
            let payload = &encoded[12..];
            let length = 16 + payload.len();
            riff_size(output.stream_position().map_err(ExportError::Output)? + 8 + length as u64)?;
            output.write_all(b"ANMF").map_err(ExportError::Output)?;
            output
                .write_all(&(length as u32).to_le_bytes())
                .map_err(ExportError::Output)?;
            let mut header = [0; 16];
            header[6..9].copy_from_slice(&(size.0 - 1).to_le_bytes()[..3]);
            header[9..12].copy_from_slice(&(size.1 - 1).to_le_bytes()[..3]);
            header[12..15].copy_from_slice(&delay.to_le_bytes()[..3]);
            header[15] = 2;
            output.write_all(&header).map_err(ExportError::Output)?;
            for bytes in payload.chunks(65536) {
                check_cancelled(cancelled)?;
                output.write_all(bytes).map_err(ExportError::Output)?;
            }
            elapsed += Duration::from_millis(u64::from((*delay).max(10)));
            progress(elapsed);
        }
        let length = riff_size(output.stream_position().map_err(ExportError::Output)?)?;
        output
            .seek(SeekFrom::Start(4))
            .map_err(ExportError::Output)?;
        output
            .write_all(&length.to_le_bytes())
            .map_err(ExportError::Output)?;
        output.flush().map_err(ExportError::Output)?;
        check_cancelled(cancelled)
    }
}

fn validate_frame(
    input: &mut dyn Read,
    mut remaining: u32,
    dimensions: (u32, u32),
    cancelled: &AtomicBool,
) -> Result<bool, ExportError> {
    let (mut image, mut alpha, mut count) = (false, false, 0);
    while remaining != 0 {
        check_cancelled(cancelled)?;
        count += 1;
        if remaining < 8 || count > 65536 {
            return Err(invalid("invalid ANMF subchunk boundary"));
        }
        let mut header = [0; 8];
        input.read_exact(&mut header).map_err(ExportError::Output)?;
        let length = u32::from_le_bytes(header[4..].try_into().expect("subchunk size"));
        let consumed = 8 + u64::from(length) + u64::from(length % 2);
        if consumed > u64::from(remaining) {
            return Err(invalid("subchunk exceeds ANMF length"));
        }
        remaining -= consumed as u32;
        let mut payload = (&mut *input).take(u64::from(length));
        match &header[..4] {
            b"ALPH" => {
                if alpha || image || length == 0 {
                    return Err(invalid("invalid frame ALPH order"));
                }
                alpha = true;
            }
            b"VP8 " | b"VP8L" => {
                if image {
                    return Err(invalid("multiple frame bitstreams"));
                }
                let size = if &header[..4] == b"VP8L" {
                    if alpha || length < 5 {
                        return Err(invalid("invalid frame VP8L layout"));
                    }
                    let mut bytes = [0; 5];
                    payload
                        .read_exact(&mut bytes)
                        .map_err(ExportError::Output)?;
                    let packed = u32::from_le_bytes(bytes[1..].try_into().expect("VP8L header"));
                    if bytes[0] != 0x2f || packed >> 29 != 0 {
                        return Err(invalid("invalid frame VP8L header"));
                    }
                    alpha = packed & (1 << 28) != 0;
                    ((packed & 0x3fff) + 1, ((packed >> 14) & 0x3fff) + 1)
                } else {
                    if length < 10 {
                        return Err(invalid("incomplete frame VP8 header"));
                    }
                    let mut bytes = [0; 10];
                    payload
                        .read_exact(&mut bytes)
                        .map_err(ExportError::Output)?;
                    if bytes[0] & 1 != 0 || bytes[3..6] != [0x9d, 1, 0x2a] {
                        return Err(invalid("invalid frame VP8 keyframe"));
                    }
                    (
                        u32::from(u16::from_le_bytes([bytes[6], bytes[7]]) & 0x3fff),
                        u32::from(u16::from_le_bytes([bytes[8], bytes[9]]) & 0x3fff),
                    )
                };
                if size != dimensions {
                    return Err(invalid("frame and bitstream dimensions differ"));
                }
                image = true;
            }
            _ => {}
        }
        copy(&mut payload, &mut std::io::sink(), cancelled)?;
        if payload.limit() != 0 {
            return Err(invalid("truncated ANMF bitstream"));
        }
        if length % 2 != 0 {
            let mut pad = [0];
            input.read_exact(&mut pad).map_err(ExportError::Output)?;
            if pad != [0] {
                return Err(invalid("nonzero ANMF padding"));
            }
        }
    }
    if !image {
        return Err(invalid("missing frame bitstream"));
    }
    Ok(alpha)
}

fn riff_size(length: u64) -> Result<u32, ExportError> {
    if !(12..=u64::from(u32::MAX) - 1).contains(&length) || !length.is_multiple_of(2) {
        return Err(invalid("animation exceeds RIFF length limit"));
    }
    Ok((length - 8) as u32)
}

fn failed(error: impl std::fmt::Display) -> ExportError {
    invalid(&error.to_string())
}

pub(super) struct Cancellable<'a> {
    pub(super) file: fs::File,
    pub(super) cancelled: &'a AtomicBool,
}
impl Read for Cancellable<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("WebP export cancelled"));
        }
        let limit = bytes.len().min(65536);
        self.file.read(&mut bytes[..limit])
    }
}
impl Seek for Cancellable<'_> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("WebP export cancelled"));
        }
        self.file.seek(position)
    }
}

#[cfg(test)]
#[path = "export_webp_animation_tests.rs"]
mod tests;
