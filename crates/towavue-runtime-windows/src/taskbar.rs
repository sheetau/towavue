use std::cell::Cell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{
    DefSubclassProc, GetWindowSubclass, ITaskbarList3, RemoveWindowSubclass, SetWindowSubclass,
    TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL, TaskbarList,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowThreadProcessId, RegisterWindowMessageW, WM_NCDESTROY,
};
use windows::core::w;
use winit::window::Window;

mod icons;
#[cfg(test)]
mod transport_tests;
pub use icons::TaskbarIcons;
use windows::Win32::UI::Shell::{
    THB_FLAGS, THB_ICON, THB_TOOLTIP, THBF_DISABLED, THBF_ENABLED, THBF_HIDDEN, THBN_CLICKED,
    THUMBBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::WM_COMMAND;
use windows::Win32::UI::WindowsAndMessaging::{
    CHANGEFILTERSTRUCT, ChangeWindowMessageFilterEx, MSGFLT_ALLOW, MSGFLT_DISALLOW, MSGFLTINFO_NONE,
};

const SUBCLASS_ID: usize = 0x7476_7462;
const BUTTON_BASE: u32 = 0x7470;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskbarAction {
    Previous,
    PlayPause,
    Next,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskbarEvent {
    Ready,
    Click { context: u64, action: TaskbarAction },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskbarTransport {
    /// Opaque owner revision returned with a click; revalidate it on delivery.
    pub context: u64,
    pub previous: bool,
    pub play_pause: bool,
    pub next: bool,
    pub playing: bool,
}

impl TaskbarTransport {
    fn enabled(self, action: TaskbarAction) -> bool {
        match action {
            TaskbarAction::Previous => self.previous,
            TaskbarAction::PlayPause => self.play_pause,
            TaskbarAction::Next => self.next,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TaskbarProgress {
    #[default]
    Hidden,
    Indeterminate,
    /// Thousandths of the complete operation, clamped to 1000.
    Fraction(u16),
}

struct State {
    message: u32,
    epoch: Cell<u64>,
    alive: Cell<bool>,
    transport: Cell<Option<TaskbarTransport>>,
    notify: Box<dyn Fn(TaskbarEvent)>,
}

struct Apartment(PhantomData<Rc<()>>);

struct MessageFilter {
    window: HWND,
    message: u32,
    changed: bool,
}

impl MessageFilter {
    fn allow(window: HWND, message: u32) -> windows::core::Result<Self> {
        let mut previous = CHANGEFILTERSTRUCT {
            cbSize: std::mem::size_of::<CHANGEFILTERSTRUCT>() as u32,
            ..Default::default()
        };
        // SAFETY: same-thread owned window. Admit only the scalar Shell readiness
        // and thumbnail command protocols when Explorer has lower integrity.
        unsafe { ChangeWindowMessageFilterEx(window, message, MSGFLT_ALLOW, Some(&mut previous))? };
        Ok(Self {
            window,
            message,
            changed: previous.ExtStatus == MSGFLTINFO_NONE,
        })
    }
}

impl Drop for MessageFilter {
    fn drop(&mut self) {
        if self.changed {
            // SAFETY: runs before the retained window is released; do not change
            // any process-wide filter or a message previously allowed by others.
            let _ = unsafe {
                ChangeWindowMessageFilterEx(self.window, self.message, MSGFLT_DISALLOW, None)
            };
        }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: paired with successful initialization on this same UI thread.
        unsafe { CoUninitialize() };
    }
}

struct Connection {
    interface: ITaskbarList3,
    // Field order releases the interface before balancing the apartment reference.
    _apartment: Apartment,
}

impl Connection {
    fn new() -> windows::core::Result<Self> {
        // SAFETY: caller is the retained window's UI thread; this !Send guard
        // balances both S_OK and S_FALSE and outlives every interface reference.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
        let apartment = Apartment(PhantomData);
        // SAFETY: initialized apartment; the returned interface remains thread-local.
        let interface: ITaskbarList3 =
            unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)? };
        // SAFETY: initializes this freshly created, thread-owned Shell interface.
        unsafe { interface.HrInit()? };
        Ok(Self {
            interface,
            _apartment: apartment,
        })
    }

    fn progress(&self, window: HWND, progress: TaskbarProgress) -> windows::core::Result<()> {
        // SAFETY: the owner retains its same-thread HWND. Shell copies scalar values.
        unsafe {
            match progress {
                TaskbarProgress::Hidden => self.interface.SetProgressState(window, TBPF_NOPROGRESS),
                TaskbarProgress::Indeterminate => {
                    self.interface.SetProgressState(window, TBPF_INDETERMINATE)
                }
                TaskbarProgress::Fraction(value) => {
                    self.interface.SetProgressState(window, TBPF_NORMAL)?;
                    self.interface
                        .SetProgressValue(window, u64::from(value.min(1000)), 1000)
                }
            }
        }
    }
}

/// Thread-owned Shell integration. Install before showing the window; `notify`
/// should enqueue a wakeup, never do rendering or COM work inside the subclass.
pub struct NativeTaskbar {
    _filters: [MessageFilter; 2],
    _window: Arc<Window>,
    handle: HWND,
    state: Rc<State>,
    connection: Option<Connection>,
    epoch: u64,
    applied: Option<TaskbarProgress>,
    failed: bool,
    icons: Option<TaskbarIcons>,
    buttons_added: bool,
    transport_failed: bool,
    applied_transport: Option<Option<TaskbarTransport>>,
}

impl NativeTaskbar {
    pub fn new(
        window: Arc<Window>,
        notify: impl Fn(TaskbarEvent) + 'static,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
            return Err("Taskbar integration requires a Windows window".into());
        };
        let handle = HWND(handle.hwnd.get() as *mut _);
        // SAFETY: the retained winit window owns this handle; synchronous queries only.
        if unsafe { GetWindowThreadProcessId(handle, None) != GetCurrentThreadId() } {
            return Err("Taskbar integration must be installed on the window thread".into());
        }
        // SAFETY: same-thread query; replacing this ID would leak its existing Rc.
        if unsafe { GetWindowSubclass(handle, Some(taskbar_proc), SUBCLASS_ID, None) }.as_bool() {
            return Err("Taskbar integration is already installed on this window".into());
        }
        // SAFETY: Windows copies this static message name.
        let message = unsafe { RegisterWindowMessageW(w!("TaskbarButtonCreated")) };
        if message == 0 {
            return Err(windows::core::Error::from_thread().into());
        }
        let filters = [
            MessageFilter::allow(handle, message)?,
            MessageFilter::allow(handle, WM_COMMAND)?,
        ];
        let state = Rc::new(State {
            message,
            epoch: Cell::new(0),
            alive: Cell::new(true),
            transport: Cell::new(None),
            notify: Box::new(notify),
        });
        let callback = Rc::into_raw(state.clone());
        // SAFETY: the subclass owns one Rc reference until removal/destruction.
        // The Rc-containing owner is !Send and retains the same-thread HWND.
        if !unsafe { SetWindowSubclass(handle, Some(taskbar_proc), SUBCLASS_ID, callback as usize) }
            .as_bool()
        {
            // SAFETY: failed registration did not consume the callback reference.
            unsafe { drop(Rc::from_raw(callback)) };
            return Err("Could not install taskbar message handling".into());
        }
        Ok(Self {
            _filters: filters,
            _window: window,
            handle,
            state,
            connection: None,
            epoch: 0,
            applied: None,
            failed: false,
            icons: None,
            buttons_added: false,
            transport_failed: false,
            applied_transport: None,
        })
    }

    pub fn set_progress(&mut self, progress: TaskbarProgress) -> windows::core::Result<()> {
        if !self.ready() {
            return Ok(());
        }
        if self.failed || self.applied == Some(progress) {
            return Ok(());
        }
        if self.connection.is_none() && progress == TaskbarProgress::Hidden {
            // An idle window has nothing to clear after Shell creation/recreation.
            self.applied = Some(progress);
            return Ok(());
        }
        let result = (|| {
            if self.connection.is_none() {
                self.connection = Some(Connection::new()?);
            }
            self.connection
                .as_ref()
                .expect("connected taskbar")
                .progress(self.handle, progress)
        })();
        if result.is_ok() {
            self.applied = Some(progress);
        } else {
            // Do not retry a failing Shell call on every frame or idle wakeup.
            self.failed = true;
        }
        result
    }

    fn ready(&mut self) -> bool {
        let epoch = self.state.epoch.get();
        if !self.state.alive.get() || epoch == 0 {
            return false;
        }
        if self.epoch != epoch {
            // Explorer can recreate its taskbar; never reuse the previous proxy/cache.
            self.connection = None;
            self.epoch = epoch;
            self.applied = None;
            self.failed = false;
            self.buttons_added = false;
            self.transport_failed = false;
            self.applied_transport = None;
        }
        true
    }

    pub fn set_icons(&mut self, icons: TaskbarIcons) {
        self.icons = Some(icons);
        self.applied_transport = None;
    }

    pub fn set_transport(
        &mut self,
        transport: Option<TaskbarTransport>,
    ) -> windows::core::Result<()> {
        if !self.ready()
            || self.icons.is_none()
            || self.transport_failed
            || self.applied_transport == Some(transport)
        {
            return Ok(());
        }
        if !self.buttons_added && transport.is_none() {
            return Ok(());
        }
        // Invalidate the old clickable state before a possibly reentrant Shell call.
        self.state.transport.set(None);
        let result = (|| {
            if self.connection.is_none() {
                self.connection = Some(Connection::new()?);
            }
            let buttons = buttons(transport, self.icons.as_ref().expect("icons"));
            let interface = &self.connection.as_ref().expect("connection").interface;
            // SAFETY: same-thread retained HWND; Shell copies the bounded button
            // array and icon pixels. Keep order/IDs fixed for the HWND's lifetime.
            unsafe {
                if self.buttons_added {
                    interface.ThumbBarUpdateButtons(self.handle, &buttons)
                } else {
                    interface.ThumbBarAddButtons(self.handle, &buttons)
                }
            }
        })();
        if result.is_ok() {
            self.buttons_added = true;
            self.applied_transport = Some(transport);
            if self.epoch == self.state.epoch.get() {
                self.state.transport.set(transport);
            }
        } else {
            self.transport_failed = true;
        }
        result
    }
}

impl Drop for NativeTaskbar {
    fn drop(&mut self) {
        let _ = self.set_transport(None);
        if self.state.alive.get()
            && self.epoch == self.state.epoch.get()
            && let Some(connection) = &self.connection
        {
            // Removing integration while its window survives must not leave a bar.
            let _ = connection.progress(self.handle, TaskbarProgress::Hidden);
        }
        // SAFETY: retained HWND and !Send owner; removal precedes callback destruction.
        if unsafe { RemoveWindowSubclass(self.handle, Some(taskbar_proc), SUBCLASS_ID) }.as_bool() {
            // SAFETY: successful removal releases the subclass's strong reference.
            unsafe { drop(Rc::from_raw(Rc::as_ptr(&self.state))) };
        }
    }
}

unsafe extern "system" fn taskbar_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    let pointer = data as *const State;
    // SAFETY: registration owns a reference. Hold another across callbacks/default
    // processing, which may reenter and remove or destroy the subclass/window.
    let state = unsafe {
        Rc::increment_strong_count(pointer);
        Rc::from_raw(pointer)
    };
    if message == WM_NCDESTROY {
        state.alive.set(false);
        // SAFETY: same-thread removal of this registered subclass.
        if unsafe { RemoveWindowSubclass(window, Some(taskbar_proc), SUBCLASS_ID) }.as_bool() {
            // SAFETY: releases only registration's reference; the local Rc stays alive.
            unsafe { drop(Rc::from_raw(pointer)) };
        }
    } else if message == state.message {
        state.transport.set(None);
        state.epoch.set(state.epoch.get().wrapping_add(1).max(1));
        // A caller-supplied notifier must never unwind across the native ABI.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (state.notify)(TaskbarEvent::Ready)
        }));
    } else if message == WM_COMMAND
        && (wparam.0 >> 16) & 0xffff == THBN_CLICKED as usize
        && let Some(action) = match (wparam.0 & 0xffff) as u32 {
            BUTTON_BASE => Some(TaskbarAction::Previous),
            value if value == BUTTON_BASE + 1 => Some(TaskbarAction::PlayPause),
            value if value == BUTTON_BASE + 2 => Some(TaskbarAction::Next),
            _ => None,
        }
    {
        if let Some(transport) = state
            .transport
            .get()
            .filter(|transport| transport.enabled(action))
        {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (state.notify)(TaskbarEvent::Click {
                    context: transport.context,
                    action,
                })
            }));
        }
        return LRESULT(0);
    }
    // SAFETY: forward the original native message to the remaining subclass chain.
    unsafe { DefSubclassProc(window, message, wparam, lparam) }
}

