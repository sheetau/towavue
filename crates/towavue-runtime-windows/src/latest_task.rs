use std::sync::{Arc, Condvar, Mutex};
use std::thread;

type Task = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Mailbox {
    pending: Option<Task>,
    closed: bool,
}

/// Runs one task at a time, retaining only the latest unstarted task.
pub struct LatestTask {
    shared: Arc<(Mutex<Mailbox>, Condvar)>,
}

impl LatestTask {
    pub fn new(name: &str) -> std::io::Result<Self> {
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let worker_shared = Arc::clone(&shared);
        thread::Builder::new().name(name.into()).spawn(move || {
            let (mutex, ready) = &*worker_shared;
            loop {
                let task = {
                    let mut mailbox = ready
                        .wait_while(mutex.lock().expect("task mailbox"), |mailbox| {
                            !mailbox.closed && mailbox.pending.is_none()
                        })
                        .expect("task mailbox");
                    if mailbox.closed {
                        return;
                    }
                    mailbox.pending.take().expect("pending task")
                };
                task();
            }
        })?;
        Ok(Self { shared })
    }

    pub fn submit(&self, task: impl FnOnce() + Send + 'static) {
        self.shared.0.lock().expect("task mailbox").pending = Some(Box::new(task));
        self.shared.1.notify_one();
    }

    pub fn clear(&self) {
        self.shared.0.lock().expect("task mailbox").pending = None;
    }
}

impl Drop for LatestTask {
    fn drop(&mut self) {
        let mut mailbox = self.shared.0.lock().expect("task mailbox");
        mailbox.closed = true;
        mailbox.pending = None;
        self.shared.1.notify_one();
        // In-flight preview work owns no window/GPU resources; do not block window close.
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn blocked_task_keeps_only_latest_pending_task_and_clear_discards_it() {
        for clear in [false, true] {
            let worker = LatestTask::new("latest-task-test").expect("worker");
            let (started_tx, started_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            worker.submit(move || {
                started_tx.send(()).expect("started");
                release_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("released");
            });
            started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("first task");
            let (result_tx, result_rx) = mpsc::channel();
            for index in 0..1000 {
                let tx = result_tx.clone();
                worker.submit(move || {
                    tx.send(index).expect("result");
                });
            }
            if clear {
                worker.clear();
            }
            drop(result_tx);
            release_tx.send(()).expect("release");
            if !clear {
                assert_eq!(result_rx.recv_timeout(Duration::from_secs(5)), Ok(999));
            }
            assert_eq!(
                result_rx.recv_timeout(Duration::from_secs(5)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            );
        }
    }

    #[test]
    fn closing_a_busy_worker_discards_pending_work_without_joining() {
        let worker = LatestTask::new("closing-task-test").expect("worker");
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        worker.submit(move || {
            started_tx.send(()).expect("started");
            release_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("released");
            finished_tx.send(()).expect("finished");
        });
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("first task");
        let (pending_tx, pending_rx) = mpsc::channel();
        worker.submit(move || {
            pending_tx.send(()).expect("must not execute");
        });
        drop(worker);
        assert_eq!(pending_rx.try_recv(), Err(mpsc::TryRecvError::Disconnected));
        release_tx.send(()).expect("release after close");
        finished_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("in-flight task finishes");
    }
}
