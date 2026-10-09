use super::*;
use image::codecs::tiff::TiffDecoder;
use std::io::{BufRead, Cursor};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use tiff::decoder::{ChunkType, Decoder};
use tiff::tags::Tag;

const INPUT_LIMIT: usize = 64 * 1024 * 1024;
const MIN_RGB_BYTES: usize = 1024 * 1024;
const MAX_WORKERS: usize = 8;

pub(super) fn decode(
    mut input: impl BufRead + Seek,
    byte_limit: usize,
    current: &dyn Fn() -> bool,
) -> Result<DecodedImageFrame, ImageDecodeError> {
    check_current(current)?;
    let mut adapter = TiffDecoder::new(&mut input)?;
    adapter.set_limits(image::Limits::default())?;
    let (width, height) = adapter.dimensions();
    let rgba_bytes = (u64::from(width) * u64::from(height))
        .checked_mul(4)
        .ok_or(ImageDecodeError::TooLarge)?;
    if rgba_bytes > byte_limit as u64 {
        return Err(ImageDecodeError::TooLarge);
    }
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(MAX_WORKERS);
    if workers < 2
        || adapter.original_color_type() != image::ExtendedColorType::Rgb8
        || rgba_bytes / 4 * 3 < MIN_RGB_BYTES as u64
        || rgba_bytes > IMAGE_BYTE_LIMIT as u64
    {
        return static_frame(adapter, byte_limit, current);
    }
    let orientation = adapter.orientation()?;
    drop(adapter);
    input.rewind().map_err(ImageDecodeError::Open)?;
    let eligible = Decoder::new(&mut input).is_ok_and(|mut header| {
        header.get_chunk_type() == ChunkType::Strip
            && header.strip_count().is_ok_and(|count| count >= 2)
            && header
                .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)
                .is_ok_and(|value| value.unwrap_or(1) == 1)
    });
    if !eligible {
        check_current(current)?;
        input.rewind().map_err(ImageDecodeError::Open)?;
        return static_frame(TiffDecoder::new(input)?, byte_limit, current);
    }
    let length = input
        .seek(SeekFrom::End(0))
        .map_err(ImageDecodeError::Open)?;
    input.rewind().map_err(ImageDecodeError::Open)?;
    if length <= INPUT_LIMIT as u64 {
        let mut encoded = Vec::with_capacity(length as usize);
        // The original cancellable reader bounds reads to 64 KiB. Taking one
        // extra byte also detects a growing file without unbounded allocation.
        input
            .by_ref()
            .take(INPUT_LIMIT as u64 + 1)
            .read_to_end(&mut encoded)
            .map_err(ImageDecodeError::Open)?;
        check_current(current)?;
        let rgb = (encoded.len() <= INPUT_LIMIT)
            .then(|| parallel_rgb(&encoded, (width, height), workers, current))
            .flatten();
        // Release the immutable compressed snapshot before orientation/RGBA
        // conversion. All scoped workers have joined before it can be released.
        drop(encoded);
        check_current(current)?;
        if let Some(rgb) = rgb {
            let mut image = DynamicImage::ImageRgb8(
                image::RgbImage::from_raw(width, height, rgb).expect("packed TIFF RGB"),
            );
            image.apply_orientation(orientation);
            check_current(current)?;
            let image = image.into_rgba8();
            return Ok(DecodedImageFrame {
                width: image.width(),
                height: image.height(),
                rgba: image.into_raw(),
                delay: Duration::ZERO,
            });
        }
    }
    // Unsupported layouts, worker/resource failures and damaged chunks retain
    // the image adapter's original decoding and diagnostics. Cancellation never
    // retries. Reuse the original file handle rather than reopening its path.
    check_current(current)?;
    input.rewind().map_err(ImageDecodeError::Open)?;
    static_frame(TiffDecoder::new(input)?, byte_limit, current)
}

