use super::*;
use crate::ClipboardImageError;
use std::io::{BufWriter, Write};

impl RetainedSource {
    /// Worker-only construction. The caller supplies validated straight RGBA8.
    /// Cancellation or any write failure drops the same fixed-file cleanup owner
    /// as source saving; the immutable lease is acquired only after writer close.
    pub(crate) fn from_pasted_frame(
        frame: &crate::DecodedImageFrame,
        current: &dyn Fn() -> Result<(), ClipboardImageError>,
    ) -> Result<Self, ClipboardImageError> {
        current()?;
        let files = Files::under(Path::new("untitled.png"), &std::env::temp_dir(), "pasted")?;
        Self::write_pasted_frame(frame, current, files)
    }

    fn write_pasted_frame(
        frame: &crate::DecodedImageFrame,
        current: &dyn Fn() -> Result<(), ClipboardImageError>,
        files: Arc<Files>,
    ) -> Result<Self, ClipboardImageError> {
        let write = || -> Result<(), ClipboardImageError> {
            let mut file = BufWriter::new(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&files.original)?,
            );
            let mut encoder = png::Encoder::new(&mut file, frame.width, frame.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_compression(png::Compression::Fast);
            let mut encoder = encoder.write_header()?;
            {
                let mut stream = encoder.stream_writer()?;
                // Bound cancellation intervals even for unusually wide images.
                for chunk in frame.rgba.chunks(65536) {
                    current()?;
                    stream.write_all(chunk)?;
                }
                stream.finish()?;
            }
            encoder.finish()?;
            file.flush()?;
            current()?;
            Ok(())
        };
        write()?;
        let source = FileOperationSource::capture(&files.original)?;
        let lease = source.verify_for_copy()?;
        current()?;
        Ok(Self(Arc::new(Original {
            _lease: lease,
            source,
            _files: files,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClipboardImageError;
    use std::cell::Cell;

    #[test]
    fn cancelled_paste_encoding_removes_partial_owned_png() {
        let frame = crate::DecodedImageFrame {
            width: 1024,
            height: 1024,
            rgba: vec![123; 1024 * 1024 * 4],
            delay: Duration::ZERO,
        };
        let files = Files::under(Path::new("untitled.png"), &std::env::temp_dir(), "pasted")
            .expect("owned files");
        let directory = files.directory.clone();
        let count = Cell::new(0);
        let result = RetainedSource::write_pasted_frame(
            &frame,
            &|| {
                count.set(count.get() + 1);
                if count.get() == 3 {
                    Err(ClipboardImageError::Message(
                        towavue_core::localization::Text::ClipboardImagePasteCancelled,
                    ))
                } else {
                    Ok(())
                }
            },
            files,
        );
        let error = result.expect_err("cancelled");
        assert_eq!(error.to_string(), "Image paste cancelled");
        assert_eq!(
            error.message(towavue_core::localization::Language::Japanese),
            "画像の貼り付けをキャンセルしました"
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while directory.exists() {
            assert!(std::time::Instant::now() < deadline, "partial PNG cleanup");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
