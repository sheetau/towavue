//! One blocking worker owns disk/network work and the installer helper. UI
//! messages contain values only; dropping a stale event cannot launch Setup.
use super::{PendingHandoff, StartupUpdate, UpdatePhase, UpdateStore};
use crate::Cancellation;
use std::{
    fs::File,
    io::{self, Read},
    path::PathBuf,
    sync::mpsc,
    thread,
};
use towavue_core::release::ReleaseVersion;
use windows::{
    Win32::{
        Foundation::ERROR_FILE_NOT_FOUND,
        System::{Com::CoTaskMemFree, Registry::*},
        UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath},
    },
    core::{PCWSTR, w},
};

#[derive(Clone, Debug)]
pub enum UpdateEvent {
    Disabled,
    StartupComplete,
    InstallationInProgress,
    Ready {
        version: ReleaseVersion,
        phase: UpdatePhase,
        startup: bool,
    },
    Current,
    Deferred,
    HandoffReady(u64),
    Committed(u64),
    Cancelled(u64),
    Error {
        message: String,
        startup: bool,
        operation: Option<u64>,
    },
}

enum Request {
    Check(bool),
    Defer,
    Prepare(u64, Cancellation),
    Commit(u64),
    Cancel(u64),
    Stop,
}

pub struct UpdateService {
    sender: mpsc::Sender<Request>,
    cancel: Cancellation,
    operation: Option<(u64, Cancellation)>,
    thread: thread::JoinHandle<()>,
}

impl UpdateService {
    /// Invoke once for a primary application host, after launch forwarding.
    /// Development/evaluation copies do not create a cache or make HTTP calls.
    pub fn start(
        installed: ReleaseVersion,
        initial_path: Option<PathBuf>,
        notify: impl Fn(UpdateEvent) + Send + 'static,
    ) -> io::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let cancel = Cancellation::default();
        let worker_cancel = cancel.clone();
        let thread = thread::Builder::new()
            .name("towavue-updates".into())
            .spawn(move || {
                let context = match Installation::discover(installed) {
                    Ok(Some(context)) => context,
                    Ok(None) => {
                        notify(UpdateEvent::Disabled);
                        return;
                    }
                    Err(error) => {
                        notify(UpdateEvent::Error {
                            message: error.to_string(),
                            startup: true,
                            operation: None,
                        });
                        notify(UpdateEvent::Disabled);
                        return;
                    }
                };
                run(
                    context,
                    installed,
                    initial_path,
                    receiver,
                    worker_cancel,
                    &notify,
                );
            })?;
        Ok(Self {
            sender,
            cancel,
            operation: None,
            thread,
        })
    }

    pub fn check(&self, manual: bool) {
        let _ = self.sender.send(Request::Check(manual));
    }
    pub fn defer(&self) {
        let _ = self.sender.send(Request::Defer);
    }
    pub fn prepare(&mut self, token: u64) {
        let cancel = Cancellation::default();
        self.operation = Some((token, cancel.clone()));
        let _ = self.sender.send(Request::Prepare(token, cancel));
    }
    pub fn commit(&self, token: u64) {
        let _ = self.sender.send(Request::Commit(token));
    }
    pub fn cancel_install(&mut self, token: u64) {
        if self.operation.as_ref().is_some_and(|(id, _)| *id == token)
            && let Some((_, cancel)) = self.operation.take()
        {
            cancel.cancel();
        }
        let _ = self.sender.send(Request::Cancel(token));
    }
    pub fn shutdown(&mut self) {
        self.cancel.cancel();
        if let Some((_, cancel)) = self.operation.take() {
            cancel.cancel();
        }
        let _ = self.sender.send(Request::Stop);
    }
    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }
}
impl Drop for UpdateService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Installation {
    directory: PathBuf,
    store: UpdateStore,
}

