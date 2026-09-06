use std::cmp::Ordering;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use thiserror::Error;
use towavue_core::{
    FolderMediaItem, FolderSnapshot, FolderSnapshotSource, MediaKind, PropertyKey, ShellIdentity,
    SortColumn, SortDirection,
};
use windows::Win32::Foundation::{HANDLE, HWND, PROPERTYKEY, RECT, WAIT_FAILED};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree, IServiceProvider};
use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    EBO_NOBORDER, EBO_NOPERSISTVIEWSTATE, EBO_NOTRAVELLOG, ExplorerBrowser, IExplorerBrowser,
    IFolderView2, IPersistFolder2, IShellBrowser, IShellItem, IShellItemArray, IShellWindows,
    IWebBrowserApp, SBSP_ABSOLUTE, SHCreateItemFromIDList, SHGetIDListFromObject,
    SHParseDisplayName, SID_STopLevelBrowser, SIGDN_FILESYSPATH, SORT_ASCENDING, SORT_DESCENDING,
    SORTCOLUMN, SVGIO_ALLVIEW, SVGIO_FLAG_VIEWORDER, ShellWindows, StrCmpLogicalW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, MWMO_INPUTAVAILABLE,
    MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, QS_ALLINPUT, TranslateMessage,
    WINDOW_EX_STYLE, WINDOW_STYLE,
};
use windows::core::{Interface, PCWSTR, w};

#[derive(Debug, Error)]
pub enum FolderOrderError {
    #[error("the Shell worker stopped")]
    WorkerStopped,
    #[error("the Shell worker did not return a snapshot")]
    ResponseLost,
}

struct Request {
    folder: PathBuf,
    generation: u64,
    reply: Option<mpsc::Sender<FolderSnapshot>>,
}

#[derive(Default)]
struct Mailbox {
    generation: u64,
    pending: Option<Request>,
    completed: Option<FolderSnapshot>,
    closed: bool,
}

struct ShellWake(OwnedHandle);

impl ShellWake {
    fn new() -> Result<Self, FolderOrderError> {
        // SAFETY: unnamed auto-reset event, with no borrowed security/name data. Transfer
        // its sole ownership to OwnedHandle; shared worker state keeps it live through waits.
        let handle = unsafe { CreateEventW(None, false, false, None) }
            .map_err(|_| FolderOrderError::WorkerStopped)?;
        Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle.0) }))
    }

    fn signal(&self) {
        // SAFETY: the borrowed event remains live; signaling is thread-safe and carries no data.
        unsafe { SetEvent(HANDLE(self.0.as_raw_handle())).expect("signal Shell worker") };
    }

    fn wait(&self) {
        // SAFETY: this worker retains the event for the entire wait. No mailbox lock is held.
        // Wake for queued/sent Windows messages as well as requests; dispatch on the owning STA.
        let result = unsafe {
            MsgWaitForMultipleObjectsEx(
                Some(&[HANDLE(self.0.as_raw_handle())]),
                INFINITE,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            )
        };
        assert_ne!(result, WAIT_FAILED, "wait for Shell requests or messages");
    }
}

/// The STA owns all Shell objects; requests and results carry only owned domain values.
pub struct FolderOrderProvider {
    shared: Arc<(Mutex<Mailbox>, ShellWake)>,
}

impl FolderOrderProvider {
    pub fn new() -> Result<Self, FolderOrderError> {
        Self::with_notify(|| {})
    }

    pub fn with_notify(notify: impl Fn() + Send + 'static) -> Result<Self, FolderOrderError> {
        let shared = Arc::new((Mutex::new(Mailbox::default()), ShellWake::new()?));
        let worker_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("towavue-shell-sta".into())
            .spawn(move || shell_worker(worker_shared, notify))
            .map_err(|_| FolderOrderError::WorkerStopped)?;
        Ok(Self { shared })
    }

    /// Blocking interface for callers outside the UI event loop.
    pub fn snapshot(&mut self, folder: &Path) -> Result<FolderSnapshot, FolderOrderError> {
        let (reply, response) = mpsc::channel();
        self.enqueue(Some(folder.to_owned()), Some(reply));
        response.recv().map_err(|_| FolderOrderError::ResponseLost)
    }

    /// Replaces queued work. None invalidates pending and in-flight results.
    pub fn request(&self, folder: Option<PathBuf>) -> u64 {
        self.enqueue(folder, None)
    }