fn buttons(transport: Option<TaskbarTransport>, icons: &TaskbarIcons) -> [THUMBBUTTON; 3] {
    let playing = transport.is_some_and(|state| state.playing);
    [
        TaskbarAction::Previous,
        TaskbarAction::PlayPause,
        TaskbarAction::Next,
    ]
    .map(|action| {
        let (index, icon, title) = match action {
            TaskbarAction::Previous => (0, 0, "Previous"),
            TaskbarAction::PlayPause => (
                1,
                if playing { 2 } else { 1 },
                if playing { "Pause" } else { "Play" },
            ),
            TaskbarAction::Next => (2, 3, "Next"),
        };
        let mut button = THUMBBUTTON {
            dwMask: THB_ICON | THB_TOOLTIP | THB_FLAGS,
            iId: BUTTON_BASE + index,
            hIcon: icons.handle(icon),
            dwFlags: transport.map_or(THBF_HIDDEN, |state| {
                if state.enabled(action) {
                    THBF_ENABLED
                } else {
                    THBF_DISABLED
                }
            }),
            ..Default::default()
        };
        for (target, unit) in button.szTip.iter_mut().zip(title.encode_utf16()) {
            *target = unit;
        }
        button
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, SendMessageW};
    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::platform::windows::EventLoopBuilderExtWindows;

    #[test]
    fn taskbar_subclass_owns_notifications_and_resets_after_shell_recreation() {
        run_trial(false);
    }

    #[test]
    #[ignore = "requires a running Windows Shell; sends progress to a hidden owned HWND"]
    fn taskbar_native_progress_accepts_all_states_and_reinitialization() {
        run_trial(true);
    }

    fn run_trial(native: bool) {
        if std::env::var_os("TOWAVUE_TASKBAR_TEST_CHILD").is_none() {
            let test = if native {
                "taskbar::tests::taskbar_native_progress_accepts_all_states_and_reinitialization"
            } else {
                "taskbar::tests::taskbar_subclass_owns_notifications_and_resets_after_shell_recreation"
            };
            let output =
                std::process::Command::new(std::env::current_exe().expect("test executable"))
                    .args(["--exact", test, "--include-ignored", "--nocapture"])
                    .env("TOWAVUE_TASKBAR_TEST_CHILD", "1")
                    .output()
                    .expect("isolated native trial");
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        struct Trial(bool);
        impl ApplicationHandler for Trial {
            fn resumed(&mut self, event_loop: &ActiveEventLoop) {
                let window = Arc::new(
                    event_loop
                        .create_window(Window::default_attributes().with_visible(false))
                        .expect("hidden window"),
                );
                let _caption =
                    crate::NativeCaption::new(window.clone()).expect("coexisting caption");
                let notifications = Rc::new(Cell::new(0));
                let count = notifications.clone();
                let mut taskbar =
                    NativeTaskbar::new(window.clone(), move |_| count.set(count.get() + 1))
                        .expect("taskbar");
                let state = taskbar.state.clone();
                assert_eq!(Rc::strong_count(&state), 3);
                assert!(NativeTaskbar::new(window.clone(), |_| {}).is_err());
                taskbar
                    .set_progress(TaskbarProgress::Fraction(500))
                    .expect("deferred before readiness");
                assert!(taskbar.connection.is_none());
                assert_eq!(taskbar.applied, None);
                for epoch in 1..=2 {
                    // SAFETY: synchronous delivery to this trial's own same-thread HWND.
                    unsafe { SendMessageW(taskbar.handle, state.message, None, None) };
                    assert_eq!(notifications.get(), epoch);
                    taskbar
                        .set_progress(TaskbarProgress::Hidden)
                        .expect("reset idle state");
                    assert_eq!(taskbar.epoch, epoch);
                    assert!(!taskbar.failed);
                    assert_eq!(taskbar.applied, Some(TaskbarProgress::Hidden));
                    if self.0 {
                        for progress in [
                            TaskbarProgress::Indeterminate,
                            TaskbarProgress::Fraction(0),
                            TaskbarProgress::Fraction(500),
                            TaskbarProgress::Fraction(2000),
                            TaskbarProgress::Hidden,
                        ] {
                            taskbar
                                .set_progress(progress)
                                .expect("native progress call");
                            assert_eq!(taskbar.applied, Some(progress));
                            taskbar.set_progress(progress).expect("unchanged state");
                        }
                    } else {
                        assert!(taskbar.connection.is_none(), "idle does not initialize COM");
                    }
                    // A failed epoch must not retry until a new readiness notification.
                    taskbar.failed = true;
                    taskbar
                        .set_progress(TaskbarProgress::Fraction(123))
                        .expect("suppressed failure");
                    assert_eq!(taskbar.applied, Some(TaskbarProgress::Hidden));
                }
                let handle = taskbar.handle;
                drop(taskbar);
                assert_eq!(Rc::strong_count(&state), 1);
                let external_filter =
                    MessageFilter::allow(handle, state.message).expect("filter reset after drop");
                assert!(external_filter.changed);
                let borrowed = NativeTaskbar::new(window.clone(), |_| {}).expect("existing filter");
                drop(borrowed);
                let unchanged =
                    MessageFilter::allow(handle, state.message).expect("preserved external filter");
                assert!(!unchanged.changed);
                drop(unchanged);
                drop(external_filter);
                // SAFETY: the surviving window belongs to this thread; handler was removed.
                unsafe { SendMessageW(handle, state.message, None, None) };
                assert_eq!(notifications.get(), 2);
                let mut replacement =
                    NativeTaskbar::new(window.clone(), |_| {}).expect("registration after drop");
                let destroyed = replacement.state.clone();
                // SAFETY: destroy only this fixture's same-thread HWND. This exercises
                // external native destruction before the retained Rust owners drop.
                unsafe { DestroyWindow(handle) }.expect("destroy owned window");
                assert!(!destroyed.alive.get());
                assert_eq!(Rc::strong_count(&destroyed), 2);
                replacement
                    .set_progress(TaskbarProgress::Fraction(500))
                    .expect("destroyed window ignored");
                drop(replacement);
                assert_eq!(Rc::strong_count(&destroyed), 1);
                event_loop.exit();
            }
            fn window_event(
                &mut self,
                _: &ActiveEventLoop,
                _: winit::window::WindowId,
                _: WindowEvent,
            ) {
            }
        }
        let event_loop = EventLoop::builder()
            .with_any_thread(true)
            .build()
            .expect("event loop");
        event_loop.run_app(&mut Trial(native)).expect("trial");
    }
}