fn registry_string(name: PCWSTR) -> io::Result<Option<String>> {
    let mut buffer = vec![0u16; 32768];
    let mut bytes = (buffer.len() * 2) as u32;
    // SAFETY: fixed HKCU/Registry64 identity, bounded UTF-16 output. RegGetValue
    // opens/closes its subkey internally and retains no pointer after this call.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\towavue"),
            name,
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    status.ok().map_err(io::Error::other)?;
    if bytes < 2 || !bytes.is_multiple_of(2) || bytes as usize > buffer.len() * 2 {
        return Err(io::Error::other("Invalid installation registration string"));
    }
    let text = &buffer[..bytes as usize / 2];
    if text.last() != Some(&0) || text[..text.len() - 1].contains(&0) {
        return Err(io::Error::other(
            "Ambiguous installation registration string",
        ));
    }
    String::from_utf16(&text[..text.len() - 1])
        .map(Some)
        .map_err(io::Error::other)
}

impl Installation {
    fn discover(installed: ReleaseVersion) -> io::Result<Option<Self>> {
        let Some(directory) = registry_string(w!("InstallLocation"))? else {
            return Ok(None);
        };
        let directory = PathBuf::from(directory);
        let executable = std::env::current_exe()?;
        if directory.join("towavue.exe").canonicalize().ok() != Some(executable.canonicalize()?) {
            return Ok(None);
        }
        if registry_string(w!("DisplayVersion"))?.as_deref() != Some(installed.to_string().as_str())
        {
            return Err(io::Error::other(
                "Installed and registered versions differ. Run Setup to recover the installation.",
            ));
        }
        if registry_string(w!("TowavuePendingUpdate"))?.is_some() {
            return Err(io::Error::other(
                "An interrupted installation needs recovery. Run Setup before updating.",
            ));
        }
        let identity = registry_string(w!("TowavueOwnershipId"))?
            .ok_or_else(|| io::Error::other("Installation ownership is missing"))?;
        let hash = identity
            .strip_prefix("towavue-release-")
            .filter(|hash| hash.len() == 64)
            .ok_or_else(|| io::Error::other("Not a production installation"))?;
        let mut inventory = Vec::new();
        File::open(directory.join("licenses/INSTALLED-FILES.json"))?
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut inventory)?;
        let digest = super::crypto::sha256(inventory.as_slice())?
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if inventory.len() > 4 * 1024 * 1024 || digest != hash {
            return Err(io::Error::other(
                "The installed file inventory changed. Run Setup to inspect the installation.",
            ));
        }
        // SAFETY: Shell returns a task-allocated, NUL-terminated string. Copy it
        // into owned Rust storage and release the allocation on both outcomes.
        let folder = unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None) }
            .map_err(io::Error::other)?;
        let local = unsafe { folder.to_string() }.map_err(io::Error::other);
        unsafe { CoTaskMemFree(Some(folder.0.cast())) };
        let store = UpdateStore::new(PathBuf::from(local?).join("towavue").join("updates"));
        Ok(Some(Self { directory, store }))
    }
}