    fn enqueue(&self, folder: Option<PathBuf>, reply: Option<mpsc::Sender<FolderSnapshot>>) -> u64 {
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("Shell mailbox");
        mailbox.generation = mailbox.generation.wrapping_add(1);
        mailbox.pending = folder.map(|folder| Request {
            folder,
            generation: mailbox.generation,
            reply,
        });
        mailbox.completed = None;
        ready.signal();
        mailbox.generation
    }

    pub fn take_completed(&self) -> Option<FolderSnapshot> {
        self.shared
            .0
            .lock()
            .expect("Shell mailbox")
            .completed
            .take()
    }
}

impl Drop for FolderOrderProvider {
    fn drop(&mut self) {
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("Shell mailbox");
        mailbox.closed = true;
        mailbox.pending = None;
        mailbox.completed = None;
        ready.signal();
        // A Shell extension can block inside COM. The STA finishes and releases its own objects.
    }
}

fn run_requests(
    shared: Arc<(Mutex<Mailbox>, ShellWake)>,
    notify: impl Fn(),
    mut resolve: impl FnMut(&Path, u64) -> FolderSnapshot,
) {
    let (mutex, ready) = &*shared;
    loop {
        // SAFETY: pump only this worker's queue, outside the mailbox lock. Shell/COM may
        // retain apartment-affine helper windows after a folder snapshot has completed.
        unsafe { pump_messages() };
        let request = {
            let mut mailbox = mutex.lock().expect("Shell mailbox");
            if mailbox.closed {
                return;
            }
            mailbox.pending.take()
        };
        let Some(request) = request else {
            ready.wait();
            continue;
        };
        let snapshot = resolve(&request.folder, request.generation);
        let publish = {
            let mut mailbox = mutex.lock().expect("Shell mailbox");
            if mailbox.closed {
                return;
            }
            if mailbox.generation != request.generation {
                false
            } else if let Some(reply) = request.reply {
                let _ = reply.send(snapshot);
                false
            } else {
                mailbox.completed = Some(snapshot);
                true
            }
        };
        if publish {
            notify();
        }
    }
}

fn shell_worker(shared: Arc<(Mutex<Mailbox>, ShellWake)>, notify: impl Fn()) {
    // SAFETY: this worker owns every COM interface it creates and never moves one across threads.
    // It initializes and uninitializes the apartment on this same thread.
    let initialized = unsafe { OleInitialize(None).is_ok() };
    let mut last_live_window = None;
    run_requests(shared, notify, |folder, generation| {
        if initialized {
            shell_snapshot(folder, generation, &mut last_live_window).unwrap_or_else(|| {
                eprintln!(
                    "towavue: Shell view unavailable for {}; using natural-name order",
                    folder.display()
                );
                fallback_snapshot(folder, generation)
            })
        } else {
            eprintln!(
                "towavue: Shell STA initialization failed for {}; using natural-name order",
                folder.display()
            );
            fallback_snapshot(folder, generation)
        }
    });
    if initialized {
        // SAFETY: balances this thread's successful OleInitialize call after COM objects drop.
        unsafe { OleUninitialize() };
    }
}

