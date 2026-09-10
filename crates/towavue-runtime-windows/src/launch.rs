use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::Security::{
    GetLengthSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

#[cfg(test)]
mod tests;

const CLASS: PCWSTR = w!("towavue-launch-v1");
const PROTOCOL: usize = 0x7476_0001;
const MAX_UNITS: usize = 32_768;
const ACK_TIMEOUT: Duration = Duration::from_secs(4);

/// An owned path-only request. Acknowledge only after native window startup succeeds.
pub struct LaunchRequest {
    pub path: Option<PathBuf>,
    reply: mpsc::SyncSender<bool>,
}

impl LaunchRequest {
    pub fn acknowledge(self, accepted: bool) {
        let _ = self.reply.send(accepted);
    }
}

pub enum LaunchRole {
    Primary(LaunchServer),
    Forwarded,
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns a successful kernel handle acquisition.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Owns a message-only receiver thread and the session-local host lifetime marker.
pub struct LaunchServer {
    window: isize,
    worker: Option<JoinHandle<()>>,
    stopping: Arc<AtomicBool>,
    _marker: Handle,
}

impl LaunchServer {
    pub fn start_or_forward(
        path: Option<&Path>,
        notify: impl Fn(LaunchRequest) + Send + 'static,
    ) -> io::Result<LaunchRole> {
        let executable = std::env::current_exe()?.canonicalize()?;
        // SAFETY: the current-process pseudo handle is borrowed, never closed.
        let user = unsafe { process_user(GetCurrentProcess())? };
        let identity = format!("towavue-v1-{user}-{}", executable.display());
        start_or_forward(&identity, &executable, path, Box::new(notify))
    }
}

impl Drop for LaunchServer {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        // SAFETY: the worker owns this message-only HWND until its close handler runs.
        // Posting requests destruction on that thread; the join precedes marker release.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.window as *mut _)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn encode(path: Option<&Path>) -> io::Result<Vec<u16>> {
    let mut units = vec![u16::from(path.is_some())];
    if let Some(path) = path {
        if !path.is_absolute() {
            return Err(io::Error::other("launch path must be absolute"));
        }
        units.extend(path.as_os_str().encode_wide());
        if units[1..].contains(&0) {
            return Err(io::Error::other("launch path contains NUL"));
        }
    }
    if units.len() > MAX_UNITS {
        return Err(io::Error::other("launch path is too long"));
    }
    Ok(units)
}

fn decode(units: &[u16]) -> Option<Option<PathBuf>> {
    match units {
        [0] => Some(None),
        [1, rest @ ..] if units.len() <= MAX_UNITS && !rest.is_empty() && !rest.contains(&0) => {
            let path = PathBuf::from(OsString::from_wide(rest));
            path.is_absolute().then_some(Some(path))
        }
        _ => None,
    }
}

// The caller supplies a live process handle with TOKEN_QUERY access available.
unsafe fn process_user(process: HANDLE) -> io::Result<String> {
    let mut token = HANDLE::default();
    // SAFETY: process is borrowed; all output buffers below are aligned, owned and
    // sized from GetTokenInformation. The SID remains inside that live token buffer.
    unsafe {
        OpenProcessToken(process, TOKEN_QUERY, &mut token).map_err(io::Error::other)?;
        let token = Handle(token);
        let mut bytes = 0;
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut bytes);
        if bytes < std::mem::size_of::<TOKEN_USER>() as u32 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0usize; (bytes as usize).div_ceil(std::mem::size_of::<usize>())];
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            bytes,
            &mut bytes,
        )
        .map_err(io::Error::other)?;
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let sid = std::slice::from_raw_parts(
            user.User.Sid.0.cast::<u8>(),
            GetLengthSid(user.User.Sid) as usize,
        );
        Ok(sid.iter().map(|byte| format!("{byte:02x}")).collect())
    }
}

