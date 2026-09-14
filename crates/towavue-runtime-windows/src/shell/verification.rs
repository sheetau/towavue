//! Isolated-process controls for comparing headless tests with a retained main STA.

use super::*;
use std::sync::Condvar;

static WORKERS: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

pub(super) struct Worker;

impl Worker {
    pub(super) fn new() -> Self {
        *WORKERS.0.lock().expect("worker count") += 1;
        Self
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        *WORKERS.0.lock().expect("worker count") -= 1;
        WORKERS.1.notify_all();
    }
}

/// Verification only: create before any providers in an isolated test process.
/// Optional main STA pumps messages without resolving a path or warming Shell factories.
/// Finish after dropping all applications/providers; never use this wait on a UI thread.
pub struct ShellLifetimeTrial {
    main: Option<(FolderOrderProvider, mpsc::Receiver<()>)>,
}

impl ShellLifetimeTrial {
    pub fn new(hold_main: bool) -> Self {
        assert_eq!(*WORKERS.0.lock().expect("worker count"), 0);
        let main = hold_main.then(|| {
            let shared = Arc::new((
                Mutex::new(Mailbox::default()),
                ShellWake::new().expect("main STA wake"),
            ));
            let provider = FolderOrderProvider {
                shared: Arc::clone(&shared),
            };
            let (initialized, ready) = mpsc::channel();
            let (finished, stopped) = mpsc::channel();
            thread::Builder::new()
                .name("towavue-verification-main-sta".into())
                .spawn(move || {
                    use windows::Win32::System::Com::{
                        APTTYPE_CURRENT, APTTYPE_MAINSTA, APTTYPEQUALIFIER_NONE, CoGetApartmentType,
                    };
                    let apartment = ShellApartment::new();
                    assert!(apartment.0, "initialize verification main STA");
                    let mut kind = APTTYPE_CURRENT;
                    let mut qualifier = APTTYPEQUALIFIER_NONE;
                    // SAFETY: query the initialized current thread into local outputs.
                    unsafe { CoGetApartmentType(&mut kind, &mut qualifier) }
                        .expect("query main STA");
                    assert_eq!(kind, APTTYPE_MAINSTA, "trial must own the first STA");
                    initialized.send(()).expect("main STA observer");
                    run_requests(shared, || {}, |_, _| unreachable!("anchor has no requests"));
                    drop(apartment);
                    let _ = finished.send(());
                })
                .expect("start verification main STA");
            ready
                .recv_timeout(Duration::from_secs(10))
                .expect("main STA ready");
            (provider, stopped)
        });
        Self { main }
    }

    /// Proves every instrumented provider finished its Shell work and apartment cleanup.
    /// This is not a join of every OS thread or a certification of DLL/process shutdown.
    pub fn finish(self) {
        let (workers, timeout) = WORKERS
            .1
            .wait_timeout_while(
                WORKERS.0.lock().expect("worker count"),
                Duration::from_secs(10),
                |count| *count != 0,
            )
            .expect("wait for Shell work");
        let remaining = *workers;
        drop(workers);
        assert_eq!(
            remaining,
            0,
            "Shell work remains after trial: timeout={}",
            timeout.timed_out()
        );
        let held_main = self.main.is_some();
        if let Some((provider, stopped)) = self.main {
            drop(provider);
            stopped
                .recv_timeout(Duration::from_secs(10))
                .expect("main STA cleanup");
        }
        eprintln!("SHELL_LIFETIME held_main={held_main} workers_drained=true main_released=true");
    }
}