fn shell_snapshot(
    folder: &Path,
    generation: u64,
    last_live_window: &mut Option<HWND>,
) -> Option<FolderSnapshot> {
    let folder = canonical_shell_path(folder).ok()?;
    let folder_pidl = parse_path(&folder)?;

    // SAFETY: all COM and PIDL values remain on the initialized STA worker.
    unsafe {
        if let Some((view, window)) = matching_live_view(folder_pidl.as_ptr(), *last_live_window) {
            *last_live_window = Some(window);
            if let Some(snapshot) = capture_view(
                &view,
                &folder,
                &folder_pidl,
                FolderSnapshotSource::LiveExplorerView,
                generation,
            ) {
                return Some(snapshot);
            }
        }

        let browser = HiddenExplorerBrowser::new()?;
        if let Err(error) = browser
            .browser
            .BrowseToIDList(folder_pidl.as_ptr(), SBSP_ABSOLUTE)
        {
            eprintln!("towavue: hidden Shell navigation failed: {error}");
            return None;
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            pump_messages();
            if let Ok(view) = browser.browser.GetCurrentView::<IFolderView2>()
                && view_matches(&view, folder_pidl.as_ptr())
                && let Some(snapshot) = capture_view(
                    &view,
                    &folder,
                    &folder_pidl,
                    FolderSnapshotSource::PersistedShellView,
                    generation,
                )
            {
                return Some(snapshot);
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

unsafe fn matching_live_view(
    folder_pidl: *const ITEMIDLIST,
    last_live_window: Option<HWND>,
) -> Option<(IFolderView2, HWND)> {
    unsafe {
        let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL).ok()?;
        let foreground = windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow();
        let mut recent = None;
        let mut first = None;
        for index in 0..windows.Count().ok()? {
            let Ok(dispatch) = windows.Item(&VARIANT::from(index)) else {
                continue;
            };
            let Ok(web) = dispatch.cast::<IWebBrowserApp>() else {
                continue;
            };
            let Ok(native_window) = web.HWND() else {
                continue;
            };
            let hwnd = HWND(native_window.0 as *mut _);
            let Ok(services) = dispatch.cast::<IServiceProvider>() else {
                continue;
            };
            let Ok(browser) = services.QueryService::<IShellBrowser>(&SID_STopLevelBrowser) else {
                continue;
            };
            let Ok(shell_view) = browser.QueryActiveShellView() else {
                continue;
            };
            let Ok(view) = shell_view.cast::<IFolderView2>() else {
                continue;
            };
            if !view_matches(&view, folder_pidl) {
                continue;
            }
            if hwnd == foreground {
                return Some((view, hwnd));
            }
            if Some(hwnd) == last_live_window {
                recent = Some((view.clone(), hwnd));
            }
            first.get_or_insert((view, hwnd));
        }
        recent.or(first)
    }
}

unsafe fn view_matches(view: &IFolderView2, folder_pidl: *const ITEMIDLIST) -> bool {
    unsafe {
        let Ok(folder) = view.GetFolder::<IPersistFolder2>() else {
            return false;
        };
        let Ok(current) = folder.GetCurFolder() else {
            return false;
        };
        let current = OwnedPidl(current);
        windows::Win32::UI::Shell::ILIsEqual(current.as_ptr(), folder_pidl).as_bool()
            || pidl_path(current.as_ptr()).is_some_and(|current_path| {
                pidl_path(folder_pidl).is_some_and(|folder_path| {
                    canonical_shell_path(&current_path).ok()
                        == canonical_shell_path(&folder_path).ok()
                })
            })
    }
}

unsafe fn pidl_path(pidl: *const ITEMIDLIST) -> Option<PathBuf> {
    unsafe {
        let item: IShellItem = SHCreateItemFromIDList(pidl).ok()?;
        shell_item_path(&item)
    }
}

unsafe fn capture_view(
    view: &IFolderView2,
    folder: &Path,
    folder_pidl: &OwnedPidl,
    source: FolderSnapshotSource,
    generation: u64,
) -> Option<FolderSnapshot> {
    unsafe {
        let array: IShellItemArray = view.Items(SVGIO_ALLVIEW | SVGIO_FLAG_VIEWORDER).ok()?;
        let mut items = Vec::new();
        for index in 0..array.GetCount().ok()? {
            let item = array.GetItemAt(index).ok()?;
            let Some(path) = shell_item_path(&item) else {
                continue;
            };
            let Some(kind) = MediaKind::from_path(&path) else {
                continue;
            };
            let absolute_pidl = OwnedPidl(SHGetIDListFromObject(&item).ok()?);
            items.push(FolderMediaItem {
                identity: absolute_pidl.identity(),
                path,
                kind,
            });
        }

        let count = view.GetSortColumnCount().ok()?.max(0) as usize;
        let mut native_columns = vec![SORTCOLUMN::default(); count];
        view.GetSortColumns(&mut native_columns).ok()?;
        let sort_columns = native_columns
            .into_iter()
            .map(sort_column)
            .collect::<Option<Vec<_>>>()?;

        Some(FolderSnapshot {
            folder_identity: folder_pidl.identity(),
            folder_path: folder.to_owned(),
            items,
            sort_columns,
            source,
            generation,
            captured_at: SystemTime::now(),
        })
    }
}

fn sort_column(column: SORTCOLUMN) -> Option<SortColumn> {
    let direction = match column.direction {
        SORT_ASCENDING => SortDirection::Ascending,
        SORT_DESCENDING => SortDirection::Descending,
        _ => return None,
    };
    Some(SortColumn {
        property: property_key(column.propkey),
        direction,
    })
}

fn property_key(key: PROPERTYKEY) -> PropertyKey {
    PropertyKey {
        format_id: key.fmtid.to_u128(),
        property_id: key.pid,
    }
}

fn fallback_snapshot(folder: &Path, generation: u64) -> FolderSnapshot {
    let folder = canonical_shell_path(folder).unwrap_or_else(|_| folder.to_owned());
    let mut items = std::fs::read_dir(&folder)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let kind = MediaKind::from_path(&path)?;
            let identity = parse_path(&path)
                .map(|pidl| pidl.identity())
                .unwrap_or_else(|| path_identity(&path));
            Some(FolderMediaItem {
                identity,
                path,
                kind,
            })
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| natural_path_cmp(&left.path, &right.path));
    let folder_identity = parse_path(&folder)
        .map(|pidl| pidl.identity())
        .unwrap_or_else(|| path_identity(&folder));
    FolderSnapshot {
        folder_identity,
        folder_path: folder,
        items,
        sort_columns: Vec::new(),
        source: FolderSnapshotSource::NaturalNameFallback,
        generation,
        captured_at: SystemTime::now(),
    }
}

fn natural_path_cmp(left: &Path, right: &Path) -> Ordering {
    let left = wide_null(left.file_name().unwrap_or_else(|| OsStr::new("")));
    let right = wide_null(right.file_name().unwrap_or_else(|| OsStr::new("")));
    // SAFETY: both arguments are valid, terminated UTF-16 strings for this call.
    match unsafe { StrCmpLogicalW(PCWSTR(left.as_ptr()), PCWSTR(right.as_ptr())) } {
        value if value < 0 => Ordering::Less,
        0 => Ordering::Equal,
        _ => Ordering::Greater,
    }
}

fn parse_path(path: &Path) -> Option<OwnedPidl> {
    let wide = wide_null(path.as_os_str());
    let mut pidl = std::ptr::null_mut();
    // SAFETY: output ownership is transferred to OwnedPidl; the input is terminated UTF-16.
    unsafe {
        SHParseDisplayName(PCWSTR(wide.as_ptr()), None, &mut pidl, 0, None).ok()?;
    }
    (!pidl.is_null()).then_some(OwnedPidl(pidl))
}

unsafe fn shell_item_path(item: &IShellItem) -> Option<PathBuf> {
    unsafe {
        let value = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = value.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(value.0.cast()));
        path
    }
}

fn path_identity(path: &Path) -> ShellIdentity {
    let bytes = path
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    ShellIdentity::new(bytes)
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

pub fn canonical_shell_path(path: &Path) -> std::io::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    let wide = canonical.as_os_str().encode_wide().collect::<Vec<_>>();
    const EXTENDED: &[u16] = &[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    const UNC: &[u16] = &[
        b'\\' as u16,
        b'\\' as u16,
        b'?' as u16,
        b'\\' as u16,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        b'\\' as u16,
    ];
    let normalized = if let Some(rest) = wide.strip_prefix(UNC) {
        [b'\\' as u16, b'\\' as u16]
            .into_iter()
            .chain(rest.iter().copied())
            .collect()
    } else if let Some(rest) = wide.strip_prefix(EXTENDED) {
        rest.to_vec()
    } else {
        wide
    };
    Ok(PathBuf::from(OsString::from_wide(&normalized)))
}

struct OwnedPidl(*mut ITEMIDLIST);

impl OwnedPidl {
    fn as_ptr(&self) -> *const ITEMIDLIST {
        self.0.cast_const()
    }

    fn identity(&self) -> ShellIdentity {
        // SAFETY: the PIDL is owned and valid for the lifetime of self.
        unsafe {
            let size = windows::Win32::UI::Shell::ILGetSize(Some(self.as_ptr())) as usize;
            ShellIdentity::new(std::slice::from_raw_parts(self.0.cast::<u8>(), size).to_vec())
        }
    }
}

impl Drop for OwnedPidl {
    fn drop(&mut self) {
        // SAFETY: this allocation came from a Shell API using the COM allocator and is freed once.
        unsafe { CoTaskMemFree(Some(self.0.cast())) };
    }
}

struct HiddenExplorerBrowser {
    browser: IExplorerBrowser,
    host: HWND,
}

impl HiddenExplorerBrowser {
    unsafe fn new() -> Option<Self> {
        unsafe {
            let host = match CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!(""),
                WINDOW_STYLE::default(),
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            ) {
                Ok(host) => host,
                Err(error) => {
                    eprintln!("towavue: hidden Shell host creation failed: {error}");
                    return None;
                }
            };
            let browser: IExplorerBrowser =
                match CoCreateInstance(&ExplorerBrowser, None, CLSCTX_ALL) {
                    Ok(browser) => browser,
                    Err(_) => {
                        eprintln!("towavue: IExplorerBrowser creation failed");
                        let _ = DestroyWindow(host);
                        return None;
                    }
                };
            if browser
                .SetOptions(EBO_NOBORDER | EBO_NOPERSISTVIEWSTATE | EBO_NOTRAVELLOG)
                .is_err()
            {
                eprintln!("towavue: IExplorerBrowser SetOptions failed");
                let _ = DestroyWindow(host);
                return None;
            }
            let rect = RECT {
                left: 0,
                top: 0,
                right: 1,
                bottom: 1,
            };
            if browser.Initialize(host, &rect, None).is_err() {
                eprintln!("towavue: IExplorerBrowser initialization failed");
                let _ = DestroyWindow(host);
                return None;
            }
            Some(Self { browser, host })
        }
    }
}

impl Drop for HiddenExplorerBrowser {
    fn drop(&mut self) {
        // SAFETY: both objects were created and are destroyed on this STA thread.
        unsafe {
            let _ = self.browser.Destroy();
            let _ = DestroyWindow(self.host);
        }
    }
}

unsafe fn pump_messages() {
    unsafe {
        let mut message = MSG::default();
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::fs::OpenOptions;
    use std::os::windows::io::AsRawHandle;
    use std::process::Command;
    use windows::Win32::Foundation::{FILETIME, HANDLE, LPARAM, WPARAM};
    use windows::Win32::Storage::FileSystem::SetFileTime;
    use windows::Win32::UI::Shell::{FWF_AUTOARRANGE, IShellView, SORTDIRECTION};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    use windows::core::GUID;

    #[test]
    fn relative_paths_match_shell_style_absolute_paths() {
        let relative = canonical_shell_path(Path::new("Cargo.toml")).expect("relative manifest");
        let directory = std::env::current_dir().expect("test directory");
        let absolute =
            canonical_shell_path(&directory.join("Cargo.toml")).expect("absolute manifest");
        assert!(relative.is_absolute());
        assert_eq!(relative, absolute);
        assert!(!relative.to_string_lossy().starts_with(r"\\?\"));
    }

    const STORAGE_PROPERTY_FORMAT: GUID = GUID::from_u128(0xb725f130_47ef_101a_a5f1_02608c9eebac);
    const NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: STORAGE_PROPERTY_FORMAT,
        pid: 10,
    };
    const SIZE: PROPERTYKEY = PROPERTYKEY {
        fmtid: STORAGE_PROPERTY_FORMAT,
        pid: 12,
    };
    const DATE_MODIFIED: PROPERTYKEY = PROPERTYKEY {
        fmtid: STORAGE_PROPERTY_FORMAT,
        pid: 14,
    };
    const DATE_CREATED: PROPERTYKEY = PROPERTYKEY {
        fmtid: STORAGE_PROPERTY_FORMAT,
        pid: 15,
    };
    const TYPE: PROPERTYKEY = PROPERTYKEY {
        fmtid: STORAGE_PROPERTY_FORMAT,
        pid: 4,
    };

    #[test]
    fn idle_shell_worker_services_window_messages_and_closes_without_requests() {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_NULL,
        };

        let shared = Arc::new((
            Mutex::new(Mailbox::default()),
            ShellWake::new().expect("wake event"),
        ));
        let provider = FolderOrderProvider {
            shared: Arc::clone(&shared),
        };
        let (window_tx, window_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            // SAFETY: this thread owns the hidden test window until its request loop returns.
            // Only its integer identity crosses threads for a bounded, scalar message probe.
            let window = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("STATIC"),
                    w!(""),
                    WINDOW_STYLE::default(),
                    0,
                    0,
                    1,
                    1,
                    None,
                    None,
                    None,
                    None,
                )
                .expect("hidden Shell-thread test window")
            };
            window_tx
                .send(window.0 as usize)
                .expect("test window identity");
            run_requests(shared, || {}, |_, _| panic!("no folder requests"));
            // SAFETY: the request loop has stopped; destroy this thread's retained window.
            unsafe { DestroyWindow(window).expect("destroy test window") };
        });
        let window = window_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("worker window");
        let responsive = (0..3).all(|_| {
            // Separate probes so dispatch at startup alone cannot normally satisfy this test.
            thread::sleep(Duration::from_millis(20));
            // SAFETY: the worker retains this HWND until provider drop below. WM_NULL has no
            // pointer payload; timeout bounds the call without terminating the worker.
            unsafe {
                SendMessageTimeoutW(
                    HWND(window as *mut _),
                    WM_NULL,
                    WPARAM(0),
                    LPARAM(0),
                    SMTO_ABORTIFHUNG,
                    1000,
                    None,
                )
                .0 != 0
            }
        });
        drop(provider);
        worker.join().expect("idle worker exits on close");
        assert!(
            responsive,
            "idle Shell STA must dispatch incoming window messages"
        );
    }

    #[test]
    fn asynchronous_requests_replace_queued_work_and_reject_stale_results() {
        let shared = Arc::new((
            Mutex::new(Mailbox::default()),
            ShellWake::new().expect("wake event"),
        ));
        let provider = FolderOrderProvider {
            shared: Arc::clone(&shared),
        };
        let (started_tx, started_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            run_requests(
                shared,
                || {
                    ready_tx.send(()).expect("completion notification");
                },
                |path, generation| {
                    started_tx.send(path.to_owned()).expect("started request");
                    if path == Path::new("first") {
                        resume_rx
                            .recv_timeout(Duration::from_secs(5))
                            .expect("release first request");
                    }
                    FolderSnapshot {
                        folder_identity: ShellIdentity::new(Vec::new()),
                        folder_path: path.to_owned(),
                        items: Vec::new(),
                        sort_columns: Vec::new(),
                        source: FolderSnapshotSource::PersistedShellView,
                        generation,
                        captured_at: SystemTime::now(),
                    }
                },
            )
        });
        provider.request(Some("first".into()));
        assert_eq!(
            started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("first starts"),
            PathBuf::from("first")
        );
        provider.request(Some("middle".into()));
        let generation = provider.request(Some("last".into()));
        assert!(provider.take_completed().is_none());
        resume_tx.send(()).expect("resume worker");
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("latest result");
        let result = provider.take_completed().expect("completed snapshot");
        assert_eq!(result.generation, generation);
        assert_eq!(result.folder_path, PathBuf::from("last"));
        assert_eq!(
            started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("last starts"),
            PathBuf::from("last")
        );
        assert!(started_rx.try_recv().is_err());
        assert!(ready_rx.try_recv().is_err());
        provider.request(Some("ready".into()));
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("completed slot");
        provider.request(None);
        assert!(provider.take_completed().is_none());
        drop(provider);
        worker.join().expect("closed worker exits");
    }

    #[test]
    fn closing_during_shell_work_does_not_wait_or_publish() {
        let shared = Arc::new((
            Mutex::new(Mailbox::default()),
            ShellWake::new().expect("wake event"),
        ));
        let provider = FolderOrderProvider {
            shared: Arc::clone(&shared),
        };
        let (started_tx, started_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            run_requests(
                shared,
                || {
                    ready_tx.send(()).expect("completion notification");
                },
                |path, generation| {
                    started_tx.send(()).expect("started request");
                    resume_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("release after close");
                    FolderSnapshot {
                        folder_identity: ShellIdentity::new(Vec::new()),
                        folder_path: path.to_owned(),
                        items: Vec::new(),
                        sort_columns: Vec::new(),
                        source: FolderSnapshotSource::PersistedShellView,
                        generation,
                        captured_at: SystemTime::now(),
                    }
                },
            )
        });
        provider.request(Some("blocked".into()));
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("request starts");
        drop(provider);
        resume_tx
            .send(())
            .expect("close returned before worker completed");
        worker.join().expect("worker exits without publishing");
        assert!(ready_rx.try_recv().is_err());
    }

    #[test]
    fn windows_natural_order_handles_numeric_names() {
        assert_eq!(
            natural_path_cmp(Path::new("item2.jpg"), Path::new("item10.jpg")),
            Ordering::Less
        );
    }

    #[test]
    fn hidden_shell_view_returns_ordered_media() {
        let root = test_directory("basic");
        fs::create_dir(&root).expect("create fixture folder");
        fs::write(root.join("item10.jpg"), []).expect("create fixture");
        fs::write(root.join("item2.jpg"), []).expect("create fixture");

        let mut provider = FolderOrderProvider::new().expect("start Shell worker");
        let snapshot = provider.snapshot(&root).expect("capture folder order");

        assert_eq!(snapshot.items.len(), 2);
        assert_ne!(
            snapshot.source,
            FolderSnapshotSource::NaturalNameFallback,
            "IExplorerBrowser must resolve the persisted/default Shell view"
        );
        assert_eq!(
            snapshot.items[0].path.file_name(),
            Some(OsStr::new("item2.jpg"))
        );

        drop(provider);
        fs::remove_dir_all(root).expect("remove fixture folder");
    }

    #[test]
    #[ignore = "opens and closes a real Explorer window"]
    fn live_explorer_sort_matrix_matches_metadata_and_view_order() {
        let root = test_directory("sort-matrix");
        fs::create_dir(&root).expect("create fixture folder");
        create_timed_file(&root.join("item10.jpg"), 100, 1, 4);
        create_timed_file(&root.join("item2.jpg"), 200, 2, 3);
        create_timed_file(&root.join("alpha.png"), 100, 3, 2);
        create_timed_file(&root.join("beta.gif"), 300, 4, 1);

        // SAFETY: this test owns the apartment, browser, PIDL, and all view
        // interfaces on the current thread.
        unsafe {
            OleInitialize(None).expect("initialize Shell test STA");
            let folder = canonical_shell_path(&root).expect("canonical fixture folder");
            let folder_pidl = parse_path(&folder).expect("parse fixture folder");
            let mut explorer_launcher = Command::new("explorer.exe")
                .arg(&folder)
                .spawn()
                .expect("open fixture folder in Explorer");
            let (view, explorer_window) = wait_for_live_view(folder_pidl.as_ptr());
            view.SetCurrentFolderFlags(FWF_AUTOARRANGE.0 as u32, FWF_AUTOARRANGE.0 as u32)
                .expect("enable automatic Shell view arrangement");

            for direction in [SORT_ASCENDING, SORT_DESCENDING] {
                let snapshot = set_sort_and_capture(
                    &view,
                    &folder,
                    &folder_pidl,
                    &[native_column(NAME, direction)],
                );
                for pair in snapshot.items.windows(2) {
                    let ordering = natural_path_cmp(&pair[0].path, &pair[1].path);
                    if direction == SORT_DESCENDING {
                        assert!(ordering != Ordering::Less);
                    } else {
                        assert!(ordering != Ordering::Greater);
                    }
                }
            }

            for (property, value) in [
                (DATE_MODIFIED, order_by_modified as fn(&Path) -> SortValue),
                (DATE_CREATED, order_by_created),
                (SIZE, order_by_size),
            ] {
                let ascending = set_sort_and_capture(
                    &view,
                    &folder,
                    &folder_pidl,
                    &[native_column(property, SORT_ASCENDING)],
                );
                assert_monotonic(&ascending, value, false);
                let descending = set_sort_and_capture(
                    &view,
                    &folder,
                    &folder_pidl,
                    &[native_column(property, SORT_DESCENDING)],
                );
                assert_monotonic(&descending, value, true);
            }

            let type_ascending = set_sort_and_capture(
                &view,
                &folder,
                &folder_pidl,
                &[
                    native_column(TYPE, SORT_ASCENDING),
                    native_column(NAME, SORT_ASCENDING),
                ],
            );
            let type_descending = set_sort_and_capture(
                &view,
                &folder,
                &folder_pidl,
                &[
                    native_column(TYPE, SORT_DESCENDING),
                    native_column(NAME, SORT_ASCENDING),
                ],
            );
            let mut ascending_groups = extension_groups(&type_ascending);
            let descending_groups = extension_groups(&type_descending);
            ascending_groups.reverse();
            assert_eq!(descending_groups, ascending_groups);

            let multiple = set_sort_and_capture(
                &view,
                &folder,
                &folder_pidl,
                &[
                    native_column(SIZE, SORT_ASCENDING),
                    native_column(NAME, SORT_DESCENDING),
                ],
            );
            assert_monotonic(&multiple, order_by_size, false);
            let size_tie = multiple
                .items
                .windows(2)
                .find(|pair| order_by_size(&pair[0].path) == order_by_size(&pair[1].path))
                .expect("fixture contains a size tie");
            assert_eq!(
                natural_path_cmp(&size_tie[0].path, &size_tie[1].path),
                Ordering::Greater
            );
            view.cast::<IShellView>()
                .expect("query Shell view")
                .SaveViewState()
                .expect("save fixture view state");

            drop(view);
            PostMessageW(Some(explorer_window), WM_CLOSE, WPARAM(0), LPARAM(0))
                .expect("close fixture Explorer window");
            wait_for_live_close(folder_pidl.as_ptr());
            explorer_launcher
                .wait()
                .expect("wait for Explorer launcher process");
            drop(folder_pidl);
            OleUninitialize();
        }

        fs::remove_dir_all(root).expect("remove fixture folder");
    }

    #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
    enum SortValue {
        Time(SystemTime),
        Size(u64),
    }

    fn test_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "towavue-shell-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("clock is after Unix epoch")
                .as_nanos()
        ))
    }

    fn create_timed_file(path: &Path, size: usize, created_days: u64, modified_days: u64) {
        fs::write(path, vec![0_u8; size]).expect("create fixture file");
        let file = OpenOptions::new()
            .write(true)
            .open(path)
            .expect("open fixture file");
        let created = filetime(created_days);
        let modified = filetime(modified_days);
        // SAFETY: the borrowed std file handle remains valid for this call.
        unsafe {
            SetFileTime(
                HANDLE(file.as_raw_handle()),
                Some(&created),
                None,
                Some(&modified),
            )
            .expect("set fixture timestamps");
        }
    }

    fn filetime(days: u64) -> FILETIME {
        let ticks = ((20_000 + days) * 24 * 60 * 60 + 11_644_473_600) * 10_000_000;
        FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        }
    }

    unsafe fn wait_for_live_view(folder_pidl: *const ITEMIDLIST) -> (IFolderView2, HWND) {
        unsafe {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                pump_messages();
                if let Some(found) = matching_live_view(folder_pidl, None) {
                    return found;
                }
                assert!(
                    Instant::now() < deadline,
                    "Explorer window discovery timed out"
                );
                thread::sleep(Duration::from_millis(50));
            }
        }
    }

    unsafe fn wait_for_live_close(folder_pidl: *const ITEMIDLIST) {
        unsafe {
            let deadline = Instant::now() + Duration::from_secs(5);
            while matching_live_view(folder_pidl, None).is_some() {
                pump_messages();
                assert!(
                    Instant::now() < deadline,
                    "Explorer fixture window did not close"
                );
                thread::sleep(Duration::from_millis(50));
            }
        }
    }

    fn native_column(property: PROPERTYKEY, direction: SORTDIRECTION) -> SORTCOLUMN {
        SORTCOLUMN {
            propkey: property,
            direction,
        }
    }

    unsafe fn set_sort_and_capture(
        view: &IFolderView2,
        folder: &Path,
        folder_pidl: &OwnedPidl,
        columns: &[SORTCOLUMN],
    ) -> FolderSnapshot {
        unsafe {
            view.SetSortColumns(columns)
                .expect("set Shell sort columns");
            view.cast::<IShellView>()
                .expect("query Shell view")
                .Refresh()
                .expect("refresh Shell view");
            let settle = Instant::now() + Duration::from_millis(150);
            while Instant::now() < settle {
                pump_messages();
                thread::sleep(Duration::from_millis(10));
            }
            let expected = columns
                .iter()
                .copied()
                .map(sort_column)
                .collect::<Option<Vec<_>>>()
                .expect("valid test columns");
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                pump_messages();
                if let Some(snapshot) = capture_view(
                    view,
                    folder,
                    folder_pidl,
                    FolderSnapshotSource::PersistedShellView,
                    1,
                ) && snapshot.items.len() == 4
                    && snapshot.sort_columns == expected
                {
                    return snapshot;
                }
                assert!(Instant::now() < deadline, "Shell sort update timed out");
                thread::sleep(Duration::from_millis(10));
            }
        }
    }

    fn order_by_modified(path: &Path) -> SortValue {
        SortValue::Time(
            fs::metadata(path)
                .expect("read fixture metadata")
                .modified()
                .expect("read modified time"),
        )
    }

    fn order_by_created(path: &Path) -> SortValue {
        SortValue::Time(
            fs::metadata(path)
                .expect("read fixture metadata")
                .created()
                .expect("read creation time"),
        )
    }

    fn order_by_size(path: &Path) -> SortValue {
        SortValue::Size(fs::metadata(path).expect("read fixture metadata").len())
    }

    fn assert_monotonic(
        snapshot: &FolderSnapshot,
        value: fn(&Path) -> SortValue,
        descending: bool,
    ) {
        for pair in snapshot.items.windows(2) {
            let ordering = value(&pair[0].path).cmp(&value(&pair[1].path));
            if descending {
                assert!(
                    ordering != Ordering::Less,
                    "{} ({:?}) precedes {} ({:?}) in descending order",
                    pair[0].path.display(),
                    value(&pair[0].path),
                    pair[1].path.display(),
                    value(&pair[1].path)
                );
            } else {
                assert!(
                    ordering != Ordering::Greater,
                    "{} ({:?}) precedes {} ({:?}) in ascending order",
                    pair[0].path.display(),
                    value(&pair[0].path),
                    pair[1].path.display(),
                    value(&pair[1].path)
                );
            }
        }
    }

    fn extension_groups(snapshot: &FolderSnapshot) -> Vec<OsString> {
        let mut groups = Vec::new();
        for item in &snapshot.items {
            let extension = item
                .path
                .extension()
                .expect("fixture has an extension")
                .to_os_string();
            if groups.last() != Some(&extension) {
                assert!(!groups.contains(&extension), "type group is not contiguous");
                groups.push(extension);
            }
        }
        groups
    }
}
