use super::*;
use std::io::{Seek, SeekFrom, Write};
use towavue_core::localization::Text;

#[path = "export_webp_animation.rs"]
mod animation;

fn invalid_reason(reason: Text) -> ExportError {
    crate::ExportFailure::reason(Text::ImageEditWebpContext, reason).into()
}

fn invalid(message: &str) -> ExportError {
    crate::ExportFailure::diagnostic(Text::ImageEditWebpContext, message).into()
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
        return Err(invalid_reason(
            Text::ExportValidationInvalidRiffWebpHeaderOrLength,
        ));
    }
    let mut remaining = u64::from(size) - 4;
    let mut count = 0;
    while remaining != 0 {
        check_cancelled(cancelled)?;
        if remaining < 8 {
            return Err(invalid_reason(
                Text::ExportValidationInvalidChunkBoundaryOrTooManyChunks,
            ));
        }
        let mut chunk = [0; 8];
        input.read_exact(&mut chunk).map_err(ExportError::Output)?;
        // Animation frames have their own bound; keep the existing ancillary-chunk bound.
        count += usize::from(&chunk[..4] != b"ANMF");
        if count > 65536 {
            return Err(invalid_reason(Text::ExportValidationTooManyChunks));
        }
        let length = u32::from_le_bytes(chunk[4..].try_into().expect("chunk size"));
        remaining = remaining
            .checked_sub(8 + u64::from(length) + u64::from(length % 2))
            .ok_or_else(|| invalid_reason(Text::ExportValidationChunkExceedsRiffLength))?;
        let mut payload = (&mut input).take(u64::from(length));
        visit(chunk[..4].try_into().expect("FourCC"), length, &mut payload)?;
        copy(&mut payload, &mut std::io::sink(), cancelled)?;
        if payload.limit() != 0 {
            return Err(invalid_reason(Text::ExportValidationTruncatedChunk));
        }
        if length % 2 != 0 {
            let mut pad = [0];
            input.read_exact(&mut pad).map_err(ExportError::Output)?;
            if pad != [0] {
                return Err(invalid_reason(Text::ExportValidationNonzeroRiffPadding));
            }
        }
    }
    if input.read(&mut [0]).map_err(ExportError::Output)? != 0 {
        return Err(invalid_reason(Text::ExportValidationTrailingDataAfterRiff));
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
    animation: Option<animation::Animation>,
}

pub(super) fn export_traits(
    path: &Path,
    cancelled: &AtomicBool,
) -> Result<(bool, bool), ExportError> {
    let info = container(
        BufReader::new(fs::File::open(path).map_err(ExportError::Output)?),
        cancelled,
    )?;
    Ok((info.alpha, info.animation.is_some()))
}

fn u24(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0])
}

