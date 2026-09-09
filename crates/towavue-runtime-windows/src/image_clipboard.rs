use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use towavue_core::UnitPoint;

use crate::DecodedImage;

pub struct ImageCopyRequest {
    pub image: Arc<DecodedImage>,
    pub frame_index: usize,
    pub source_uv: [UnitPoint; 4],
    pub size: (u32, u32),
}

pub struct ImageCopyJob {
    cancelled: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ImageCopyJob {
    pub fn start(
        request: ImageCopyRequest,
        notify: impl FnOnce(Result<(u32, u32), String>) + Send + 'static,
    ) -> std::io::Result<Self> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = Arc::clone(&cancelled);
        let thread = thread::Builder::new()
            .name("towavue-image-copy".into())
            .spawn(move || {
                let result = pixels(&request, &cancel).and_then(|rgba| {
                    if cancel.load(Ordering::Relaxed) {
                        return Err("Image copy cancelled".into());
                    }
                    arboard::Clipboard::new()
                        .and_then(|mut clipboard| {
                            clipboard.set_image(arboard::ImageData {
                                width: request.size.0 as usize,
                                height: request.size.1 as usize,
                                bytes: Cow::Owned(rgba),
                            })
                        })
                        .map_err(|error| error.to_string())?;
                    Ok(request.size)
                });
                notify(result);
            })?;
        Ok(Self {
            cancelled,
            thread: Some(thread),
        })
    }
}

impl Drop for ImageCopyJob {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn pixels(request: &ImageCopyRequest, cancelled: &AtomicBool) -> Result<Vec<u8>, String> {
    let frame = request
        .image
        .frames
        .get(request.frame_index)
        .ok_or("Image frame is unavailable")?;
    let (width, height) = request.size;
    let bytes = (u64::from(width) * u64::from(height))
        .checked_mul(4)
        .filter(|bytes| *bytes <= 512 * 1024 * 1024)
        .ok_or("Image copy exceeds its memory limit")?;
    if width == 0
        || height == 0
        || frame.width == 0
        || frame.height == 0
        || (u64::from(frame.width) * u64::from(frame.height)).checked_mul(4)
            != Some(frame.rgba.len() as u64)
        || request.source_uv.iter().any(|p| {
            !p.x.is_finite()
                || !p.y.is_finite()
                || !(0.0..=1.0).contains(&p.x)
                || !(0.0..=1.0).contains(&p.y)
        })
    {
        return Err("Invalid or oversized image copy region".into());
    }
    if cancelled.load(Ordering::Relaxed) {
        return Err("Image copy cancelled".into());
    }
    let mut output = Vec::with_capacity(bytes as usize);
    let [top_left, top_right, _, bottom_left] = request.source_uv;
    // Orthogonal image edits map pixel centers exactly; no display scaling or alpha conversion.
    for y in 0..height {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Image copy cancelled".into());
        }
        let v = (f64::from(y) + 0.5) / f64::from(height);
        for x in 0..width {
            let u = (f64::from(x) + 0.5) / f64::from(width);
            let sx = f64::from(top_left.x)
                + u * f64::from(top_right.x - top_left.x)
                + v * f64::from(bottom_left.x - top_left.x);
            let sy = f64::from(top_left.y)
                + u * f64::from(top_right.y - top_left.y)
                + v * f64::from(bottom_left.y - top_left.y);
            let sx = ((sx * f64::from(frame.width)) as u32).min(frame.width - 1);
            let sy = ((sy * f64::from(frame.height)) as u32).min(frame.height - 1);
            let offset = (sy as usize * frame.width as usize + sx as usize) * 4;
            output.extend_from_slice(&frame.rgba[offset..offset + 4]);
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires an interactive Windows clipboard and explicitly replaces its image contents"]
    fn image_clipboard_round_trip_preserves_transparent_rgba_after_worker_exit() {
        let rgba = vec![
            255, 123, 12, 0, 200, 99, 3, 1, 17, 55, 33, 127, 1, 2, 3, 255,
        ];
        let request = ImageCopyRequest {
            image: Arc::new(DecodedImage {
                format: "test",
                frames: vec![crate::DecodedImageFrame {
                    width: 2,
                    height: 2,
                    rgba: rgba.clone(),
                    delay: std::time::Duration::ZERO,
                }],
            }),
            frame_index: 0,
            size: (2, 2),
            source_uv: [
                UnitPoint { x: 0.0, y: 0.0 },
                UnitPoint { x: 1.0, y: 0.0 },
                UnitPoint { x: 1.0, y: 1.0 },
                UnitPoint { x: 0.0, y: 1.0 },
            ],
        };
        let (send, receive) = std::sync::mpsc::channel();
        let job = ImageCopyJob::start(request, move |result| {
            send.send(result).expect("result receiver");
        })
        .expect("worker");
        assert_eq!(
            receive
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("copy notification")
                .expect("clipboard write"),
            (2, 2)
        );
        drop(job);
        let result = arboard::Clipboard::new()
            .expect("clipboard")
            .get_image()
            .expect("our copied PNG");
        assert_eq!((result.width, result.height), (2, 2));
        assert_eq!(result.bytes.as_ref(), rgba);
    }

    #[test]
    fn copy_pixels_keep_straight_alpha_and_snapshot_frame() {
        let frame = crate::DecodedImageFrame {
            width: 2,
            height: 2,
            delay: std::time::Duration::ZERO,
            rgba: vec![
                255, 123, 12, 0, 200, 99, 3, 1, 17, 55, 33, 127, 1, 2, 3, 255,
            ],
        };
        let mut request = ImageCopyRequest {
            image: Arc::new(DecodedImage {
                format: "test",
                frames: vec![
                    frame.clone(),
                    crate::DecodedImageFrame {
                        rgba: vec![9; 16],
                        ..frame.clone()
                    },
                ],
            }),
            frame_index: 0,
            size: (2, 2),
            source_uv: [
                UnitPoint { x: 0.0, y: 0.0 },
                UnitPoint { x: 1.0, y: 0.0 },
                UnitPoint { x: 1.0, y: 1.0 },
                UnitPoint { x: 0.0, y: 1.0 },
            ],
        };
        let cancel = AtomicBool::new(false);
        assert_eq!(pixels(&request, &cancel).expect("copy"), frame.rgba);
        request.source_uv.rotate_right(1);
        let rotated = pixels(&request, &cancel).expect("rotated");
        assert_eq!(
            rotated,
            [
                frame.rgba[8..12].to_vec(),
                frame.rgba[0..4].to_vec(),
                frame.rgba[12..16].to_vec(),
                frame.rgba[4..8].to_vec()
            ]
            .concat()
        );
        request.frame_index = 1;
        assert_eq!(pixels(&request, &cancel).expect("snapshot"), vec![9; 16]);
        cancel.store(true, Ordering::Relaxed);
        assert!(pixels(&request, &cancel).is_err());
        cancel.store(false, Ordering::Relaxed);
        request.size = (u32::MAX, u32::MAX);
        assert!(pixels(&request, &cancel).is_err());
    }
}
