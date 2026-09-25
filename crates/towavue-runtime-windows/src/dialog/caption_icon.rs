//! Caption-only branding for native dialogs, including nested Shell prompts.
use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{
    DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetClassNameW, HCBT_ACTIVATE, HHOOK, HICON, ICON_BIG, ICON_SMALL, LoadIconW,
    SendMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_CBT, WM_NCDESTROY, WM_SETICON,
};
use windows::core::PCWSTR;

thread_local! {
    static CAPTION_ICON: Cell<HICON> = const { Cell::new(HICON(std::ptr::null_mut())) };
    static WINDOWS: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) };
}

const SUBCLASS_ID: usize = 0x7476_6469;

/// Installed only on a dedicated modal worker, through the whole synchronous
/// native operation. The thread-local hook also covers nested overwrite/error
/// dialogs. It neither observes other threads nor changes their window classes.
/// The icon is a shared module resource; neither dialogs nor this guard destroy it.
pub(super) struct DialogCaptionIcon {
    hook: HHOOK,
    previous: HICON,
    previous_windows: Vec<HWND>,
    // Removing a thread hook and restoring its context must use the creating thread.
    _thread: PhantomData<Rc<()>>,
}

impl DialogCaptionIcon {
    pub(super) fn new() -> Option<Self> {
        // SAFETY: resource 1 is embedded in the application module. LoadIconW
        // returns a shared icon with module lifetime; test hosts may lack it.
        let icon = unsafe {
            let module = GetModuleHandleW(None).ok()?;
            LoadIconW(Some(module.into()), PCWSTR(1 as _)).ok()?
        };
        Self::install(icon).ok()
    }

    fn install(icon: HICON) -> windows::core::Result<Self> {
        // SAFETY: this is a thread-local hook in this process, never a global
        // hook. Its callback code and the shared icon outlive its RAII guard.
        let hook =
            unsafe { SetWindowsHookExW(WH_CBT, Some(on_activate), None, GetCurrentThreadId())? };
        let previous = CAPTION_ICON.replace(icon);
        let previous_windows = WINDOWS.take();
        Ok(Self {
            hook,
            previous,
            previous_windows,
            _thread: PhantomData,
        })
    }
}

impl Drop for DialogCaptionIcon {
    fn drop(&mut self) {
        // SAFETY: the guard cannot leave its creating thread. Native modal calls
        // have returned before removal, including error and unwind paths.
        unsafe {
            let _ = UnhookWindowsHookEx(self.hook);
            for window in WINDOWS.take() {
                let _ = RemoveWindowSubclass(window, Some(caption_proc), SUBCLASS_ID);
            }
        }
        CAPTION_ICON.set(self.previous);
        WINDOWS.set(std::mem::take(&mut self.previous_windows));
    }
}

unsafe extern "system" fn on_activate(code: i32, parameter: WPARAM, data: LPARAM) -> LRESULT {
    // No Rust panic may cross the native callback. Always continue the hook
    // chain, including negative codes; this hook never cancels native operations.
    if code == HCBT_ACTIVATE as i32 {
        let _ = std::panic::catch_unwind(|| {
            let window = HWND(parameter.0 as *mut _);
            let icon = CAPTION_ICON.get();
            let mut class = [0u16; 32];
            // SAFETY: HCBT_ACTIVATE supplies a live HWND on this worker. The
            // class buffer is stack-owned; only native dialog windows qualify.
            let length = unsafe { GetClassNameW(window, &mut class) } as usize;
            if !icon.is_invalid() && class[..length].iter().copied().eq("#32770".encode_utf16()) {
                // SAFETY: Shell file dialogs replace caption icons after
                // activation. The same-thread subclass retains no borrowed data;
                // destruction or guard teardown removes it.
                unsafe {
                    if !GetWindowSubclass(window, Some(caption_proc), SUBCLASS_ID, None).as_bool()
                        && SetWindowSubclass(window, Some(caption_proc), SUBCLASS_ID, 0).as_bool()
                    {
                        WINDOWS.with_borrow_mut(|windows| windows.push(window));
                    }
                }
                for size in [ICON_SMALL, ICON_BIG] {
                    // SAFETY: the window is live and the shared icon outlives it.
                    // WM_SETICON affects the caption/Alt-Tab icon, not body icons.
                    unsafe {
                        SendMessageW(
                            window,
                            WM_SETICON,
                            Some(WPARAM(size as usize)),
                            Some(LPARAM(icon.0 as isize)),
                        );
                    }
                }
            }
        });
    }
    // SAFETY: forward the original hook notification without retaining pointers.
    unsafe { CallNextHookEx(None, code, parameter, data) }
}