fn parallel_rgb(
    encoded: &[u8],
    size: (u32, u32),
    workers: usize,
    current: &dyn Fn() -> bool,
) -> Option<Vec<u8>> {
    let mut header = Decoder::new(Cursor::new(encoded)).ok()?;
    if header.dimensions().ok()? != size
        || header.colortype().ok()? != tiff::ColorType::RGB(8)
        || header.get_chunk_type() != ChunkType::Strip
        || header
            .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)
            .ok()?
            .unwrap_or(1)
            != 1
    {
        return None;
    }
    let strips = header.strip_count().ok()? as usize;
    let (width, rows) = header.chunk_dimensions();
    if width != size.0 || rows == 0 || strips < 2 {
        return None;
    }
    let workers = workers.clamp(1, MAX_WORKERS).min(strips);
    let group = strips.div_ceil(workers);
    let row_bytes = (width as usize).checked_mul(3)?;
    let strip_bytes = row_bytes.checked_mul(rows as usize)?;
    let group_bytes = strip_bytes.checked_mul(group)?;
    let rgb_bytes = row_bytes.checked_mul(size.1 as usize)?;
    if strips != (size.1 as usize).div_ceil(rows as usize)
        || rgb_bytes == 0
        || rgb_bytes > IMAGE_BYTE_LIMIT
    {
        return None;
    }
    // Split the adapter's remaining allocation allowance among workers, also
    // accounting for the shared compressed snapshot. Native stacks and metadata
    // remain additional overhead; this is not a whole-process memory cap.
    let intermediate = IMAGE_BYTE_LIMIT
        .checked_sub(rgb_bytes)?
        .checked_sub(encoded.len())?
        / workers;
    if intermediate == 0 {
        return None;
    }
    drop(header);
    let mut rgb = vec![0; rgb_bytes];
    let stop = AtomicBool::new(false);
    let success = std::thread::scope(|scope| {
        let (sender, receiver) = mpsc::sync_channel(workers);
        let mut handles = Vec::new();
        let mut success = true;
        for (index, destination) in rgb.chunks_mut(group_bytes).enumerate() {
            let stop = &stop;
            let sender = sender.clone();
            let job = std::thread::Builder::new()
                .name("towavue-tiff-strip".into())
                .spawn_scoped(scope, move || {
                    let current = || !stop.load(Ordering::Relaxed);
                    let result = (|| {
                        let reader = BufReader::new(CancellableReader {
                            inner: Cursor::new(encoded),
                            is_current: &current,
                        });
                        let mut limits = tiff::decoder::Limits::default();
                        limits.decoding_buffer_size = rgb_bytes;
                        limits.intermediate_buffer_size = intermediate;
                        limits.ifd_value_size = intermediate;
                        let mut decoder = Decoder::new(reader).ok()?.with_limits(limits);
                        for (local, chunk) in destination.chunks_mut(strip_bytes).enumerate() {
                            if !current() {
                                return None;
                            }
                            let strip = u32::try_from(index * group + local).ok()?;
                            let dimensions = decoder.chunk_data_dimensions(strip);
                            if dimensions.0 != size.0
                                || dimensions.1 as usize * row_bytes != chunk.len()
                            {
                                return None;
                            }
                            decoder.read_chunk_bytes(strip, chunk).ok()?;
                        }
                        Some(())
                    })()
                    .is_some();
                    if !result {
                        stop.store(true, Ordering::Relaxed);
                    }
                    let _ = sender.send(result);
                    result
                });
            match job {
                Ok(handle) => handles.push(handle),
                Err(_) => {
                    success = false;
                    stop.store(true, Ordering::Relaxed);
                    break;
                }
            }
        }
        drop(sender);
        let mut completed = 0;
        while completed < handles.len() {
            if !current() {
                success = false;
                stop.store(true, Ordering::Relaxed);
            }
            match receiver.recv_timeout(Duration::from_millis(1)) {
                Ok(result) => {
                    completed += 1;
                    success &= result;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        for handle in handles {
            success &= handle.join().unwrap_or(false);
        }
        success
    });
    (success && current()).then_some(rgb)
}

#[cfg(test)]
mod tests;