fn run(
    context: Installation,
    installed: ReleaseVersion,
    initial_path: Option<PathBuf>,
    requests: mpsc::Receiver<Request>,
    cancel: Cancellation,
    notify: &impl Fn(UpdateEvent),
) {
    let mut cached = match context.store.startup(installed) {
        Ok(StartupUpdate::None) => {
            notify(UpdateEvent::StartupComplete);
            None
        }
        Ok(StartupUpdate::Installing) => {
            notify(UpdateEvent::InstallationInProgress);
            return;
        }
        Ok(StartupUpdate::Cached(cached)) => {
            notify(UpdateEvent::Ready {
                version: cached.version(),
                phase: cached.phase(),
                startup: true,
            });
            Some(cached)
        }
        Err(error) => {
            notify(UpdateEvent::Error {
                message: error.to_string(),
                startup: true,
                operation: None,
            });
            None
        }
    };
    let mut handoff: Option<(u64, PendingHandoff)> = None;
    while let Ok(request) = requests.recv() {
        if matches!(request, Request::Stop) {
            break;
        }
        // Persist an already accepted NextLaunch choice even if the user closes
        // the final window immediately. Shutdown cancels network/preparation,
        // but drains durable choices queued before Stop; it never starts Setup.
        if cancel.is_cancelled() && !matches!(request, Request::Defer) {
            continue;
        }
        let mut operation = None;
        let result: io::Result<()> = (|| {
            match request {
                Request::Stop => return Ok(()),
                Request::Check(manual) => {
                    if let Some(cached) = &cached {
                        if manual || cached.phase() == UpdatePhase::Ready {
                            notify(UpdateEvent::Ready {
                                version: cached.version(),
                                phase: cached.phase(),
                                startup: false,
                            });
                        } else {
                            notify(UpdateEvent::Current);
                        }
                    } else if handoff.is_none() {
                        if let Some(update) = super::check_for_update(installed, &cancel)? {
                            let update = context.store.download(update, &cancel)?;
                            notify(UpdateEvent::Ready {
                                version: update.version(),
                                phase: update.phase(),
                                startup: false,
                            });
                            cached = Some(update);
                        } else {
                            notify(UpdateEvent::Current);
                        }
                    }
                }
                Request::Defer => {
                    let cached = cached
                        .as_mut()
                        .ok_or_else(|| io::Error::other("Check for updates again"))?;
                    if cached.phase() == UpdatePhase::Failed {
                        context.store.transition(cached, UpdatePhase::Ready)?;
                    }
                    if cached.phase() != UpdatePhase::NextLaunch {
                        context.store.transition(cached, UpdatePhase::NextLaunch)?;
                    }
                    notify(UpdateEvent::Deferred);
                }
                Request::Prepare(token, operation_cancel) => {
                    operation = Some(token);
                    if operation_cancel.is_cancelled() {
                        notify(UpdateEvent::Cancelled(token));
                        return Ok(());
                    }
                    if handoff.is_some() {
                        return Err(io::Error::other(
                            "An installation is already being prepared",
                        ));
                    }
                    let mut selected = cached
                        .take()
                        .ok_or_else(|| io::Error::other("Check for updates again"))?;
                    if selected.phase() == UpdatePhase::Failed
                        && let Err(error) =
                            context.store.transition(&mut selected, UpdatePhase::Ready)
                    {
                        cached = Some(selected);
                        return Err(error);
                    }
                    match context.store.start_handoff(
                        selected,
                        &context.directory,
                        initial_path.as_deref(),
                        &operation_cancel,
                    ) {
                        Ok(pending) => {
                            handoff = Some((token, pending));
                            notify(UpdateEvent::HandoffReady(token));
                        }
                        Err(error) => {
                            cached = context.store.load()?;
                            return Err(error);
                        }
                    }
                }
                Request::Commit(token) => {
                    operation = Some(token);
                    if handoff.as_ref().is_some_and(|(id, _)| *id == token) {
                        handoff.take().expect("matching helper").1.commit()?;
                        notify(UpdateEvent::Committed(token));
                    }
                }
                Request::Cancel(token) => {
                    operation = Some(token);
                    if handoff.as_ref().is_some_and(|(id, _)| *id == token) {
                        drop(handoff.take());
                        cached = context.store.load()?;
                    }
                    notify(UpdateEvent::Cancelled(token));
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            // A failed commit consumes/drops its helper. Reopen the retained
            // failed generation so an explicit retry does not depend on HTTP.
            if operation.is_some()
                && handoff.is_none()
                && cached.is_none()
                && let Ok(selected) = context.store.load()
            {
                cached = selected;
            }
            notify(UpdateEvent::Error {
                message: error.to_string(),
                startup: false,
                operation,
            });
        }
    }
    // Any uncommitted helper is killed/reaped here, on this worker, never UI.
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn worker_shutdown_persists_an_accepted_next_launch_choice_without_checking_or_installing() {
        let fixture = super::super::storage::tests::Fixture::new();
        drop(fixture.stage(1));
        let context = Installation {
            directory: fixture.store.root().join("never-installed"),
            store: fixture.store.clone(),
        };
        let (send, requests) = mpsc::channel();
        let (events, receive) = mpsc::channel();
        let cancel = Cancellation::default();
        let worker_cancel = cancel.clone();
        let worker = thread::spawn(move || {
            run(
                context,
                ReleaseVersion(1, 0, 0),
                None,
                requests,
                worker_cancel,
                &|event| {
                    events.send(event).expect("test receiver");
                },
            )
        });
        assert!(matches!(
            receive
                .recv_timeout(Duration::from_secs(10))
                .expect("startup"),
            UpdateEvent::Ready { .. }
        ));
        cancel.cancel();
        send.send(Request::Check(true)).expect("old queued check");
        send.send(Request::Defer).expect("accepted choice");
        send.send(Request::Stop).expect("shutdown");
        assert!(matches!(
            receive
                .recv_timeout(Duration::from_secs(10))
                .expect("durable choice"),
            UpdateEvent::Deferred
        ));
        worker.join().expect("worker finished");
        assert_eq!(
            fixture
                .store
                .load()
                .expect("cache")
                .expect("scheduled")
                .phase(),
            UpdatePhase::NextLaunch
        );
        assert!(
            receive.try_recv().is_err(),
            "no check result or helper readiness"
        );
    }

    #[test]
    fn worker_preserves_signed_next_launch_state_and_cancelled_preparation_stays_offline() {
        let fixture = super::super::storage::tests::Fixture::new();
        drop(fixture.stage(1));
        let context = Installation {
            directory: fixture.store.root().join("never-installed"),
            store: fixture.store.clone(),
        };
        let (send, requests) = mpsc::channel();
        let (events, receive) = mpsc::channel();
        let cancel = Cancellation::default();
        let worker_cancel = cancel.clone();
        let worker = thread::spawn(move || {
            run(
                context,
                ReleaseVersion(1, 0, 0),
                None,
                requests,
                worker_cancel,
                &|event| {
                    events.send(event).expect("test receiver");
                },
            )
        });
        let next = || {
            receive
                .recv_timeout(Duration::from_secs(10))
                .expect("worker event")
        };
        assert!(matches!(
            next(),
            UpdateEvent::Ready {
                phase: UpdatePhase::Ready,
                startup: true,
                ..
            }
        ));
        send.send(Request::Defer).expect("request");
        assert!(matches!(next(), UpdateEvent::Deferred));
        assert_eq!(
            fixture
                .store
                .load()
                .expect("cache")
                .expect("selected")
                .phase(),
            UpdatePhase::NextLaunch
        );
        send.send(Request::Check(false)).expect("request");
        assert!(matches!(next(), UpdateEvent::Current));
        send.send(Request::Check(true)).expect("request");
        assert!(matches!(
            next(),
            UpdateEvent::Ready {
                phase: UpdatePhase::NextLaunch,
                startup: false,
                ..
            }
        ));
        let operation = Cancellation::default();
        operation.cancel();
        send.send(Request::Prepare(4, operation)).expect("request");
        assert!(matches!(next(), UpdateEvent::Cancelled(4)));
        assert_eq!(
            fixture
                .store
                .load()
                .expect("cache")
                .expect("selected")
                .phase(),
            UpdatePhase::NextLaunch
        );
        send.send(Request::Cancel(3)).expect("request");
        assert!(matches!(next(), UpdateEvent::Cancelled(3)));
        cancel.cancel();
        send.send(Request::Stop).expect("request");
        worker.join().expect("worker finished");
        assert!(
            matches!(fixture.store.startup(ReleaseVersion(1, 0, 0)).expect("fresh launch"), StartupUpdate::Cached(cached) if cached.phase() == UpdatePhase::NextLaunch)
        );
    }

    #[test]
    fn stale_cancellation_does_not_cancel_the_new_operation() {
        let (sender, receiver) = mpsc::channel();
        let thread = thread::spawn(move || while receiver.recv().is_ok() {});
        let operation = Cancellation::default();
        let mut service = UpdateService {
            sender,
            cancel: Cancellation::default(),
            operation: Some((2, operation.clone())),
            thread,
        };
        service.cancel_install(1);
        assert!(!operation.is_cancelled());
        service.cancel_install(2);
        assert!(operation.is_cancelled());
    }
}
