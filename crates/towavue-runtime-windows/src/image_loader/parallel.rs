// Ordered one-image lookahead. The coordinator publishes in
// plan order and owns/join-cancels this helper; no UI thread waits for a decoder.
use super::*;

pub(super) type Decode = dyn Fn(&Path, usize, &dyn Fn() -> bool) -> Result<Option<DecodedImage>, ImageDecodeError>
    + Send
    + Sync;

pub(super) struct Context {
    pub shared: Arc<(Mutex<Mailbox>, Condvar)>,
    pub id: Arc<()>,
    pub cancellation: crate::Cancellation,
    pub work_cancellation: crate::Cancellation,
}

pub(super) struct Ahead {
    pub path: PathBuf,
    pub bytes: usize,
    stamp: ImageStamp,
    context: Context,
    cancel: crate::Cancellation,
    thread: Option<thread::JoinHandle<Result<Option<DecodedImage>, ImageDecodeError>>>,
}

pub(super) fn pair_budget(first: &Path, second: &Path, remaining: usize) -> Option<usize> {
    let bytes = |path: &Path| image_bytes(std::io::BufReader::new(std::fs::File::open(path).ok()?));
    let first = bytes(first)?;
    let second = bytes(second)?;
    (first > 0 && second > 0 && first.checked_add(second)? <= remaining).then_some(second)
}

fn image_bytes(input: impl std::io::BufRead + std::io::Seek) -> Option<usize> {
    let reader = ::image::ImageReader::new(input)
        .with_guessed_format()
        .ok()?;
    let (width, height) = match reader.format()? {
        ::image::ImageFormat::Jpeg => {
            // image's JPEG adapter copies the entire compressed file even for dimensions.
            // Use the same backend/options on the buffered stream and stop at the first scan.
            let options = zune_core::options::DecoderOptions::default()
                .set_strict_mode(false)
                .set_max_width(usize::MAX)
                .set_max_height(usize::MAX);
            let mut decoder =
                zune_jpeg::JpegDecoder::new_with_options(reader.into_inner(), options);
            decoder.decode_headers().ok()?;
            decoder.dimensions()?
        }
        ::image::ImageFormat::Png | ::image::ImageFormat::WebP => {
            let (width, height) = reader.into_dimensions().ok()?;
            (width as usize, height as usize)
        }
        _ => return None,
    };
    width.checked_mul(height)?.checked_mul(4)
}

impl Ahead {
    pub fn start(
        path: PathBuf,
        bytes: usize,
        context: Context,
        decode: Arc<Decode>,
    ) -> Option<Self> {
        let stamp = ImageStamp::read(&path)?;
        let shared = Arc::clone(&context.shared);
        let id = Arc::clone(&context.id);
        {
            let mut mailbox = shared.0.lock().expect("image mailbox");
            let work = mailbox
                .prefetch
                .as_mut()
                .filter(|work| Arc::ptr_eq(&work.id, &id))?;
            work.lookahead = Some(path.clone());
        }
        let cancel = crate::Cancellation::default();
        let thread_cancel = cancel.clone();
        let cancellation = context.cancellation.clone();
        let work_cancellation = context.work_cancellation.clone();
        let thread_path = path.clone();
        let thread = thread::Builder::new()
            .name("towavue-prefetch-pair".into())
            .spawn(move || {
                let current = || {
                    let mailbox = shared.0.lock().expect("image mailbox");
                    !mailbox.closed
                        && !thread_cancel.is_cancelled()
                        && !cancellation.is_cancelled()
                        && !work_cancellation.is_cancelled()
                        && mailbox.prefetch.as_ref().is_some_and(|work| {
                            Arc::ptr_eq(&work.id, &id)
                                && work.generation == mailbox.generation
                                && work.lookahead.as_ref() == Some(&thread_path)
                        })
                };
                #[cfg(any(test, feature = "render-verification"))]
                let started = {
                    let mut mailbox = shared.0.lock().expect("image mailbox");
                    mailbox.metrics.prefetch.calls += 1;
                    mailbox.trace(&thread_path, TraceKind::PrefetchDecodeStarted);
                    std::time::Instant::now()
                };
                let result = decode(&thread_path, bytes, &current);
                #[cfg(any(test, feature = "render-verification"))]
                {
                    let elapsed = started.elapsed();
                    let outcome = match (current(), &result) {
                        (false, _) => verification::Outcome::Superseded,
                        (true, Ok(Some(_))) => verification::Outcome::Completed,
                        (true, Ok(None)) => verification::Outcome::Unsupported,
                        (true, Err(ImageDecodeError::TooLarge)) => {
                            verification::Outcome::BudgetRejected
                        }
                        (true, Err(_)) => verification::Outcome::Failed,
                    };
                    {
                        let mut mailbox = shared.0.lock().expect("image mailbox");
                        mailbox.metrics.prefetch.record(elapsed, outcome);
                        mailbox.trace(
                            &thread_path,
                            TraceKind::PrefetchReturned { elapsed, outcome },
                        );
                    }
                }
                if !current() || ImageStamp::read(&thread_path) != Some(stamp) {
                    return Err(ImageDecodeError::Cancelled);
                }
                result
            });
        let mut ahead = Self {
            path,
            bytes,
            stamp,
            context,
            cancel,
            thread: None,
        };
        match thread {
            Ok(thread) => {
                ahead.thread = Some(thread);
                Some(ahead)
            }
            Err(_) => None,
        }
    }

    pub fn finish(mut self, stamp: ImageStamp) -> Result<Option<DecodedImage>, ImageDecodeError> {
        let result = self
            .thread
            .take()
            .expect("lookahead thread")
            .join()
            .expect("lookahead decoder");
        if stamp != self.stamp {
            return Err(ImageDecodeError::Cancelled);
        }
        result
    }
}