fn start_or_forward(
    identity: &str,
    executable: &Path,
    path: Option<&Path>,
    notify: Box<dyn Fn(LaunchRequest) + Send>,
) -> io::Result<LaunchRole> {
    let payload = encode(path)?;
    let title = wide(identity);
    let marker_name = wide(&format!(
        "Local\\towavue-launch-{:08x}",
        crc32fast::hash(identity.as_bytes())
    ));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        // SAFETY: strings are terminated and live for each synchronous API call.
        // Default token DACL applies; Local limits the marker to this logon session.
        let (marker, existed) = unsafe {
            let marker = CreateMutexW(None, false, PCWSTR(marker_name.as_ptr()))
                .map_err(io::Error::other)?;
            (Handle(marker), GetLastError() == ERROR_ALREADY_EXISTS)
        };
        if !existed {
            return start_receiver(title, marker, notify).map(LaunchRole::Primary);
        }
        drop(marker);
        // SAFETY: only the exact protocol class/title in this desktop's message-only
        // namespace is queried; there is no broadcast or foreground manipulation.
        let window =
            unsafe { FindWindowExW(Some(HWND_MESSAGE), None, CLASS, PCWSTR(title.as_ptr())) };
        if let Ok(window) = window {
            forward(window, executable, &payload)?;
            return Ok(LaunchRole::Forwarded);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(
                "existing towavue host did not become ready; no duplicate window was started",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn forward(window: HWND, executable: &Path, payload: &[u16]) -> io::Result<()> {
    // SAFETY: process query handles are owned locally; UTF-16 buffer and COPYDATA
    // payload remain immutable and live through synchronous, bounded SendMessage.
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        let process = Handle(
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(io::Error::other)?,
        );
        let mut name = vec![0u16; 32_768];
        let mut length = name.len() as u32;
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(name.as_mut_ptr()),
            &mut length,
        )
        .map_err(io::Error::other)?;
        let actual = PathBuf::from(OsString::from_wide(&name[..length as usize])).canonicalize()?;
        if actual != executable || process_user(process.0)? != process_user(GetCurrentProcess())? {
            return Err(io::Error::other(
                "launch receiver identity does not match this executable and user",
            ));
        }
        let _ = AllowSetForegroundWindow(pid);
        let data = COPYDATASTRUCT {
            dwData: PROTOCOL,
            cbData: (payload.len() * 2) as u32,
            lpData: payload.as_ptr() as *mut _,
        };
        let mut accepted = 0;
        let sent = if pid == GetCurrentProcessId() {
            // In-process callers keep the borrowed buffer alive until the procedure
            // returns; do not rely on cross-process WM_COPYDATA marshalling here.
            accepted = SendMessageW(
                window,
                WM_COPYDATA,
                Some(WPARAM(0)),
                Some(LPARAM(&data as *const _ as isize)),
            )
            .0 as usize;
            LRESULT(1)
        } else {
            SendMessageTimeoutW(
                window,
                WM_COPYDATA,
                WPARAM(0),
                LPARAM(&data as *const _ as isize),
                SMTO_BLOCK | SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
                5_000,
                Some(&mut accepted),
            )
        };
        if sent.0 == 0 || accepted != 1 {
            return Err(io::Error::other(
                "could not confirm the new window in the existing towavue host; it may still open; no duplicate was started",
            ));
        }
    }
    Ok(())
}

struct ReceiverState {
    notify: Box<dyn Fn(LaunchRequest) + Send>,
    stopping: Arc<AtomicBool>,
}

fn start_receiver(
    title: Vec<u16>,
    marker: Handle,
    notify: Box<dyn Fn(LaunchRequest) + Send>,
) -> io::Result<LaunchServer> {
    let (ready, receive_ready) = mpsc::sync_channel(1);
    let stopping = Arc::new(AtomicBool::new(false));
    let state = ReceiverState {
        notify,
        stopping: Arc::clone(&stopping),
    };
    let worker = thread::Builder::new()
        .name("towavue-launch".into())
        .spawn(move || {
            let mut state = Box::new(state);
            // SAFETY: this thread owns registration, HWND, message pump and boxed state.
            // The box is stable until DestroyWindow finishes and never escapes to app code.
            let window = unsafe {
                (|| -> windows::core::Result<HWND> {
                    let instance = GetModuleHandleW(None)?.into();
                    let class = WNDCLASSW {
                        lpfnWndProc: Some(window_proc),
                        hInstance: instance,
                        lpszClassName: CLASS,
                        ..Default::default()
                    };
                    if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
                        return Err(windows::core::Error::from_thread());
                    }
                    CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        CLASS,
                        PCWSTR(title.as_ptr()),
                        WINDOW_STYLE::default(),
                        0,
                        0,
                        0,
                        0,
                        Some(HWND_MESSAGE),
                        None,
                        Some(instance),
                        Some((&mut *state as *mut ReceiverState).cast()),
                    )
                })()
            };
            let window = match window {
                Ok(window) => window,
                Err(error) => {
                    let _ = ready.send(Err(io::Error::other(error)));
                    return;
                }
            };
            let _ = ready.send(Ok(window.0 as isize));
            // SAFETY: only this thread dispatches its receiver's messages and destroys it.
            unsafe {
                let mut message = MSG::default();
                while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                    DispatchMessageW(&message);
                }
                if IsWindow(Some(window)).as_bool() {
                    let _ = DestroyWindow(window);
                }
            }
        })?;
    match receive_ready.recv() {
        Ok(Ok(window)) => Ok(LaunchServer {
            window,
            worker: Some(worker),
            stopping,
            _marker: marker,
        }),
        Ok(Err(error)) => {
            let _ = worker.join();
            Err(error)
        }
        Err(error) => {
            let _ = worker.join();
            Err(io::Error::other(error))
        }
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: Windows supplies CREATESTRUCT/COPYDATA storage for the duration of its
    // synchronous callbacks. The worker-owned state outlives HWND destruction. Copy
    // the bounded payload before returning, and never unwind across this ABI boundary.
    unsafe {
        match message {
            WM_NCCREATE => {
                let create = &*(lparam.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
                return DefWindowProcW(window, message, wparam, lparam);
            }
            WM_COPYDATA if lparam.0 != 0 => {
                let state = GetWindowLongPtrW(window, GWLP_USERDATA) as *const ReceiverState;
                if state.is_null() || (*state).stopping.load(Ordering::Acquire) {
                    return LRESULT(0);
                }
                let data = &*(lparam.0 as *const COPYDATASTRUCT);
                if data.dwData != PROTOCOL
                    || data.cbData < 2
                    || data.cbData as usize > MAX_UNITS * 2
                    || !data.cbData.is_multiple_of(2)
                    || data.lpData.is_null()
                {
                    return LRESULT(0);
                }
                let bytes =
                    std::slice::from_raw_parts(data.lpData.cast::<u8>(), data.cbData as usize);
                let units: Vec<u16> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect();
                let Some(path) = decode(&units) else {
                    return LRESULT(0);
                };
                let (reply, receive_reply) = mpsc::sync_channel(1);
                let accepted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    ((*state).notify)(LaunchRequest { path, reply });
                    receive_reply.recv_timeout(ACK_TIMEOUT).unwrap_or(false)
                }))
                .unwrap_or(false);
                return LRESULT(isize::from(accepted));
            }
            WM_CLOSE => {
                let _ = DestroyWindow(window);
                return LRESULT(0);
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                return LRESULT(0);
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            }
            _ => {}
        }
        DefWindowProcW(window, message, wparam, lparam)
    }
}