fn container(input: impl Read, cancelled: &AtomicBool) -> Result<Container, ExportError> {
    let mut extended = None;
    let mut image = None;
    let (mut alpha, mut icc, mut exif) = (false, false, false);
    let mut packet = None;
    let mut animation = None;
    let mut index = 0;
    let length = walk(
        input,
        |kind, size, input| {
            match &kind {
                b"VP8X" => {
                    if index != 0 || size < 10 {
                        return Err(invalid_reason(
                            Text::ExportValidationVp8xMustBeTheFirstChunkWithAtLeast10Bytes,
                        ));
                    }
                    let mut bytes = [0; 10];
                    input.read_exact(&mut bytes).map_err(ExportError::Output)?;
                    extended = Some(bytes);
                }
                b"ANIM" => {
                    if size != 6
                        || image.is_some()
                        || alpha
                        || animation.is_some()
                        || extended.as_ref().is_none_or(|bytes| bytes[0] & 2 == 0)
                    {
                        return Err(invalid_reason(
                            Text::ExportValidationInvalidAnimOrderOrLayout,
                        ));
                    }
                    let mut control = [0; 6];
                    input
                        .read_exact(&mut control)
                        .map_err(ExportError::Output)?;
                    animation = Some(animation::Animation {
                        control,
                        delays: Vec::new(),
                    });
                }
                b"ANMF" => {
                    let animation = animation
                        .as_mut()
                        .ok_or_else(|| invalid_reason(Text::ExportValidationAnmfRequiresAnim))?;
                    let canvas = extended.as_ref().expect("ANIM requires VP8X");
                    let frame_alpha = animation.observe(
                        input,
                        size,
                        (u24(&canvas[4..7]) + 1, u24(&canvas[7..10]) + 1),
                        cancelled,
                    )?;
                    if frame_alpha && canvas[0] & 16 == 0 {
                        return Err(invalid_reason(
                            Text::ExportValidationFrameAlphaRequiresVp8xAlphaFlag,
                        ));
                    }
                }
                b"VP8 " => {
                    if image.is_some() || animation.is_some() || size < 10 {
                        return Err(invalid_reason(
                            Text::ExportValidationExpectedOneCompleteImageBitstream,
                        ));
                    }
                    let mut bytes = [0; 10];
                    input.read_exact(&mut bytes).map_err(ExportError::Output)?;
                    if bytes[0] & 1 != 0 || bytes[3..6] != [0x9d, 1, 0x2a] {
                        return Err(invalid_reason(
                            Text::ExportValidationInvalidVp8KeyframeHeader,
                        ));
                    }
                    image = Some((
                        u32::from(u16::from_le_bytes([bytes[6], bytes[7]]) & 0x3fff),
                        u32::from(u16::from_le_bytes([bytes[8], bytes[9]]) & 0x3fff),
                        false,
                    ));
                }
                b"VP8L" => {
                    if image.is_some() || animation.is_some() || alpha || size < 5 {
                        return Err(invalid_reason(
                            Text::ExportValidationInvalidLosslessImageLayout,
                        ));
                    }
                    let mut bytes = [0; 5];
                    input.read_exact(&mut bytes).map_err(ExportError::Output)?;
                    let packed = u32::from_le_bytes(bytes[1..].try_into().expect("VP8L header"));
                    if bytes[0] != 0x2f || packed >> 29 != 0 {
                        return Err(invalid_reason(Text::ExportValidationInvalidVp8lHeader));
                    }
                    image = Some((
                        (packed & 0x3fff) + 1,
                        ((packed >> 14) & 0x3fff) + 1,
                        packed & (1 << 28) != 0,
                    ));
                }
                b"ALPH" => {
                    if alpha
                        || image.is_some()
                        || animation.is_some()
                        || extended.is_none()
                        || size == 0
                    {
                        return Err(invalid_reason(
                            Text::ExportValidationInvalidAlphOrderOrLayout,
                        ));
                    }
                    alpha = true;
                }
                b"ICCP" => {
                    if icc || image.is_some() || animation.is_some() {
                        return Err(invalid_reason(
                            Text::ExportValidationInvalidIccpOrderOrDuplication,
                        ));
                    }
                    icc = true;
                }
                b"EXIF" => {
                    if exif {
                        return Err(invalid_reason(Text::ExportValidationDuplicateExifChunk));
                    }
                    exif = true;
                }
                b"XMP " => {
                    if packet.is_some() || size as usize > xmp::LIMIT {
                        return Err(invalid_reason(
                            Text::ExportValidationMultipleOrOversizedXmpPackets,
                        ));
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
    let (width, height, lossless_alpha) = if let Some(animation) = &animation {
        if animation.delays.is_empty() {
            return Err(invalid_reason(Text::ExportValidationAnimationHasNoFrames));
        }
        let canvas = extended.as_ref().expect("ANIM requires VP8X");
        (
            u24(&canvas[4..7]) + 1,
            u24(&canvas[7..10]) + 1,
            canvas[0] & 16 != 0,
        )
    } else {
        image.ok_or_else(|| invalid_reason(Text::ExportValidationMissingStillImageBitstream))?
    };
    if width == 0 || height == 0 {
        return Err(invalid_reason(
            Text::ExportValidationInvalidCanvasDimensions,
        ));
    }
    if let Some(bytes) = extended {
        if u24(&bytes[4..7]) + 1 != width || u24(&bytes[7..10]) + 1 != height {
            return Err(invalid_reason(
                Text::ExportValidationCanvasAndBitstreamDimensionsDiffer,
            ));
        }
        if (bytes[0] & 4 != 0) != packet.is_some()
            || (bytes[0] & 2 != 0) != animation.is_some()
            || (bytes[0] & 8 != 0) != exif
            || (bytes[0] & 32 != 0) != icc
            || (alpha && bytes[0] & 16 == 0)
        {
            return Err(invalid_reason(
                Text::ExportValidationFeatureFlagsAndChunksDisagree,
            ));
        }
    } else if packet.is_some() || alpha || icc || exif {
        return Err(invalid_reason(
            Text::ExportValidationExtendedChunksRequireVp8x,
        ));
    }
    Ok(Container {
        length,
        extended: extended.is_some(),
        width,
        height,
        alpha: lossless_alpha || alpha,
        packet,
        animation,
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

pub(super) fn read_for_jpeg(
    path: &Path,
    cancelled: &AtomicBool,
) -> Result<Option<Vec<u8>>, ExportError> {
    let info = container(
        BufReader::new(fs::File::open(path).map_err(ExportError::Output)?),
        cancelled,
    )?;
    if info.animation.is_some() {
        return Err(invalid_reason(
            Text::ExportValidationAnimatedWebpCannotExportToJpegWithoutDiscardingFrames,
        ));
    }
    Ok(info.packet)
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
        return Err(invalid_reason(
            Text::ExportValidationMetadataWouldExceedRiffLengthLimit,
        ));
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
        return Err(invalid_reason(
            Text::ExportValidationRewrittenLengthDiffersFromExpectedSize,
        ));
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
    animation: Option<animation::Animation>,
}

pub(super) fn apply_png_frames(
    staging: &StagedExport,
    delays: Vec<u32>,
    plays: u16,
    cancelled: &AtomicBool,
    progress: &(impl Fn(Duration) + Sync),
) -> Result<(), ExportError> {
    let result = (|| {
        let mut control = [0; 6];
        control[4..].copy_from_slice(&plays.to_le_bytes());
        let animation = animation::Animation { control, delays };
        let temporary = staging.directory.join("animation.webp");
        {
            let mut input = BufReader::new(animation::Cancellable {
                file: fs::File::open(&staging.output).map_err(ExportError::Output)?,
                cancelled,
            });
            animation.write(
                &temporary,
                |delay| {
                    let (width, height, rgba) = gif_animation::read_png(&mut input)?;
                    Ok(crate::DecodedImageFrame {
                        width: u32::from(width),
                        height: u32::from(height),
                        rgba,
                        delay: Duration::from_millis(u64::from(delay)),
                    })
                },
                cancelled,
                progress,
            )?;
            if input.read(&mut [0]).map_err(ExportError::Output)? != 0 {
                return Err(invalid_reason(Text::ExportValidationExtraEncodedPngFrames));
            }
        }
        let info = container(
            BufReader::new(fs::File::open(&temporary).map_err(ExportError::Output)?),
            cancelled,
        )?;
        if info.animation.as_ref() != Some(&animation) {
            return Err(invalid_reason(
                Text::ExportValidationConvertedAnimationControlsDiffer,
            ));
        }
        check_cancelled(cancelled)?;
        fs::rename(&temporary, &staging.output).map_err(ExportError::Output)
    })();
    check_cancelled(cancelled)?;
    result
}

impl WebpMetadata {
    pub(super) fn prepare(
        request: &ExportRequest,
        options: &MetadataExportOptions,
        cancelled: &AtomicBool,
    ) -> Result<Self, ExportError> {
        if !(webp_path(&request.source) || jpeg_metadata::jpeg_path(&request.source))
            || !webp_path(&request.target)
        {
            return Err(invalid_reason(
                Text::ExportValidationXmpExportRequiresJpegOrWebpInputAndWebpOutput,
            ));
        }
        let (animation, source_packet) = if jpeg_metadata::jpeg_path(&request.source) {
            (
                None,
                jpeg_metadata::read_packet(&request.source, cancelled)?,
            )
        } else {
            let info = container(
                BufReader::new(fs::File::open(&request.source).map_err(ExportError::Output)?),
                cancelled,
            )?;
            (info.animation, info.packet)
        };
        let mut values = source_packet
            .as_deref()
            .map(|packet| xmp::parse(packet, cancelled))
            .transpose()?
            .unwrap_or_default();
        xmp::apply(&mut values, options)?;
        let packet = if let Some(packet) = source_packet {
            if request.operations.is_empty() && webp_path(&request.source) {
                xmp::rewrite_unedited(&packet, options, cancelled)?
            } else {
                xmp::rewrite_edited(&packet, options, cancelled)?
            }
        } else if values.is_empty() {
            Vec::new()
        } else {
            xmp::encode(&values)?
        };
        Ok(Self {
            values,
            packet,
            animation,
        })
    }

    pub(super) fn is_animated(&self) -> bool {
        self.animation.is_some()
    }

    pub(super) fn export_animation(
        &self,
        request: &ExportRequest,
        staging: &StagedExport,
        cancelled: &AtomicBool,
        progress: &(impl Fn(Duration) + Sync),
    ) -> Result<(), ExportError> {
        let result = (|| {
            self.animation
                .as_ref()
                .expect("animated source")
                .export(request, staging, cancelled, progress)?;
            self.apply(staging, cancelled)
        })();
        check_cancelled(cancelled)?;
        result
    }

    pub(super) fn apply(
        &self,
        staging: &StagedExport,
        cancelled: &AtomicBool,
    ) -> Result<(), ExportError> {
        self.apply_from(&staging.output, staging, cancelled)
    }

    pub(super) fn export_unedited(
        &self,
        source: &Path,
        staging: &StagedExport,
        cancelled: &AtomicBool,
        progress: &(impl Fn(Duration) + Sync),
    ) -> Result<(), ExportError> {
        let result = (|| {
            check_cancelled(cancelled)?;
            progress(Duration::ZERO);
            let input = BufReader::new(animation::Cancellable {
                file: fs::File::open(source).map_err(ExportError::Output)?,
                cancelled,
            });
            // Validate every compressed frame with the display decoder before copying;
            // the container scan alone checks headers, not decodability of pixel data.
            let mut decoder =
                image_webp::WebPDecoder::new(input).map_err(|error| invalid(&error.to_string()))?;
            decoder.set_memory_limit(512 * 1024 * 1024);
            let (width, height) = decoder.dimensions();
            if u64::from(width) * u64::from(height) * 4 > 512 * 1024 * 1024 {
                return Err(invalid_reason(
                    Text::ExportValidationValidationCanvasExceeds512Mib,
                ));
            }
            if decoder.is_animated() != self.animation.is_some()
                || self.animation.as_ref().is_some_and(|animation| {
                    animation.delays.len() != decoder.num_frames() as usize
                })
            {
                return Err(invalid_reason(
                    Text::ExportValidationDecodedAnimationFrameCountDiffers,
                ));
            }
            let mut pixels = vec![
                0;
                decoder.output_buffer_size().ok_or_else(|| invalid_reason(
                    Text::ExportValidationInvalidFrameSize
                ))?
            ];
            if let Some(animation) = &self.animation {
                let mut elapsed = Duration::ZERO;
                for delay in &animation.delays {
                    check_cancelled(cancelled)?;
                    if decoder
                        .read_frame(&mut pixels)
                        .map_err(|error| invalid(&error.to_string()))?
                        != *delay
                    {
                        return Err(invalid_reason(
                            Text::ExportValidationDecodedAnimationTimingDiffers,
                        ));
                    }
                    elapsed += Duration::from_millis(u64::from((*delay).max(10)));
                    progress(elapsed);
                }
            } else {
                decoder
                    .read_image(&mut pixels)
                    .map_err(|error| invalid(&error.to_string()))?;
            }
            drop(pixels);
            drop(decoder);
            check_cancelled(cancelled)?;
            self.apply_from(source, staging, cancelled)
        })();
        check_cancelled(cancelled)?;
        result
    }

    fn apply_from(
        &self,
        source: &Path,
        staging: &StagedExport,
        cancelled: &AtomicBool,
    ) -> Result<(), ExportError> {
        let info = container(
            BufReader::new(fs::File::open(source).map_err(ExportError::Output)?),
            cancelled,
        )?;
        if info.animation != self.animation {
            return Err(invalid_reason(
                Text::ExportValidationSavedAnimationControlsDiffer,
            ));
        }
        let temporary = staging.directory.join("metadata.webp");
        {
            let input = BufReader::new(fs::File::open(source).map_err(ExportError::Output)?);
            let mut output = std::io::BufWriter::new(
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)
                    .map_err(ExportError::Output)?,
            );
            rewrite(input, &mut output, &info, &self.packet, cancelled)?;
        }
        let packet = container(
            BufReader::new(fs::File::open(&temporary).map_err(ExportError::Output)?),
            cancelled,
        )?
        .packet;
        if packet.as_deref().unwrap_or_default() != self.packet {
            return Err(invalid_reason(
                Text::ExportValidationStagedXmpDiffersFromThePreparedPacket,
            ));
        }
        if packet
            .map(|packet| xmp::parse(&packet, cancelled))
            .transpose()?
            .unwrap_or_default()
            != self.values
        {
            return Err(invalid_reason(
                Text::ExportValidationStagedXmpDidNotRetainRequestedValues,
            ));
        }
        check_cancelled(cancelled)?;
        fs::rename(&temporary, &staging.output).map_err(ExportError::Output)
    }
}

pub(super) struct SnapshotConversion {
    animation: animation::Animation,
    output: SnapshotOutput,
}

enum SnapshotOutput {
    Png(png_metadata::PngMetadata),
    Gif(gif_animation::Animation),
    Avif(u16),
}

impl SnapshotConversion {
    pub(super) fn prepare(
        path: &Path,
        target: &Path,
        cancelled: &AtomicBool,
    ) -> Result<Option<Self>, ExportError> {
        let Some(animation) = container(
            BufReader::new(fs::File::open(path).map_err(ExportError::Output)?),
            cancelled,
        )?
        .animation
        else {
            return Ok(None);
        };
        let plays = u16::from_le_bytes(animation.control[4..].try_into().expect("loop count"));
        if avif::avif_path(target) {
            if animation.delays.contains(&0) {
                return Err(invalid_reason(Text::ExportValidationAvifRequiresPositiveSampleDurationsUseWebpOutputToRetainZeroDelays,
                ));
            }
            return Ok(Some(Self {
                animation,
                output: SnapshotOutput::Avif(plays),
            }));
        }
        if gif_animation::gif_path(target) {
            let gif = gif_animation::Animation::from_milliseconds(plays, &animation.delays)?;
            return Ok(Some(Self {
                animation,
                output: SnapshotOutput::Gif(gif),
            }));
        }
        let delays = animation
            .delays
            .iter()
            .map(|delay| {
                // Reduce milliseconds/1000 before checking APNG's unsigned 16-bit fraction.
                // Rounding or splitting a hold would change timing or frame count.
                let (mut divisor, mut remainder) = (*delay, 1000);
                while remainder != 0 {
                    (divisor, remainder) = (remainder, divisor % remainder);
                }
                let numerator = u16::try_from(*delay / divisor).map_err(|_| {
                    crate::ExportFailure::frame_delay(
                        Text::ImageEditWebpContext,
                        Some(*delay),
                        "APNG",
                        "WebP",
                    )
                })?;
                let denominator = (1000 / divisor) as u16;
                let [a, b] = numerator.to_be_bytes();
                let [c, d] = denominator.to_be_bytes();
                Ok([a, b, c, d])
            })
            .collect::<Result<Vec<_>, ExportError>>()?;
        Ok(Some(Self {
            animation,
            output: SnapshotOutput::Png(png_metadata::PngMetadata::from_animation(
                u32::from(plays),
                delays,
            )),
        }))
    }

    pub(super) fn export(
        &self,
        request: &ExportRequest,
        staging: &StagedExport,
        cancelled: &AtomicBool,
        progress: &(impl Fn(Duration) + Sync),
    ) -> Result<(), ExportError> {
        let result = (|| {
            self.animation
                .export(request, staging, cancelled, progress)?;
            match &self.output {
                SnapshotOutput::Png(png) => png.apply(staging, cancelled),
                SnapshotOutput::Gif(gif) => gif.apply(staging, cancelled),
                SnapshotOutput::Avif(plays) => avif::apply_png_frames(
                    staging,
                    &self.animation.delays,
                    1000,
                    u32::from(*plays),
                    cancelled,
                    progress,
                ),
            }
        })();
        check_cancelled(cancelled)?;
        result
    }
}

pub(super) fn require_static(path: &Path, cancelled: &AtomicBool) -> Result<(), ExportError> {
    if container(
        BufReader::new(fs::File::open(path).map_err(ExportError::Output)?),
        cancelled,
    )?
    .animation
    .is_some()
    {
        return Err(invalid_reason(Text::ExportValidationAnimatedWebpConversionMustPreserveFramesUseWebpApngPngApngOrGifOutput,
        ));
    }
    Ok(())
}
