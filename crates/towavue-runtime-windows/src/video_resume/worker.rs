use super::*;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

#[derive(Debug)]
pub enum VideoResumeEvent {
    Loaded {
        token: u64,
        path: PathBuf,
        result: Result<VideoResume, String>,
    },
    SaveFailed(String),
}

struct PendingWrite {
    source: VideoResumeSource,
    position: Duration,
    observed: SystemTime,
}

#[derive(Default)]
struct Mailbox {
    lookup: Option<(u64, PathBuf)>,
    writes: Vec<PendingWrite>,
    closed: bool,
}

/// One worker per window. Lookups are latest-pending; accepted writes coalesce
/// per path and drain on Drop. Callers must reject stale lookup tokens/paths.
pub struct VideoResumeHistory {
    shared: Arc<(Mutex<Mailbox>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}

impl VideoResumeHistory {
    pub fn new(
        path: PathBuf,
        notify: impl Fn(VideoResumeEvent) + Send + 'static,
    ) -> io::Result<Self> {
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let worker_shared = shared.clone();
        let worker = thread::Builder::new()
            .name("towavue-video-resume".into())
            .spawn(move || {
                let (mutex, ready) = &*worker_shared;
                loop {
                    let (writes, lookup, closed) = {
                        let mut mailbox = ready
                            .wait_while(mutex.lock().expect("resume mailbox"), |mailbox| {
                                !mailbox.closed
                                    && mailbox.lookup.is_none()
                                    && mailbox.writes.is_empty()
                            })
                            .expect("resume mailbox");
                        (
                            std::mem::take(&mut mailbox.writes),
                            mailbox.lookup.take(),
                            mailbox.closed,
                        )
                    };
                    // A same-window reopen must observe its preceding close/save.
                    for write in writes {
                        if let Err(error) = remember_video_resume(
                            &path,
                            &write.source,
                            write.position,
                            write.observed,
                        ) {
                            notify(VideoResumeEvent::SaveFailed(error.to_string()));
                        }
                    }
                    if closed {
                        return;
                    }
                    if let Some((token, media)) = lookup {
                        let result =
                            load_video_resume(&path, &media).map_err(|error| error.to_string());
                        notify(VideoResumeEvent::Loaded {
                            token,
                            path: media,
                            result,
                        });
                    }
                }
            })?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    pub fn load(&self, token: u64, path: PathBuf) {
        self.shared.0.lock().expect("resume mailbox").lookup = Some((token, path));
        self.shared.1.notify_one();
    }

    pub fn remember(&self, source: VideoResumeSource, position: Duration, observed: SystemTime) {
        let mut mailbox = self.shared.0.lock().expect("resume mailbox");
        if mailbox
            .writes
            .iter()
            .any(|write| write.source.path == source.path && write.observed > observed)
        {
            return;
        }
        mailbox
            .writes
            .retain(|write| write.source.path != source.path);
        mailbox.writes.push(PendingWrite {
            source,
            position,
            observed,
        });
        mailbox
            .writes
            .sort_by_key(|write| std::cmp::Reverse(write.observed));
        mailbox.writes.truncate(LIMIT);
        self.shared.1.notify_one();
    }
}

impl Drop for VideoResumeHistory {
    fn drop(&mut self) {
        self.shared.0.lock().expect("resume mailbox").closed = true;
        self.shared.1.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