unsafe extern "system" fn caption_proc(
    window: HWND,
    message: u32,
    parameter: WPARAM,
    data: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    let data = std::panic::catch_unwind(|| {
        if message == WM_NCDESTROY {
            // SAFETY: called on this window's creating thread before destruction.
            unsafe {
                let _ = RemoveWindowSubclass(window, Some(caption_proc), SUBCLASS_ID);
            }
        } else if message == WM_SETICON {
            let icon = CAPTION_ICON.get();
            if !icon.is_invalid() {
                return LPARAM(icon.0 as isize);
            }
        }
        data
    })
    .unwrap_or(data);
    // SAFETY: forward every message and retain native return/ownership semantics.
    // Only the shared caption icon replaces WM_SETICON's borrowed handle.
    unsafe { DefSubclassProc(window, message, parameter, data) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::core::w;

    #[test]
    fn caption_hook_limits_changes_to_dialogs_and_restores_thread_context() {
        std::thread::spawn(|| {
            // SAFETY: standard shared icons and owned hidden windows. All native
            // messages, hook lifetimes and window destruction stay on this thread.
            unsafe {
                let icon = LoadIconW(None, IDI_INFORMATION).expect("shared icon");
                let other = LoadIconW(None, IDI_WARNING).expect("second shared icon");
                assert!(CAPTION_ICON.get().is_invalid());
                let guard = DialogCaptionIcon::install(icon).expect("thread hook");
                {
                    let _nested = DialogCaptionIcon::install(other).expect("nested guard");
                    assert_eq!(CAPTION_ICON.get(), other);
                }
                assert_eq!(CAPTION_ICON.get(), icon);
                for (class, expected) in [(w!("#32770"), icon), (w!("STATIC"), HICON::default())] {
                    let window = CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        class,
                        w!("caption test"),
                        WS_CAPTION | WS_SYSMENU,
                        0,
                        0,
                        100,
                        100,
                        None,
                        None,
                        None,
                        None,
                    )
                    .expect("hidden native window");
                    on_activate(-1, WPARAM(window.0 as usize), LPARAM(0));
                    assert_eq!(
                        SendMessageW(window, WM_GETICON, Some(WPARAM(ICON_SMALL as usize)), None).0,
                        0
                    );
                    on_activate(HCBT_ACTIVATE as i32, WPARAM(window.0 as usize), LPARAM(0));
                    for size in [ICON_SMALL, ICON_BIG] {
                        assert_eq!(
                            SendMessageW(window, WM_GETICON, Some(WPARAM(size as usize)), None).0,
                            expected.0 as isize
                        );
                    }
                    DestroyWindow(window).expect("destroy owned test window");
                }
                let window = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("#32770"),
                    w!("teardown test"),
                    WS_CAPTION | WS_SYSMENU,
                    0,
                    0,
                    100,
                    100,
                    None,
                    None,
                    None,
                    None,
                )
                .expect("surviving owned window");
                on_activate(HCBT_ACTIVATE as i32, WPARAM(window.0 as usize), LPARAM(0));
                SendMessageW(
                    window,
                    WM_SETICON,
                    Some(WPARAM(ICON_SMALL as usize)),
                    Some(LPARAM(other.0 as isize)),
                );
                assert_eq!(
                    SendMessageW(window, WM_GETICON, Some(WPARAM(ICON_SMALL as usize)), None).0,
                    icon.0 as isize,
                    "later Shell icon replacement stays branded"
                );
                drop(guard);
                assert!(CAPTION_ICON.get().is_invalid());
                assert!(
                    !GetWindowSubclass(window, Some(caption_proc), SUBCLASS_ID, None).as_bool()
                );
                SendMessageW(
                    window,
                    WM_SETICON,
                    Some(WPARAM(ICON_SMALL as usize)),
                    Some(LPARAM(other.0 as isize)),
                );
                assert_eq!(
                    SendMessageW(window, WM_GETICON, Some(WPARAM(ICON_SMALL as usize)), None).0,
                    other.0 as isize,
                    "guard removal restores ordinary icon messages"
                );
                DestroyWindow(window).expect("destroy surviving window");
                let _ = std::panic::catch_unwind(|| {
                    let _guard = DialogCaptionIcon::install(icon).expect("unwind guard");
                    panic!("exercise icon-hook teardown");
                });
                assert!(CAPTION_ICON.get().is_invalid());
            }
        })
        .join()
        .expect("native worker");
    }

    thread_local! {
        static OBSERVATIONS: std::cell::RefCell<Vec<(isize, isize)>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    unsafe extern "system" fn observe_and_close(window: HWND, _: LPARAM) -> windows::core::BOOL {
        let _ = std::panic::catch_unwind(|| {
            let mut class = [0u16; 32];
            // SAFETY: enumeration supplies live windows on the owned test thread.
            unsafe {
                let length = GetClassNameW(window, &mut class) as usize;
                if class[..length].iter().copied().eq("#32770".encode_utf16())
                    && IsWindowVisible(window).as_bool()
                    && (GetWindowLongPtrW(window, GWL_STYLE) as u32 & WS_DISABLED.0 == 0)
                {
                    let small =
                        SendMessageW(window, WM_GETICON, Some(WPARAM(ICON_SMALL as usize)), None).0;
                    let large =
                        SendMessageW(window, WM_GETICON, Some(WPARAM(ICON_BIG as usize)), None).0;
                    OBSERVATIONS.with_borrow_mut(|values| values.push((small, large)));
                    let _ = PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
        });
        true.into()
    }

    unsafe extern "system" fn close_on_timer(_: HWND, _: u32, _: usize, _: u32) {
        // SAFETY: no global/window discovery; only this test thread's dialogs.
        unsafe {
            let _ = EnumThreadWindows(GetCurrentThreadId(), Some(observe_and_close), LPARAM(0));
        }
    }

    unsafe extern "system" fn nested_prompt(
        owner: HWND,
        notification: windows::Win32::UI::Controls::TASKDIALOG_NOTIFICATIONS,
        _: WPARAM,
        _: LPARAM,
        _: isize,
    ) -> windows::core::HRESULT {
        if notification == windows::Win32::UI::Controls::TDN_CREATED {
            // SAFETY: the TaskDialog retains this owner; its modal loop and the
            // nested MessageBox share the test thread's automatic cancel timer.
            unsafe {
                MessageBoxW(
                    Some(owner),
                    w!("Nested error check"),
                    w!("towavue"),
                    MB_OK | MB_ICONWARNING,
                );
            }
        }
        windows::core::HRESULT(0)
    }

    #[test]
    #[ignore = "opens and automatically cancels native dialogs; requires an interactive Windows desktop"]
    fn native_dialog_families_receive_caption_icons_before_automatic_cancel() {
        use crate::dialog::{
            DialogApartment, Language, SaveFilter, export_save, show_initialized_dialog,
            show_relocation_dialog,
        };
        use windows::Win32::System::Ole::OleInitialize;
        use windows::Win32::UI::Controls::{
            TASKDIALOGCONFIG, TDCBF_CANCEL_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION,
            TaskDialogIndirect,
        };

        std::thread::spawn(|| {
            // SAFETY: one owned STA, synchronous native modals and automatic
            // cancellation. No file is accepted, renamed, moved or written.
            unsafe {
                OleInitialize(None).expect("dialog STA");
                let _apartment = DialogApartment;
                let icon = LoadIconW(None, IDI_INFORMATION).expect("shared test icon");
                let _guard = DialogCaptionIcon::install(icon).expect("native icon hook");
                let timer = SetTimer(None, 0, 100, Some(close_on_timer));
                assert_ne!(timer, 0);
                for case in 0..8 {
                    OBSERVATIONS.with_borrow_mut(Vec::clear);
                    match case {
                        0 => {
                            assert_eq!(
                                MessageBoxW(
                                    None,
                                    w!("Automatic caption check"),
                                    w!("towavue"),
                                    MB_OK | MB_ICONWARNING
                                ),
                                IDOK
                            );
                        }
                        1 | 7 => {
                            let config = TASKDIALOGCONFIG {
                                cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
                                dwFlags: TDF_ALLOW_DIALOG_CANCELLATION,
                                dwCommonButtons: TDCBF_CANCEL_BUTTON,
                                pszWindowTitle: w!("towavue"),
                                pszContent: w!("Automatic caption check"),
                                pfCallback: (case == 7).then_some(nested_prompt),
                                ..Default::default()
                            };
                            let mut result = 0;
                            TaskDialogIndirect(&config, Some(&mut result), None, None)
                                .expect("TaskDialog");
                            assert_eq!(result, IDCANCEL.0);
                        }
                        2 | 3 => assert!(
                            show_initialized_dialog(Language::English, case == 3, HWND::default())
                                .expect("open dialog")
                                .is_none()
                        ),
                        4 | 5 => {
                            let source = std::env::temp_dir().join("towavue-caption-test.png");
                            assert!(
                                show_relocation_dialog(
                                    Language::English,
                                    &source,
                                    HWND::default(),
                                    case == 5
                                )
                                .expect("relocation dialog")
                                .is_none()
                            );
                        }
                        6 => assert!(
                            export_save::show_initialized_save_dialog(
                                Language::English,
                                "caption-test.png",
                                HWND::default(),
                                SaveFilter::Frame
                            )
                            .expect("save dialog")
                            .is_none()
                        ),
                        _ => unreachable!(),
                    }
                    OBSERVATIONS.with_borrow(|values| {
                        assert!(!values.is_empty(), "native case {case} was observed");
                        if case == 7 {
                            assert!(
                                values.len() >= 2,
                                "both nested and outer dialogs were observed"
                            );
                        }
                        assert!(
                            values
                                .iter()
                                .all(|value| *value == (icon.0 as isize, icon.0 as isize)),
                            "native case {case}: {values:?}"
                        );
                        eprintln!(
                            "Native caption case {case}: {} matching observations",
                            values.len()
                        );
                    });
                }
                KillTimer(None, timer).expect("remove test timer");
            }
        })
        .join()
        .expect("native dialog worker");
    }
}
