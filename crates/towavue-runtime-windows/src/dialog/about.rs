use super::*;
use crate::ProjectLink;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    TASKDIALOG_NOTIFICATIONS, TASKDIALOGCONFIG_0, TDF_ENABLE_HYPERLINKS, TDF_USE_HICON_MAIN,
    TDN_CREATED, TDN_HYPERLINK_CLICKED,
};
use windows::Win32::UI::WindowsAndMessaging::{
    HICON, ICON_BIG, ICON_SMALL, LoadIconW, SendMessageW, WM_SETICON,
};
use windows::core::HRESULT;

const LICENSES: i32 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AboutResponse {
    Close,
    Licenses,
}

pub enum AboutEvent {
    Link(ProjectLink),
    Closed(Result<AboutResponse, DialogError>),
}

/// Owns a native About dialog on the retained owner's STA worker. Links are
/// fixed identifiers; only value events cross back to the application's thread.
pub fn show_about(
    language: Language,
    owner: Arc<impl HasWindowHandle + Send + Sync + 'static>,
    version: &'static str,
    license: &'static str,
    notify: impl Fn(AboutEvent) + Send + Sync + 'static,
) -> Result<(), DialogError> {
    let handle = owner
        .window_handle()
        .map_err(|_| DialogError::OwnerUnavailable)?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err(DialogError::OwnerUnavailable);
    };
    let native_owner = handle.hwnd.get();
    let notify = Arc::new(notify);
    let completed = Arc::clone(&notify);
    start_dialog_worker(
        move || {
            let _owner = owner;
            // SAFETY: this worker owns its apartment, buffers and callback context
            // through the synchronous dialog. All are dropped before COM teardown.
            unsafe { OleInitialize(None) }?;
            let _apartment = DialogApartment;
            let title = wide(&formatted::native_about_title(language, version));
            let content = wide(&formatted::native_about_content(language, license));
            let window_title = wide(Text::CommandAbout.in_language(language));
            // SAFETY: resource 1 is the app's embedded icon. LoadIcon returns a
            // shared module resource, not an owned icon to destroy. Test hosts may
            // have no resource; omitting the icon must not prevent opening About.
            let icon = unsafe {
                GetModuleHandleW(None)
                    .ok()
                    .and_then(|module| LoadIconW(Some(module.into()), PCWSTR(1 as _)).ok())
                    .unwrap_or_default()
            };
            let callback = Callback {
                notify: notify.as_ref(),
                icon,
            };
            let labels = button_labels(
                language,
                [(LICENSES, Text::NativeLicenses), (IDOK.0, Text::NativeOk)],
            );
            let buttons = labels.each_ref().map(button_view);
            let config = TASKDIALOGCONFIG {
                cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
                hwndParent: HWND(native_owner as *mut _),
                dwFlags: TDF_ALLOW_DIALOG_CANCELLATION
                    | TDF_POSITION_RELATIVE_TO_WINDOW
                    | TDF_ENABLE_HYPERLINKS
                    | TDF_USE_HICON_MAIN,
                pszWindowTitle: PCWSTR(window_title.as_ptr()),
                pszMainInstruction: PCWSTR(title.as_ptr()),
                pszContent: PCWSTR(content.as_ptr()),
                Anonymous1: TASKDIALOGCONFIG_0 { hMainIcon: icon },
                cButtons: buttons.len() as u32,
                pButtons: buttons.as_ptr(),
                nDefaultButton: IDOK.0,
                pfCallback: Some(dialog_callback),
                lpCallbackData: &callback as *const Callback<'_> as isize,
                ..Default::default()
            };
            let mut button = IDCANCEL.0;
            // SAFETY: the retained owner and stack-owned strings/buttons/context
            // remain valid until the modal returns, including its final callbacks.
            unsafe { TaskDialogIndirect(&config, Some(&mut button), None, None) }?;
            Ok(response(button))
        },
        move |result| completed(AboutEvent::Closed(result)),
    )
}

struct Callback<'a> {
    notify: &'a dyn Fn(AboutEvent),
    icon: HICON,
}