impl Drop for Ahead {
    fn drop(&mut self) {
        self.cancel.cancel();
        {
            let mut mailbox = self.context.shared.0.lock().expect("image mailbox");
            if let Some(work) = &mut mailbox.prefetch
                && Arc::ptr_eq(&work.id, &self.context.id)
                && work.lookahead.as_ref() == Some(&self.path)
            {
                work.lookahead = None;
            }
            self.context.shared.1.notify_all();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};

    struct Counted {
        input: Cursor<Vec<u8>>,
        read: usize,
        furthest: u64,
    }

    impl Read for Counted {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let count = self.input.read(buffer)?;
            self.read += count;
            self.furthest = self.furthest.max(self.input.position());
            Ok(count)
        }
    }

    impl Seek for Counted {
        fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
            self.input.seek(position)
        }
    }

    fn jpeg_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut seed = 1_u32;
        let pixels = image::RgbImage::from_fn(width, height, |_, _| {
            let mut rgb = [0; 3];
            for value in &mut rgb {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                *value = (seed >> 24) as u8;
            }
            image::Rgb(rgb)
        });
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 95)
            .encode_image(&pixels)
            .expect("generated JPEG");
        bytes
    }

    #[test]
    fn jpeg_pair_sizing_reads_headers_without_copying_compressed_pixels() {
        let bytes = jpeg_bytes(512, 512);
        assert!(bytes.len() > 256 * 1024);
        let mut input = Counted {
            input: Cursor::new(bytes),
            read: 0,
            furthest: 0,
        };
        assert_eq!(image_bytes(BufReader::new(&mut input)), Some(512 * 512 * 4));
        // Sniffing and backend seeks can refill the 8 KiB buffer; count those reads too.
        assert!(input.read <= 32 * 1024, "sizing read {} bytes", input.read);
        assert!(
            input.furthest <= 16 * 1024,
            "sizing reached byte {}",
            input.furthest
        );
    }

    #[test]
    fn pair_sizing_matches_adapter_headers_and_rejects_unsupported_inputs() {
        let jpeg = jpeg_bytes(47, 31);
        let mut with_comment = jpeg[..2].to_vec();
        with_comment.extend_from_slice(&[0xff, 0xfe, 0xea, 0x62]); // 60,002-byte COM segment.
        with_comment.extend(std::iter::repeat_n(b'x', 60_000));
        with_comment.extend_from_slice(&jpeg[2..]);
        for bytes in [&jpeg, &with_comment] {
            let expected = image::ImageReader::new(Cursor::new(bytes))
                .with_guessed_format()
                .expect("format")
                .into_dimensions()
                .expect("dimensions");
            assert_eq!(
                image_bytes(BufReader::new(Cursor::new(bytes))),
                Some(expected.0 as usize * expected.1 as usize * 4)
            );
        }
        for format in [
            image::ImageFormat::Png,
            image::ImageFormat::WebP,
            image::ImageFormat::Bmp,
        ] {
            let mut bytes = Cursor::new(Vec::new());
            image::DynamicImage::new_rgb8(31, 17)
                .write_to(&mut bytes, format)
                .expect("fixture");
            bytes.set_position(0);
            assert_eq!(
                image_bytes(bytes),
                (format != image::ImageFormat::Bmp).then_some(31 * 17 * 4)
            );
        }
        for bytes in [Vec::new(), vec![0xff, 0xd8], jpeg[..20].to_vec()] {
            assert_eq!(image_bytes(BufReader::new(Cursor::new(bytes))), None);
        }
    }

    #[test]
    #[ignore = "generated large JPEG pair-sizing comparison; run in Release without concurrent builds"]
    #[allow(clippy::assertions_on_constants)]
    fn jpeg_pair_sizing_reports_full_read_comparison() {
        assert!(!cfg!(debug_assertions), "use Release for timing");
        let root = std::env::temp_dir().join(format!("towavue-pair-sizing-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("owned fixture directory");
        let bytes = jpeg_bytes(4096, 2304);
        let paths = [root.join("first.jpg"), root.join("second.jpg")];
        for path in &paths {
            std::fs::write(path, &bytes).expect("generated JPEG");
        }
        let compressed_bytes = bytes.len();
        drop(bytes);
        let stamps = paths.each_ref().map(|path| ImageStamp::read(path));
        let canvas = 4096 * 2304 * 4;
        let probe = |headers_only| {
            if headers_only {
                pair_budget(&paths[0], &paths[1], 2 * canvas)
            } else {
                let mut sizes = Vec::new();
                for path in &paths {
                    let (width, height) = image::ImageReader::open(path)
                        .ok()?
                        .with_guessed_format()
                        .ok()?
                        .into_dimensions()
                        .ok()?;
                    sizes.push(width as usize * height as usize * 4);
                }
                (sizes[0] + sizes[1] <= 2 * canvas).then_some(sizes[1])
            }
        };
        for headers_only in [false, true] {
            assert_eq!(probe(headers_only), Some(canvas));
        }
        for headers_only in [false, true, true, false] {
            let mut milliseconds = Vec::new();
            for _ in 0..7 {
                let start = std::time::Instant::now();
                let size = probe(headers_only);
                milliseconds.push(start.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(size, Some(canvas));
            }
            milliseconds.sort_by(f64::total_cmp);
            eprintln!(
                "JPEG_PAIR_SIZING headers_only={headers_only} compressed_bytes={compressed_bytes} median_ms={:.3}",
                milliseconds[3]
            );
        }
        assert!(
            paths
                .iter()
                .zip(stamps)
                .all(|(path, stamp)| ImageStamp::read(path) == stamp)
        );
        std::fs::remove_dir_all(root).expect("remove owned fixtures");
    }
}
