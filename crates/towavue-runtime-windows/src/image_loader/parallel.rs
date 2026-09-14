// Verification-only, ordered one-image lookahead. The coordinator publishes in
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
    let bytes = |path: &Path| {
        let reader = ::image::ImageReader::open(path)
            .ok()?
            .with_guessed_format()
            .ok()?;
        if !matches!(
            reader.format(),
            Some(
                ::image::ImageFormat::Jpeg | ::image::ImageFormat::Png | ::image::ImageFormat::WebP
            )
        ) {
            return None;
        }
        let (width, height) = reader.into_dimensions().ok()?;
        (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)
    };
    let first = bytes(first)?;
    let second = bytes(second)?;
    (first > 0 && second > 0 && first.checked_add(second)? <= remaining).then_some(second)
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
                            Arc::ptr_eq(&work.id, &id) && work.generation == mailbox.generation
                        })
                };
                let started = {
                    let mut mailbox = shared.0.lock().expect("image mailbox");
                    mailbox.metrics.prefetch.calls += 1;
                    mailbox.trace(&thread_path, TraceKind::PrefetchDecodeStarted);
                    std::time::Instant::now()
                };
                let result = decode(&thread_path, bytes, &current);
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