unsafe extern "system" fn dialog_callback(
    window: HWND,
    notification: TASKDIALOG_NOTIFICATIONS,
    _: WPARAM,
    parameter: LPARAM,
    context: isize,
) -> HRESULT {
    // Never unwind into comctl32. Context and hyperlink strings are borrowed only
    // during the synchronous callback on the dialog's STA, never retained.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: TaskDialog receives this context from the live stack above.
        let context = unsafe { &*(context as *const Callback<'_>) };
        if notification == TDN_CREATED && !context.icon.is_invalid() {
            // SAFETY: callback HWND is live and the shared icon outlives the dialog.
            unsafe {
                for size in [ICON_SMALL, ICON_BIG] {
                    SendMessageW(
                        window,
                        WM_SETICON,
                        Some(WPARAM(size as usize)),
                        Some(LPARAM(context.icon.0 as isize)),
                    );
                }
            }
        } else if notification == TDN_HYPERLINK_CLICKED && parameter.0 != 0 {
            let pointer = PCWSTR(parameter.0 as *const u16);
            // SAFETY: this notification supplies a NUL-terminated UTF-16 href.
            let href = unsafe { pointer.as_wide() };
            if let Some(link) = project_link(href) {
                (context.notify)(AboutEvent::Link(link));
            }
        }
    }));
    HRESULT(0)
}

fn project_link(href: &[u16]) -> Option<ProjectLink> {
    if href.iter().copied().eq("author".encode_utf16()) {
        Some(ProjectLink::Author)
    } else if href.iter().copied().eq("repository".encode_utf16()) {
        Some(ProjectLink::Repository)
    } else {
        None
    }
}

fn response(button: i32) -> AboutResponse {
    if button == LICENSES {
        AboutResponse::Licenses
    } else {
        AboutResponse::Close
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn japanese_about_text_preserves_version_license_and_fixed_link_targets() {
        let title = formatted::native_about_title(Language::Japanese, "1.0.3");
        assert_eq!(title, "towavue / バージョン 1.0.3");
        let content = formatted::native_about_content(Language::Japanese, "Apache-2.0");
        assert!(content.contains("Apache-2.0") && content.contains("Windows用メディアビューアー"));
        for href in ["author", "repository"] {
            assert!(content.contains(&format!("href=\"{href}\"")));
            assert!(project_link(&href.encode_utf16().collect::<Vec<_>>()).is_some());
        }
    }

    #[test]
    fn about_callback_routes_fixed_links_and_contains_panics() {
        use std::sync::Mutex;
        let received = Mutex::new(Vec::new());
        let notify = |event| {
            if let AboutEvent::Link(link) = event {
                received.lock().expect("links").push(link);
            }
        };
        let callback = Callback {
            notify: &notify,
            icon: HICON::default(),
        };
        for href in [
            "author",
            "repository",
            "https://example.com",
            "file:///C:/",
            "",
        ] {
            let text: Vec<_> = href.encode_utf16().chain(Some(0)).collect();
            // SAFETY: the owned callback/text buffers remain alive for the call.
            let result = unsafe {
                dialog_callback(
                    HWND::default(),
                    TDN_HYPERLINK_CLICKED,
                    WPARAM(0),
                    LPARAM(text.as_ptr() as isize),
                    &callback as *const _ as isize,
                )
            };
            assert!(result.is_ok());
        }
        assert!(*received.lock().expect("links") == [ProjectLink::Author, ProjectLink::Repository]);
        let notify = |_| panic!("callback failure");
        let callback = Callback {
            notify: &notify,
            icon: HICON::default(),
        };
        // SAFETY: live context and static NUL-terminated href; no HWND is used.
        assert!(
            unsafe {
                dialog_callback(
                    HWND::default(),
                    TDN_HYPERLINK_CLICKED,
                    WPARAM(0),
                    LPARAM(w!("author").as_ptr() as isize),
                    &callback as *const _ as isize,
                )
            }
            .is_ok()
        );
        for button in [IDOK.0, IDCANCEL.0, 0, -1] {
            assert_eq!(response(button), AboutResponse::Close);
        }
        assert_eq!(response(LICENSES), AboutResponse::Licenses);
    }
}
