use super::*;
use std::io::{BufWriter, Write};

impl RetainedSource {
    /// Worker-only construction. The caller supplies validated straight RGBA8.
    /// Cancellation or any write failure drops the same fixed-file cleanup owner
    /// as source saving; the immutable lease is acquired only after writer close.
    pub(crate) fn from_pasted_frame(
        frame: &crate::DecodedImageFrame,
        current: &dyn Fn() -> Result<(), String>,
    ) -> Result<Self, String> {
        current()?;
        let files = Files::under(Path::new("untitled.png"), &std::env::temp_dir(), "pasted")
            .map_err(|error| error.to_string())?;
        Self::write_pasted_frame(frame, current, files)
    }

    fn write_pasted_frame(
        frame: &crate::DecodedImageFrame,
        current: &dyn Fn() -> Result<(), String>,
        files: Arc<Files>,
    ) -> Result<Self, String> {
        let write = || -> Result<(), Box<dyn std::error::Error>> {
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
        write().map_err(|error| error.to_string())?;
        let source =
            FileOperationSource::capture(&files.original).map_err(|error| error.to_string())?;
        let lease = source
            .verify_for_copy()
            .map_err(|error| error.to_string())?;
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
                    Err("test cancellation".into())
                } else {
                    Ok(())
                }
            },
            files,
        );
        assert_eq!(result.expect_err("cancelled"), "test cancellation");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while directory.exists() {
            assert!(std::time::Instant::now() < deadline, "partial PNG cleanup");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
