use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use thiserror::Error;
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
    FILE_NOTIFY_CHANGE_CREATION, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING, ReadDirectoryChangesW,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::{
    CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForMultipleObjects,
};
use windows::core::PCWSTR;

const DEBOUNCE: Duration = Duration::from_millis(150);

#[derive(Debug, Error)]
pub enum FolderWatchError {
    #[error("Windows folder monitoring failed: {0}")]
    Windows(#[from] windows::core::Error),
    #[error("folder monitoring thread could not start: {0}")]
    Thread(#[source] std::io::Error),
    #[error("folder monitoring stopped before its first request was ready")]
    Startup,
}

fn handle(owned: &OwnedHandle) -> HANDLE {
    HANDLE(owned.as_raw_handle())
}

fn event() -> windows::core::Result<OwnedHandle> {
    // SAFETY: a successful CreateEventW transfers one valid, uniquely owned handle.
    unsafe { CreateEventW(None, true, false, None).map(|h| OwnedHandle::from_raw_handle(h.0)) }
}

/// Stable storage written only by the kernel until completion has been observed.
struct WatchStorage {
    overlapped: UnsafeCell<OVERLAPPED>,
    // ReadDirectoryChangesW requires DWORD alignment, including after transfer.
    buffer: UnsafeCell<[u32; 4096]>,
}

struct WatchRequest {
    directory: OwnedHandle,
    io_event: OwnedHandle,
    storage: Box<WatchStorage>,
    pending: bool,
}

// SAFETY: there is one Rust owner, transferred from the caller to its worker.
// The kernel retains only addresses inside the stable boxed storage. UnsafeCell
// permits its writes; Rust never accesses that storage while a request is pending.
// Both kernel handles are owned here, and Drop drains I/O before releasing any
// storage or handle. OVERLAPPED contains only our event and zeroed union fields.
// The owner is not Sync, and no request reference is shared between threads.
unsafe impl Send for WatchRequest {}

impl WatchRequest {
    fn new(folder: &Path) -> windows::core::Result<Self> {
        let wide_path = folder
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        // SAFETY: the path is terminated and lives through the call. Successful
        // creation transfers the directory handle into one RAII owner.
        let directory = unsafe {
            OwnedHandle::from_raw_handle(
                CreateFileW(
                    PCWSTR(wide_path.as_ptr()),
                    FILE_LIST_DIRECTORY.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                    None,
                )?
                .0,
            )
        };
        let mut request = Self {
            directory,
            io_event: event()?,
            storage: Box::new(WatchStorage {
                overlapped: UnsafeCell::new(OVERLAPPED::default()),
                buffer: UnsafeCell::new([0; 4096]),
            }),
            pending: false,
        };
        request.arm()?;
        Ok(request)
    }

    fn arm(&mut self) -> windows::core::Result<()> {
        assert!(!self.pending, "folder notification already pending");
        let filter = FILE_NOTIFY_CHANGE_FILE_NAME
            | FILE_NOTIFY_CHANGE_DIR_NAME
            | FILE_NOTIFY_CHANGE_SIZE
            | FILE_NOTIFY_CHANGE_LAST_WRITE
            | FILE_NOTIFY_CHANGE_CREATION;
        // SAFETY: no request uses the storage until this successful registration.
        // Both addresses remain stable until finish or Drop drains the operation.
        // No completion routine or thread-affine APC borrows this owner.
        unsafe {
            ResetEvent(handle(&self.io_event))?;
            self.storage.overlapped.get().write(OVERLAPPED {
                hEvent: handle(&self.io_event),
                ..Default::default()
            });
            ReadDirectoryChangesW(
                handle(&self.directory),
                self.storage.buffer.get().cast(),
                size_of::<[u32; 4096]>() as u32,
                false,
                filter,
                None,
                Some(self.storage.overlapped.get()),
                None,
            )?;
        }
        self.pending = true;
        Ok(())
    }

    fn finish(&mut self) -> windows::core::Result<u32> {
        assert!(self.pending, "no pending folder notification");
        let mut transferred = 0;
        // SAFETY: this is the exact live request, with its unique live event.
        // Waiting observes completion even if cancellation races with success.
        let result = unsafe {
            GetOverlappedResult(
                handle(&self.directory),
                self.storage.overlapped.get(),
                &mut transferred,
                true,
            )
        };
        self.pending = false;
        result.map(|()| transferred)
    }
}

impl Drop for WatchRequest {
    fn drop(&mut self) {
        if self.pending {
            // SAFETY: CancelIoEx can cancel the caller's initial registration
            // from the worker. It does not wait; drain before fields are dropped,
            // even if cancellation reports that the operation already completed.
            let _ =
                unsafe { CancelIoEx(handle(&self.directory), Some(self.storage.overlapped.get())) };
            let _ = self.finish();
        }
    }
}

/// Debounced notification source for direct changes within one folder.
pub struct FolderWatcher {
    changes: Receiver<Instant>,
    stop_event: Arc<OwnedHandle>,
    worker: Option<JoinHandle<()>>,
    last_change: Option<Instant>,
    // Preserve caller-thread ownership: its initial I/O remains pending until
    // completion or shutdown. Drop joins the worker before that thread retires.
    _caller_thread: PhantomData<Rc<()>>,
}

impl FolderWatcher {
    pub fn new(folder: &Path) -> Result<Self, FolderWatchError> {
        Self::new_with_spawn(folder, |request, stop, sender| {
            thread::Builder::new()
                .name("towavue-folder-watch".to_owned())
                .spawn(move || watch_loop(request, stop, sender))
        })
    }

    fn new_with_spawn(
        folder: &Path,
        spawn: impl FnOnce(
            WatchRequest,
            Arc<OwnedHandle>,
            mpsc::Sender<Instant>,
        ) -> std::io::Result<JoinHandle<()>>,
    ) -> Result<Self, FolderWatchError> {
        let stop_event = Arc::new(event()?);
        // Queue the first asynchronous request before returning to enumeration.
        // The worker may start later; the kernel already records folder changes.
        let request = WatchRequest::new(folder)?;
        let (sender, changes) = mpsc::channel();
        // On spawn failure the closure/request is dropped, cancelling and draining
        // that exact request before its owned directory/event/storage are released.
        let worker =
            spawn(request, Arc::clone(&stop_event), sender).map_err(FolderWatchError::Thread)?;
        Ok(Self {
            changes,
            stop_event,
            worker: Some(worker),
            last_change: None,
            _caller_thread: PhantomData,
        })
    }

    /// Returns true once after a quiet debounce period follows one or more changes.
    pub fn try_changed(&mut self) -> bool {
        for changed_at in self.changes.try_iter() {
            self.last_change = Some(changed_at);
        }
        if self
            .last_change
            .is_some_and(|changed_at| changed_at.elapsed() >= DEBOUNCE)
        {
            self.last_change = None;
            true
        } else {
            false
        }
    }
}

impl Drop for FolderWatcher {
    fn drop(&mut self) {
        // SAFETY: both this owner and the worker retain the event through join.
        let _ = unsafe { SetEvent(handle(&self.stop_event)) };
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn watch_loop(mut request: WatchRequest, stop: Arc<OwnedHandle>, sender: mpsc::Sender<Instant>) {
    loop {
        // SAFETY: request and stop own both events for this entire wait.
        let wait = unsafe {
            WaitForMultipleObjects(&[handle(&stop), handle(&request.io_event)], false, INFINITE)
        };
        if wait == WAIT_OBJECT_0 {
            break;
        }
        if wait.0 != WAIT_OBJECT_0.0 + 1 {
            crate::diagnostic!("towavue: folder monitor wait failed: {}", wait.0);
            break;
        }
        let Ok(transferred) = request.finish() else {
            break;
        };
        if transferred > 0 && sender.send(Instant::now()).is_err() {
            break;
        }
        if let Err(error) = request.arm() {
            crate::diagnostic!("towavue: folder monitoring stopped: {error}");
            break;
        }
    }
    // Request Drop cancels/drains the outstanding operation on every exit path,
    // including unwinding, before releasing stable storage and native handles.
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use windows::Win32::Storage::FileSystem::FILE_SHARE_MODE;

    struct TestFolder(PathBuf);
    impl TestFolder {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "towavue-watch-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("system time follows Unix epoch")
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).expect("create watch fixture directory");
            Self(path)
        }
        fn exclusive_open(&self) -> windows::core::Result<OwnedHandle> {
            let path: Vec<u16> = self.0.as_os_str().encode_wide().chain(Some(0)).collect();
            // SAFETY: valid terminated path; successful handle has one owner.
            unsafe {
                CreateFileW(
                    PCWSTR(path.as_ptr()),
                    FILE_LIST_DIRECTORY.0,
                    FILE_SHARE_MODE(0),
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS,
                    None,
                )
                .map(|h| OwnedHandle::from_raw_handle(h.0))
            }
        }
    }
    impl Drop for TestFolder {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).expect("remove watch fixture directory");
        }
    }
    fn changed(watcher: &mut FolderWatcher) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if watcher.try_changed() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("folder notification timed out");
    }

    #[test]
    fn reports_debounced_changes_across_initial_and_rearmed_requests() {
        let root = TestFolder::new();
        let mut watcher = FolderWatcher::new(&root.0).expect("start folder monitor");
        for name in ["first.jpg", "second.jpg"] {
            std::fs::write(root.0.join(name), b"fixture").expect("write notification fixture");
            assert!(!watcher.try_changed(), "notification must debounce");
            changed(&mut watcher);
            assert!(
                !watcher.try_changed(),
                "quiet notification is consumed once"
            );
        }
        drop(watcher);
        root.exclusive_open()
            .expect("directory handle released after join");
    }

    #[test]
    fn captures_change_before_worker_enters() {
        let root = TestFolder::new();
        let (release, gate) = mpsc::channel();
        let mut watcher = FolderWatcher::new_with_spawn(&root.0, |request, stop, sender| {
            thread::Builder::new().spawn(move || {
                gate.recv_timeout(Duration::from_secs(3))
                    .expect("release worker");
                watch_loop(request, stop, sender);
            })
        })
        .expect("start monitor with delayed worker");
        std::fs::write(root.0.join("before-worker.jpg"), b"fixture")
            .expect("write fixture before worker starts");
        assert!(!watcher.try_changed());
        release.send(()).expect("release monitor worker");
        changed(&mut watcher);
        drop(watcher);
        root.exclusive_open()
            .expect("initial request and worker released");
    }

    #[test]
    fn dropping_registered_request_without_worker_drains_and_releases() {
        let root = TestFolder::new();
        let request = WatchRequest::new(&root.0).expect("register directory request");
        assert!(
            root.exclusive_open().is_err(),
            "live watch owns directory access"
        );
        drop(request);
        root.exclusive_open()
            .expect("cancelled request released directory");
    }

    #[test]
    fn spawn_failure_releases_already_registered_request() {
        let root = TestFolder::new();
        let result = FolderWatcher::new_with_spawn(&root.0, |request, _stop, _sender| {
            assert!(request.pending);
            assert!(root.exclusive_open().is_err());
            Err(std::io::Error::other("injected worker spawn failure"))
        });
        assert!(matches!(result, Err(FolderWatchError::Thread(_))));
        root.exclusive_open()
            .expect("spawn failure drained request and closed handle");
    }

    #[test]
    fn immediate_shutdown_cancels_request_on_another_thread() {
        let root = TestFolder::new();
        drop(FolderWatcher::new(&root.0).expect("start folder monitor"));
        root.exclusive_open()
            .expect("shutdown joined worker and drained caller I/O");
    }

    #[test]
    fn missing_folder_reports_native_error() {
        let root = TestFolder::new();
        assert!(matches!(
            FolderWatcher::new(&root.0.join("missing")),
            Err(FolderWatchError::Windows(_))
        ));
    }
}
