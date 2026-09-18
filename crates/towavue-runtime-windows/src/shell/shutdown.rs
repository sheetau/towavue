//! Retain native thread handles until Shell workers have actually exited.
//!
//! Closing a provider must not join an extension on the UI thread. Conversely,
//! returning from the process while a worker initializes URLMON can strand its
//! run-once state when ExitProcess kills that worker before DLL_PROCESS_DETACH.

use std::os::windows::io::AsRawHandle;
use std::sync::Mutex;
use std::thread::{self, JoinHandle};

use windows::Win32::Foundation::{HANDLE, WAIT_FAILED, WAIT_OBJECT_0};
use windows::Win32::System::Threading::WaitForSingleObject;

static WORKERS: Workers = Workers(Mutex::new(Vec::new()));

struct Workers(Mutex<Vec<JoinHandle<()>>>);

impl Workers {
    fn spawn(&self, name: &str, work: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
        // Register before releasing the lock: a concurrent exit check cannot
        // miss an already started worker, even if it has not been scheduled yet.
        let mut workers = self.0.lock().expect("Shell worker handles");
        workers.retain(|worker| !finished(worker));
        workers.push(thread::Builder::new().name(name.into()).spawn(work)?);
        Ok(())
    }

    fn pending(&self) -> bool {
        let mut workers = self.0.lock().expect("Shell worker handles");
        workers.retain(|worker| !finished(worker));
        !workers.is_empty()
    }
}

fn finished(worker: &JoinHandle<()>) -> bool {
    // SAFETY: the JoinHandle owns this native thread handle throughout the
    // zero-time wait. Unlike a closure-completion flag, the signaled OS handle
    // also proves that thread-local and DLL thread-detach cleanup has finished.
    let result = unsafe { WaitForSingleObject(HANDLE(worker.as_raw_handle()), 0) };
    assert_ne!(result, WAIT_FAILED, "query owned Shell thread completion");
    result == WAIT_OBJECT_0
}

pub(super) fn spawn(name: &str, work: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    WORKERS.spawn(name, work)
}

/// Nonblocking process-exit gate. Close all providers first and keep pumping the
/// main STA until this returns false. This also includes Explorer reveal jobs.
/// It neither cancels jobs nor waits for an in-flight Shell extension.
pub fn shell_workers_pending() -> bool {
    WORKERS.pending()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant};

    struct ExitGate {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    impl Drop for ExitGate {
        fn drop(&mut self) {
            self.entered.send(()).expect("observe TLS cleanup");
            self.release.recv().expect("release TLS cleanup");
        }
    }

    thread_local! {
        static EXIT_GATE: RefCell<Option<ExitGate>> = const { RefCell::new(None) };
    }

    #[test]
    fn exit_gate_tracks_native_thread_cleanup_after_the_worker_body_returns() {
        let workers = Workers(Mutex::new(Vec::new()));
        let (entered, ready) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        workers
            .spawn("towavue-shell-exit-test", move || {
                EXIT_GATE.with(|gate| {
                    *gate.borrow_mut() = Some(ExitGate {
                        entered,
                        release: blocked,
                    });
                });
            })
            .expect("start owned worker");
        ready
            .recv_timeout(Duration::from_secs(5))
            .expect("TLS cleanup");
        // Query from another thread with a deadline: accidentally joining from
        // pending() would deadlock against the unreleased TLS destructor.
        let workers = Arc::new(workers);
        let observed = Arc::clone(&workers);
        let (sent, result) = mpsc::channel();
        let query = thread::spawn(move || sent.send(observed.pending()).expect("observer"));
        let pending = result.recv_timeout(Duration::from_secs(2));
        release.send(()).expect("release owned worker");
        query.join().expect("observer thread");
        assert!(pending.expect("nonblocking query"));
        let deadline = Instant::now() + Duration::from_secs(5);
        while workers.pending() {
            assert!(Instant::now() < deadline, "native worker did not finish");
            thread::sleep(Duration::from_millis(1));
        }
        assert!(workers.0.lock().expect("retired handles").is_empty());
    }
}
