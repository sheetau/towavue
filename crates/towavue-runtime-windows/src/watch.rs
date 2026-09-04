use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use thiserror::Error;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
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

/// Debounced notification source for direct changes within one folder.
pub struct FolderWatcher {
    changes: Receiver<Instant>,
    directory: HANDLE,
    stop_event: HANDLE,
    worker: Option<JoinHandle<()>>,
    last_change: Option<Instant>,
}

impl FolderWatcher {
    pub fn new(folder: &Path) -> Result<Self, FolderWatchError> {
        let wide_path = folder
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        // The directory handle is shared with one worker and stays open until
        // that worker has observed the stop event and joined.
        let directory = unsafe {
            CreateFileW(
                PCWSTR(wide_path.as_ptr()),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                None,
            )?
        };
        let stop_event = match unsafe { CreateEventW(None, true, false, None) } {
            Ok(event) => event,
            Err(error) => {
                // SAFETY: creation succeeded above and ownership has not escaped.
                let _ = unsafe { CloseHandle(directory) };
                return Err(error.into());
            }
        };
        let (sender, changes) = mpsc::channel();
        let (ready_sender, ready) = mpsc::channel();
        // HANDLE is process-global but intentionally not Send in the bindings.
        // The numeric values are copied to one worker while this owner keeps the
        // kernel objects alive until that worker joins.
        let directory_value = directory.0 as usize;
        let stop_event_value = stop_event.0 as usize;
        let worker = match thread::Builder::new()
            .name("towavue-folder-watch".to_owned())
            .spawn(move || {
                watch_loop(
                    HANDLE(directory_value as *mut _),
                    HANDLE(stop_event_value as *mut _),
                    sender,
                    ready_sender,
                )
            }) {
            Ok(worker) => worker,
            Err(error) => {
                // SAFETY: neither handle has escaped to a worker when spawning fails.
                unsafe {
                    let _ = CloseHandle(stop_event);
                    let _ = CloseHandle(directory);
                }
                return Err(FolderWatchError::Thread(error));
            }
        };
        if ready.recv_timeout(Duration::from_secs(2)).is_err() {
            // SAFETY: both handles remain valid until the worker observes the
            // stop event and has returned.
            let _ = unsafe { SetEvent(stop_event) };
            let _ = worker.join();
            unsafe {
                let _ = CloseHandle(stop_event);
                let _ = CloseHandle(directory);
            }
            return Err(FolderWatchError::Startup);
        }
        Ok(Self {
            changes,
            directory,
            stop_event,
            worker: Some(worker),
            last_change: None,
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
        // SAFETY: the stop event remains valid until the worker exits. The
        // worker cancels and drains its own OVERLAPPED request before returning.
        let _ = unsafe { SetEvent(self.stop_event) };
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        // SAFETY: the worker has stopped using both uniquely owned handles.
        unsafe {
            let _ = CloseHandle(self.stop_event);
            let _ = CloseHandle(self.directory);
        }
    }
}

fn watch_loop(
    directory: HANDLE,
    stop_event: HANDLE,
    sender: mpsc::Sender<Instant>,
    ready: mpsc::Sender<()>,
) {
    let io_event = match unsafe { CreateEventW(None, true, false, None) } {
        Ok(event) => event,
        Err(error) => {
            eprintln!("towavue: folder monitor event creation failed: {error}");
            return;
        }
    };
    let mut buffer = [0_u8; 16 * 1024];
    let mut ready = Some(ready);
    loop {
        let _ = unsafe { ResetEvent(io_event) };
        let mut overlapped = OVERLAPPED {
            hEvent: io_event,
            ..Default::default()
        };
        let filter = FILE_NOTIFY_CHANGE_FILE_NAME
            | FILE_NOTIFY_CHANGE_DIR_NAME
            | FILE_NOTIFY_CHANGE_SIZE
            | FILE_NOTIFY_CHANGE_LAST_WRITE
            | FILE_NOTIFY_CHANGE_CREATION;
        // The buffer and OVERLAPPED storage remain alive until the operation
        // completes or cancellation has been synchronously drained below.
        if let Err(error) = unsafe {
            ReadDirectoryChangesW(
                directory,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                false,
                filter,
                None,
                Some(&mut overlapped),
                None,
            )
        } {
            eprintln!("towavue: folder monitoring stopped: {error}");
            break;
        }
        if let Some(ready) = ready.take() {
            let _ = ready.send(());
        }

        let wait = unsafe { WaitForMultipleObjects(&[stop_event, io_event], false, INFINITE) };
        if wait == WAIT_OBJECT_0 {
            // Cancel this exact operation, then wait for it to release its
            // borrowed buffer and OVERLAPPED storage before returning.
            let _ = unsafe { CancelIoEx(directory, Some(&overlapped)) };
            let mut transferred = 0;
            let _ = unsafe { GetOverlappedResult(directory, &overlapped, &mut transferred, true) };
            break;
        }
        if wait.0 != WAIT_OBJECT_0.0 + 1 {
            eprintln!("towavue: folder monitor wait failed: {}", wait.0);
            let _ = unsafe { CancelIoEx(directory, Some(&overlapped)) };
            let mut transferred = 0;
            let _ = unsafe { GetOverlappedResult(directory, &overlapped, &mut transferred, true) };
            break;
        }

        let mut transferred = 0;
        if unsafe { GetOverlappedResult(directory, &overlapped, &mut transferred, false) }.is_err()
        {
            break;
        }
        if transferred > 0 && sender.send(Instant::now()).is_err() {
            break;
        }
    }
    // SAFETY: this worker exclusively owns the I/O completion event.
    let _ = unsafe { CloseHandle(io_event) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_a_debounced_directory_change() {
        let root = std::env::temp_dir().join(format!(
            "towavue-watch-{}-{}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        std::fs::create_dir(&root).expect("create test directory");
        let mut watcher = FolderWatcher::new(&root).expect("watch test directory");
        std::fs::write(root.join("new.jpg"), b"fixture").expect("write test file");

        let deadline = Instant::now() + Duration::from_secs(3);
        while !watcher.try_changed() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        assert!(Instant::now() < deadline, "folder notification timed out");

        drop(watcher);
        std::fs::remove_dir_all(root).expect("remove test directory");
    }
}
