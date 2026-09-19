//! Clipboard decoding and immutable edit input, owned independently of any tab.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use crate::{DecodedImage, DecodedImageFrame, RetainedSource};

/// Pixels and their lossless original. The backing file is private storage, not a
/// saved document path. Clones keep it alive across workers and window transfers.
#[derive(Clone, Debug)]
pub struct PastedImage {
    image: Arc<DecodedImage>,
    original: RetainedSource,
}

impl PastedImage {
    pub fn image(&self) -> &Arc<DecodedImage> {
        &self.image
    }

    pub fn original(&self) -> &RetainedSource {
        &self.original
    }
}

/// Reads once on an owned worker; never writes to the system clipboard.
/// A caller must ignore a delivered result after cancelling its request owner.
pub struct ImagePasteJob {
    cancelled: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ImagePasteJob {
    pub fn start(
        notify: impl FnOnce(Result<PastedImage, String>) + Send + 'static,
    ) -> std::io::Result<Self> {
        Self::start_with_reader(
            || {
                arboard::Clipboard::new()
                    .and_then(|mut clipboard| clipboard.get_image())
                    .map_err(|error| error.to_string())
            },
            notify,
        )
    }

    /// Prepares owned straight RGBA8 pixels without reading or writing the clipboard.
    pub fn from_rgba(
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        notify: impl FnOnce(Result<PastedImage, String>) + Send + 'static,
    ) -> std::io::Result<Self> {
        Self::start_with_reader(
            move || {
                Ok(arboard::ImageData {
                    width: width as usize,
                    height: height as usize,
                    bytes: std::borrow::Cow::Owned(rgba),
                })
            },
            notify,
        )
    }

    fn start_with_reader(
        read: impl FnOnce() -> Result<arboard::ImageData<'static>, String> + Send + 'static,
        notify: impl FnOnce(Result<PastedImage, String>) + Send + 'static,
    ) -> std::io::Result<Self> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = Arc::clone(&cancelled);
        let thread = thread::Builder::new()
            .name("towavue-image-paste".into())
            .spawn(move || {
                let result = check_cancelled(&cancel)
                    .and_then(|()| read())
                    .and_then(|raw| prepare(raw, &cancel));
                notify(result);
            })?;
        Ok(Self {
            cancelled,
            thread: Some(thread),
        })
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl Drop for ImagePasteJob {
    fn drop(&mut self) {
        self.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Image paste cancelled".into())
    } else {
        Ok(())
    }
}

fn prepare(raw: arboard::ImageData<'_>, cancelled: &AtomicBool) -> Result<PastedImage, String> {
    check_cancelled(cancelled)?;
    let (width, height) = (u32::try_from(raw.width), u32::try_from(raw.height));
    let (Ok(width), Ok(height)) = (width, height) else {
        return Err("Invalid clipboard image dimensions".into());
    };
    let length = raw
        .width
        .checked_mul(raw.height)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|bytes| *bytes <= 512 * 1024 * 1024);
    if width == 0 || height == 0 || length != Some(raw.bytes.len()) {
        return Err("Invalid clipboard image or image exceeds its memory limit".into());
    }
    // arboard owns straight RGBA8. Move its allocation instead of making another
    // full-sized copy; the private PNG preserves even hidden transparent RGB.
    let frame = DecodedImageFrame {
        width,
        height,
        rgba: raw.bytes.into_owned(),
        delay: std::time::Duration::ZERO,
    };
    let original = RetainedSource::from_pasted_frame(&frame, &|| check_cancelled(cancelled))?;
    check_cancelled(cancelled)?;
    Ok(PastedImage {
        image: Arc::new(DecodedImage {
            animation_plays: 0,
            format: "PNG",
            frames: vec![frame],
        }),
        original,
    })
}

#[cfg(test)]
mod tests;
