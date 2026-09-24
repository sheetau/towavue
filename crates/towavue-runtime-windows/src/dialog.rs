use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use thiserror::Error;
use towavue_core::localization::{Language, Text, formatted};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};
use windows::Win32::UI::Controls::{
    TASKDIALOG_BUTTON, TASKDIALOGCONFIG, TDF_ALLOW_DIALOG_CANCELLATION,
    TDF_POSITION_RELATIVE_TO_WINDOW, TaskDialogIndirect,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FILEOPENDIALOGOPTIONS, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT,
    FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog, FileSaveDialog, IFileOpenDialog,
    IFileSaveDialog, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetCursorPos, IDCANCEL, IDNO, IDOK, IDRETRY, IDYES, MB_DEFBUTTON2,
    MB_DEFBUTTON3, MB_ICONWARNING, MB_OK, MB_RETRYCANCEL, MB_YESNOCANCEL, MessageBoxW,
};
use windows::core::PCWSTR;
#[cfg(test)]
use windows::core::w;

const ERROR_CANCELLED_HRESULT: u32 = 0x8007_04c7;

mod about;
pub use about::{AboutEvent, AboutResponse, show_about};

#[derive(Debug, Error)]
pub enum DialogError {
    #[error("the file dialog's owner window is unavailable")]
    OwnerUnavailable,
    #[error("the file-dialog thread could not start: {0}")]
    Thread(#[from] std::io::Error),
    #[error("the file-dialog thread stopped unexpectedly")]
    ThreadStopped,
    #[error("Windows file dialog failed: {0}")]
    Windows(#[from] windows::core::Error),
    #[error("Windows returned an invalid UTF-16 path: {0}")]
    InvalidPath(#[from] std::string::FromUtf16Error),
    #[error("Could not prepare export formats: {0}")]
    Export(#[from] crate::ExportError),
    #[error("{0}")]
    InvalidExportChoice(String),
}

impl DialogError {
    /// App-owned explanation in the selected language; external OS/I/O errors
    /// retain their original text. Display keeps its existing diagnostic form.
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::OwnerUnavailable => Text::DialogOwnerUnavailable.in_language(language).into(),
            Self::Thread(error) => formatted::dialog_thread_failed(language, &error.to_string()),
            Self::ThreadStopped => Text::DialogThreadStopped.in_language(language).into(),
            Self::Windows(error) => formatted::dialog_windows_failed(language, &error.to_string()),
            Self::InvalidPath(error) => {
                formatted::dialog_invalid_path(language, &error.to_string())
            }
            Self::Export(error) => formatted::dialog_export_failed(language, &error.to_string()),
            Self::InvalidExportChoice(message) => message.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum FileDialogKind {
    OpenFile,
    OpenFolder,
    RenameFile {
        source: PathBuf,
    },
    MoveFile {
        source: PathBuf,
    },
    SaveExport {
        suggested_name: String,
        request: Box<crate::ExportDialogRequest>,
    },
    SaveFrame {
        suggested_name: String,
    },
}

/// Keeps the owner alive until its modal dialog closes, without blocking the caller.
pub fn pick_path(
    language: Language,
    owner: Arc<impl HasWindowHandle + Send + Sync + 'static>,
    kind: FileDialogKind,
    notify: impl FnOnce(Result<Option<PathBuf>, DialogError>) + Send + 'static,
) -> Result<(), DialogError> {
    let handle = owner
        .window_handle()
        .map_err(|_| DialogError::OwnerUnavailable)?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err(DialogError::OwnerUnavailable);
    };
    let native_owner = handle.hwnd.get();
    start_dialog_worker(
        move || {
            let _owner = owner;
            dialog_thread(language, native_owner, kind)
        },
        notify,
    )
}

/// Selects a Save as destination and retains its identity at native acceptance.
/// Only detached values cross the worker boundary; queued encoding must use this
/// snapshot rather than recapturing whatever later occupies the chosen path.
pub fn pick_save_as(
    language: Language,
    owner: Arc<impl HasWindowHandle + Send + Sync + 'static>,
    suggested_name: String,
    request: crate::ExportDialogRequest,
    notify: impl FnOnce(Result<Option<crate::SaveAsTarget>, DialogError>) + Send + 'static,
) -> Result<(), DialogError> {
    let handle = owner
        .window_handle()
        .map_err(|_| DialogError::OwnerUnavailable)?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err(DialogError::OwnerUnavailable);
    };
    let native_owner = handle.hwnd.get();
    start_dialog_worker(
        move || {
            let _owner = owner;
            if request.options.output != crate::ExportOutput::Media {
                return Err(DialogError::InvalidExportChoice(
                    Text::NativeSaveCompleteDocument
                        .in_language(language)
                        .into(),
                ));
            }
            // SAFETY: the worker retains its owner and all native interfaces on
            // this STA; the prepared dialog drops before the apartment guard.
            unsafe {
                OleInitialize(None)?;
                let _apartment = DialogApartment;
                let choices = request.choices(&std::sync::atomic::AtomicBool::new(false))?;
                export_save::show_initialized_save_as_dialog(
                    language,
                    &suggested_name,
                    HWND(native_owner as *mut _),
                    choices,
                )
            }
        },
        notify,
    )
}

#[derive(Clone, Copy)]
pub enum PromptButtons {
    Information,
    Ok,
    RetryCancel,
    YesNoCancel,
    SaveDiscardCancel { discard_all: bool },
    InstallUpdate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptResponse {
    Ok,
    Retry,
    Yes,
    No,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeleteConfirmation {
    pub confirmed: bool,
    pub dont_ask_again: bool,
}

fn delete_confirmation(button: i32, checked: bool) -> DeleteConfirmation {
    let confirmed = button == IDYES.0;
    DeleteConfirmation {
        confirmed,
        // Cancelling must never silently disable a future destructive-action prompt.
        dont_ask_again: confirmed && checked,
    }
}

/// Presents the application's recycle confirmation, with native verification UI.
/// Persistence is the caller's responsibility, after a confirmed response only.
pub fn confirm_file_delete(
    language: Language,
    owner: Arc<impl HasWindowHandle + Send + Sync + 'static>,
    source: PathBuf,
    unsaved_edits: bool,
    notify: impl FnOnce(Result<DeleteConfirmation, DialogError>) + Send + 'static,
) -> Result<(), DialogError> {
    let handle = owner
        .window_handle()
        .map_err(|_| DialogError::OwnerUnavailable)?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err(DialogError::OwnerUnavailable);
    };
    let native_owner = handle.hwnd.get();
    start_dialog_worker(
        move || {
            let _owner = owner;
            let retained = if unsaved_edits {
                Text::NativeDeleteRetained.in_language(language)
            } else {
                ""
            };
            let message = wide(&formatted::native_delete(
                language,
                &source.display().to_string(),
                retained,
            ));
            let title = wide(Text::NativeDeleteTitle.in_language(language));
            let instruction = wide(Text::NativeDeleteInstruction.in_language(language));
            let verification = wide(
                if unsaved_edits {
                    Text::NativeDeleteCleanSuppression
                } else {
                    Text::NativeDeleteSuppression
                }
                .in_language(language),
            );
            // SAFETY: the worker owns this STA and all modal buffers until the
            // native dialog returns. Only plain response values cross threads.
            unsafe { OleInitialize(None) }?;
            let _apartment = DialogApartment;
            let labels = button_labels(
                language,
                [
                    (IDYES.0, Text::CommandDeleteFile),
                    (IDCANCEL.0, Text::Cancel),
                ],
            );
            let buttons = labels.each_ref().map(button_view);
            let config = TASKDIALOGCONFIG {
                cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
                hwndParent: HWND(native_owner as *mut _),
                dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_POSITION_RELATIVE_TO_WINDOW,
                pszWindowTitle: PCWSTR(title.as_ptr()),
                pszMainInstruction: PCWSTR(instruction.as_ptr()),
                pszContent: PCWSTR(message.as_ptr()),
                pszVerificationText: PCWSTR(verification.as_ptr()),
                cButtons: buttons.len() as u32,
                pButtons: buttons.as_ptr(),
                nDefaultButton: IDCANCEL.0,
                ..Default::default()
            };
            let mut button = IDCANCEL.0;
            let mut checked = windows::core::BOOL::default();
            // SAFETY: retained owner, strings, buttons and writable results all
            // outlive this synchronous call; the HWND is never exposed to the app.
            unsafe { TaskDialogIndirect(&config, Some(&mut button), None, Some(&mut checked)) }?;
            Ok(delete_confirmation(button, checked.as_bool()))
        },
        notify,
    )
}

/// Shows a GPU-independent modal prompt while retaining its owner on the worker.
pub fn show_prompt(
    language: Language,
    owner: Arc<impl HasWindowHandle + Send + Sync + 'static>,
    message: String,
    buttons: PromptButtons,
    notify: impl FnOnce(Result<PromptResponse, DialogError>) + Send + 'static,
) -> Result<(), DialogError> {
    let handle = owner
        .window_handle()
        .map_err(|_| DialogError::OwnerUnavailable)?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err(DialogError::OwnerUnavailable);
    };
    let native_owner = handle.hwnd.get();
    start_dialog_worker(
        move || {
            let _owner = owner;
            let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
            if matches!(
                buttons,
                PromptButtons::SaveDiscardCancel { .. } | PromptButtons::InstallUpdate
            ) {
                // SAFETY: this new worker owns its STA through the modal and releases it
                // on the same thread, including errors. Native accessibility uses COM.
                unsafe { OleInitialize(None) }?;
                let _apartment = DialogApartment;
                let (labels, title, instruction, default_button) = match buttons {
                    PromptButtons::SaveDiscardCancel { discard_all } => (
                        unsaved_prompt_buttons(language, discard_all).to_vec(),
                        Text::NativeUnsavedTitle,
                        Text::NativeUnsavedInstruction,
                        IDCANCEL.0,
                    ),
                    PromptButtons::InstallUpdate => (
                        update_prompt_buttons(language).to_vec(),
                        Text::NativeUpdateTitle,
                        Text::NativeUpdateInstruction,
                        IDNO.0,
                    ),
                    _ => unreachable!("task dialog buttons checked above"),
                };
                let choices: Vec<_> = labels.iter().map(button_view).collect();
                let title = wide(title.in_language(language));
                let instruction = wide(instruction.in_language(language));
                let config = TASKDIALOGCONFIG {
                    cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
                    hwndParent: HWND(native_owner as *mut _),
                    dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_POSITION_RELATIVE_TO_WINDOW,
                    pszWindowTitle: PCWSTR(title.as_ptr()),
                    pszMainInstruction: PCWSTR(instruction.as_ptr()),
                    pszContent: PCWSTR(message.as_ptr()),
                    cButtons: choices.len() as u32,
                    pButtons: choices.as_ptr(),
                    nDefaultButton: default_button,
                    ..Default::default()
                };
                let mut result = IDCANCEL.0;
                // SAFETY: the worker retains the owner, text, buttons and config until the
                // synchronous native modal returns. Only the result crosses to the app thread;
                // no native/COM/graphics resources or borrowed pointers leave this call.
                unsafe { TaskDialogIndirect(&config, Some(&mut result), None, None) }?;
                return Ok(match result {
                    value if value == IDYES.0 => PromptResponse::Yes,
                    value if value == IDNO.0 => PromptResponse::No,
                    _ => PromptResponse::Cancel,
                });
            }
            let flags = match buttons {
                PromptButtons::Information => MB_OK,
                PromptButtons::Ok => MB_OK,
                PromptButtons::RetryCancel => MB_RETRYCANCEL | MB_DEFBUTTON2,
                PromptButtons::YesNoCancel => MB_YESNOCANCEL | MB_DEFBUTTON3,
                PromptButtons::SaveDiscardCancel { .. } | PromptButtons::InstallUpdate => {
                    unreachable!("handled above")
                }
            } | if matches!(buttons, PromptButtons::Information) {
                windows::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION
            } else {
                MB_ICONWARNING
            };
            // The worker retains the HWND owner and UTF-16 buffer for the modal call.
            // MessageBox owns its native UI; no COM or graphics resources cross threads.
            let result = unsafe {
                MessageBoxW(
                    Some(HWND(native_owner as *mut _)),
                    PCWSTR(message.as_ptr()),
                    windows::core::w!("towavue"),
                    flags,
                )
            };
            if result.0 == 0 {
                return Err(windows::core::Error::from_thread().into());
            }
            Ok(match result {
                IDOK => PromptResponse::Ok,
                IDRETRY => PromptResponse::Retry,
                IDYES => PromptResponse::Yes,
                IDNO => PromptResponse::No,
                _ => PromptResponse::Cancel,
            })
        },
        notify,
    )
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

// UTF-16 storage is owned by the modal worker. Construct native pointer views
// only after these buffers exist, and retain them through TaskDialogIndirect.
fn button_labels<const N: usize>(
    language: Language,
    rows: [(i32, Text); N],
) -> [(i32, Vec<u16>); N] {
    rows.map(|(id, key)| (id, wide(key.in_language(language))))
}

fn button_view(label: &(i32, Vec<u16>)) -> TASKDIALOG_BUTTON {
    TASKDIALOG_BUTTON {
        nButtonID: label.0,
        pszButtonText: PCWSTR(label.1.as_ptr()),
    }
}

fn update_prompt_buttons(language: Language) -> [(i32, Vec<u16>); 2] {
    button_labels(
        language,
        [
            (IDYES.0, Text::NativeInstallNow),
            (IDNO.0, Text::NativeInstallNext),
        ],
    )
}

fn unsaved_prompt_buttons(language: Language, discard_all: bool) -> [(i32, Vec<u16>); 3] {
    button_labels(
        language,
        [
            (IDYES.0, Text::NativeSaveContinue),
            (
                IDNO.0,
                if discard_all {
                    Text::NativeDiscardAll
                } else {
                    Text::NativeDiscard
                },
            ),
            (IDCANCEL.0, Text::Cancel),
        ],
    )
}

fn start_dialog_worker<T: Send + 'static>(
    choose: impl FnOnce() -> Result<T, DialogError> + Send + 'static,
    notify: impl FnOnce(Result<T, DialogError>) + Send + 'static,
) -> Result<(), DialogError> {
    thread::Builder::new()
        .name("towavue-file-dialog-sta".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(choose))
                .unwrap_or(Err(DialogError::ThreadStopped));
            notify(result);
        })?;
    Ok(())
}

/// Reads the current client-space pointer after a native dialog returns focus to its owner.
pub fn cursor_position_in_window(owner: &impl HasWindowHandle) -> Option<(i32, i32)> {
    let handle = owner.window_handle().ok()?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return None;
    };
    let window = HWND(handle.hwnd.get() as *mut _);
    let mut point = POINT::default();
    let mut client = RECT::default();
    // SAFETY: the borrowed owner remains alive throughout these read-only calls. The writable
    // POINT/RECT live on this thread's stack; no pointer or window handle escapes.
    unsafe {
        GetCursorPos(&mut point).ok()?;
        if !ScreenToClient(window, &mut point).as_bool() {
            return None;
        }
        GetClientRect(window, &mut client).ok()?;
    }
    (point.x >= client.left
        && point.x < client.right
        && point.y >= client.top
        && point.y < client.bottom)
        .then_some((point.x, point.y))
}

struct DialogApartment;

impl Drop for DialogApartment {
    fn drop(&mut self) {
        // SAFETY: created only after successful OleInitialize, and dropped on that same STA.
        unsafe { OleUninitialize() };
    }
}

fn dialog_thread(
    language: Language,
    native_owner: isize,
    kind: FileDialogKind,
) -> Result<Option<PathBuf>, DialogError> {
    let owner_handle = HWND(native_owner as *mut _);
    // SAFETY: the worker retains the owner for the entire Show call. All COM interfaces stay
    // on this initialized STA and drop before the apartment guard, including on unwind.
    unsafe {
        OleInitialize(None)?;
        let _apartment = DialogApartment;
        match kind {
            FileDialogKind::OpenFile => show_initialized_dialog(language, false, owner_handle),
            FileDialogKind::RenameFile { source } => {
                show_relocation_dialog(language, &source, owner_handle, false)
            }
            FileDialogKind::MoveFile { source } => {
                show_relocation_dialog(language, &source, owner_handle, true)
            }
            FileDialogKind::OpenFolder => show_initialized_dialog(language, true, owner_handle),
            FileDialogKind::SaveExport {
                suggested_name,
                request,
            } => {
                let audio_only = request.options.output == crate::ExportOutput::AudioOnly;
                let choices = request.choices(&std::sync::atomic::AtomicBool::new(false))?;
                show_initialized_save_dialog(
                    language,
                    &suggested_name,
                    owner_handle,
                    SaveFilter::Media {
                        choices,
                        audio_only,
                    },
                )
            }
            FileDialogKind::SaveFrame { suggested_name } => show_initialized_save_dialog(
                language,
                &suggested_name,
                owner_handle,
                SaveFilter::Frame,
            ),
        }
    }
}

unsafe fn show_relocation_dialog(
    language: Language,
    source: &std::path::Path,
    owner: HWND,
    moving: bool,
) -> Result<Option<PathBuf>, DialogError> {
    use windows::Win32::UI::Shell::{IFileDialog, IShellItem, SHCreateItemFromParsingName};
    use windows::core::Interface;
    // Caller retains the worker STA and owner for all interfaces and path buffers.
    unsafe {
        let dialog: IFileDialog = if moving {
            let open: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL)?;
            open.SetOptions(FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_PICKFOLDERS)?;
            open.cast()?
        } else {
            let save: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_ALL)?;
            save.SetOptions(FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST)?;
            let name: Vec<u16> = source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .encode_utf16()
                .chain(Some(0))
                .collect();
            save.SetFileName(PCWSTR(name.as_ptr()))?;
            save.cast()?
        };
        let title = wide(
            if moving {
                Text::NativeMoveTitle
            } else {
                Text::NativeRenameTitle
            }
            .in_language(language),
        );
        let accept = wide(
            if moving {
                Text::NativeMoveHere
            } else {
                Text::NativeRename
            }
            .in_language(language),
        );
        dialog.SetTitle(PCWSTR(title.as_ptr()))?;
        dialog.SetOkButtonLabel(PCWSTR(accept.as_ptr()))?;
        if let Some(parent) = source.parent() {
            use std::os::windows::ffi::OsStrExt;
            let wide: Vec<u16> = parent.as_os_str().encode_wide().chain(Some(0)).collect();
            let folder: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)?;
            dialog.SetFolder(&folder)?;
        }
        if let Err(error) = dialog.Show(Some(owner)) {
            if error.code().0 as u32 == ERROR_CANCELLED_HRESULT {
                return Ok(None);
            }
            return Err(error.into());
        }
        let item = dialog.GetResult()?;
        let value = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = value.to_string().map(PathBuf::from);
        CoTaskMemFree(Some(value.0.cast()));
        Ok(Some(path?))
    }
}

unsafe fn show_initialized_dialog(
    language: Language,
    folder: bool,
    owner: HWND,
) -> Result<Option<PathBuf>, DialogError> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL)?;
        let mut options: FILEOPENDIALOGOPTIONS =
            FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_FILEMUSTEXIST;
        if folder {
            options |= FOS_PICKFOLDERS;
        }
        dialog.SetOptions(options)?;
        if language == Language::Japanese {
            let title = wide(
                if folder {
                    Text::CommandOpenFolder
                } else {
                    Text::CommandOpenFile
                }
                .in_language(language),
            );
            dialog.SetTitle(PCWSTR(title.as_ptr()))?;
        }
        if let Err(error) = dialog.Show(Some(owner)) {
            if error.code().0 as u32 == ERROR_CANCELLED_HRESULT {
                return Ok(None);
            }
            return Err(error.into());
        }
        let item = dialog.GetResult()?;
        let value = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = value.to_string().map(PathBuf::from);
        CoTaskMemFree(Some(value.0.cast()));
        Ok(Some(path?))
    }
}

#[path = "dialog_export.rs"]
mod export_save;
#[cfg(test)]
use export_save::prepare_save_dialog;
use export_save::{SaveFilter, show_initialized_save_dialog};

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn dialog_errors_keep_english_diagnostics_and_localize_owned_explanations() {
        let errors = [
            DialogError::OwnerUnavailable,
            DialogError::Thread(std::io::Error::other("external {thread}")),
            DialogError::ThreadStopped,
            DialogError::Windows(windows::core::Error::from_hresult(windows::core::HRESULT(
                0x80004005_u32 as i32,
            ))),
            DialogError::InvalidPath(
                String::from_utf16(&[0xd800]).expect_err("unpaired surrogate"),
            ),
            DialogError::Export(crate::ExportError::Failed("external {export}".into())),
            DialogError::InvalidExportChoice("owned translated choice".into()),
        ];
        for error in &errors {
            assert_eq!(error.message(Language::English), error.to_string());
        }
        assert_eq!(
            errors[0].message(Language::Japanese),
            "ダイアログの親ウィンドウを利用できません"
        );
        assert!(
            errors[1]
                .message(Language::Japanese)
                .ends_with("external {thread}")
        );
        assert!(errors[3].message(Language::Japanese).contains("0x80004005"));
        assert!(
            errors[5]
                .message(Language::Japanese)
                .ends_with("external {export}")
        );
        assert_eq!(
            errors[6].message(Language::Japanese),
            "owned translated choice"
        );
    }

    #[test]
    fn japanese_native_buttons_keep_response_ids_and_owned_utf16_text() {
        let labels = Box::new(update_prompt_buttons(Language::Japanese));
        let buttons = labels.each_ref().map(button_view);
        assert_eq!(buttons.map(|button| button.nButtonID), [IDYES.0, IDNO.0]);
        assert_eq!(
            buttons.map(|button| {
                let ptr = button.pszButtonText;
                // SAFETY: the owning buffers remain alive for every pointer read.
                unsafe { ptr.to_string() }.expect("owned UTF-16")
            }),
            ["今すぐインストール", "次回起動時にインストール"]
        );
        for all in [false, true] {
            let labels = unsaved_prompt_buttons(Language::Japanese, all);
            let buttons = labels.each_ref().map(button_view);
            assert_eq!(
                buttons.map(|button| button.nButtonID),
                [IDYES.0, IDNO.0, IDCANCEL.0]
            );
            assert_eq!(
                buttons.map(|button| {
                    let ptr = button.pszButtonText;
                    // SAFETY: label owners outlive the native pointer views.
                    unsafe { ptr.to_string() }.expect("owned UTF-16")
                }),
                [
                    "保存して続行",
                    if all {
                        "すべての編集を破棄して終了"
                    } else {
                        "編集を破棄"
                    },
                    "キャンセル"
                ]
            );
        }
        let labels = button_labels(
            Language::Japanese,
            [
                (IDYES.0, Text::CommandDeleteFile),
                (IDCANCEL.0, Text::Cancel),
            ],
        );
        assert_eq!(
            labels
                .each_ref()
                .map(button_view)
                .map(|button| button.nButtonID),
            [IDYES.0, IDCANCEL.0]
        );
        let path = "C:\\日本語 {source}.png";
        let message = formatted::native_delete(
            Language::Japanese,
            path,
            Text::NativeDeleteRetained.in_language(Language::Japanese),
        );
        assert!(
            message.starts_with(path)
                && message.contains("ごみ箱")
                && message.contains("編集内容は保持")
        );
    }

    #[test]
    fn cancelled_delete_never_disables_future_confirmation() {
        for checked in [false, true] {
            for button in [IDCANCEL.0, IDNO.0, 0] {
                assert_eq!(
                    delete_confirmation(button, checked),
                    DeleteConfirmation {
                        confirmed: false,
                        dont_ask_again: false,
                    }
                );
            }
            assert_eq!(
                delete_confirmation(IDYES.0, checked),
                DeleteConfirmation {
                    confirmed: true,
                    dont_ask_again: checked,
                }
            );
        }
    }

    #[test]
    fn update_prompt_keeps_the_two_requested_native_actions() {
        let labels = update_prompt_buttons(Language::English);
        let buttons = labels.each_ref().map(button_view);
        assert_eq!(buttons.map(|button| button.nButtonID), [IDYES.0, IDNO.0]);
        assert_eq!(
            buttons.map(|button| {
                let label = button.pszButtonText;
                // SAFETY: the dialog buttons retain owned NUL-terminated labels for this scope.
                unsafe { label.to_string() }.expect("static label")
            }),
            ["Install now", "Install on next launch"]
        );
    }

    #[test]
    fn unsaved_prompt_buttons_use_explicit_actions_and_exit_scope() {
        for all in [false, true] {
            let labels = unsaved_prompt_buttons(Language::English, all);
            let buttons = labels.each_ref().map(button_view);
            assert_eq!(
                buttons.map(|button| button.nButtonID),
                [IDYES.0, IDNO.0, IDCANCEL.0]
            );
            assert_eq!(
                buttons.map(|button| {
                    let label = button.pszButtonText;
                    // SAFETY: the owning label buffers remain alive through these reads.
                    unsafe { label.to_string() }.expect("static label")
                }),
                [
                    "Save and continue",
                    if all {
                        "Discard all edits and exit"
                    } else {
                        "Discard edits"
                    },
                    "Cancel",
                ]
            );
        }
    }

    #[test]
    fn audio_save_dialog_accepts_seven_types_unicode_name_and_preserves_overwrite_prompt_without_showing()
     {
        thread::spawn(|| {
            // This owned worker initializes an STA, never calls Show or changes focus,
            // frees the returned filename and drops every interface before the apartment.
            unsafe {
                OleInitialize(None).expect("STA");
                let _apartment = DialogApartment;
                let dialog = prepare_save_dialog(
                    Language::English,
                    "動画.final-audio.wav",
                    SaveFilter::Media {
                        choices: crate::export::formats::Choices::new(
                            crate::export::formats::AUDIO_FORMATS.to_vec(),
                            std::path::Path::new("audio.wav"),
                        )
                        .expect("audio choices"),
                        audio_only: true,
                    },
                )
                .expect("audio save dialog");
                let options = dialog.GetOptions().expect("options");
                assert_eq!(options & FOS_OVERWRITEPROMPT, FOS_OVERWRITEPROMPT);
                assert_eq!(options & FOS_PATHMUSTEXIST, FOS_PATHMUSTEXIST);
                assert_eq!(dialog.GetFileTypeIndex().expect("default type"), 1);
                for index in 1..=crate::export::formats::AUDIO_FORMATS.len() as u32 {
                    dialog.SetFileTypeIndex(index).expect("supported type");
                    assert_eq!(dialog.GetFileTypeIndex().expect("selected type"), index);
                }
                let name = dialog.GetFileName().expect("suggested name");
                let text = name.to_string();
                CoTaskMemFree(Some(name.0.cast()));
                assert_eq!(text.expect("UTF-16"), "動画.final-audio.wav");
                let regular = prepare_save_dialog(
                    Language::English,
                    "image-export.png",
                    SaveFilter::Media {
                        choices: crate::export::formats::Choices::new(
                            vec![crate::export::formats::Format::Png],
                            std::path::Path::new("image.png"),
                        )
                        .expect("image choices"),
                        audio_only: false,
                    },
                )
                .expect("regular save dialog");
                assert_eq!(
                    regular.GetOptions().expect("regular options") & FOS_OVERWRITEPROMPT,
                    FOS_OVERWRITEPROMPT
                );
                let frame = prepare_save_dialog(
                    Language::English,
                    "video-frame-0ns.png",
                    SaveFilter::Frame,
                )
                .expect("frame save dialog");
                assert_eq!(frame.GetFileTypeIndex().expect("PNG type"), 1);
                assert_eq!(
                    frame.GetOptions().expect("frame options") & FOS_OVERWRITEPROMPT,
                    FOS_OVERWRITEPROMPT
                );
                let name = frame.GetFileName().expect("frame name");
                let text = name.to_string();
                CoTaskMemFree(Some(name.0.cast()));
                assert_eq!(text.expect("frame UTF-16"), "video-frame-0ns.png");
            }
        })
        .join()
        .expect("owned dialog worker");
    }

    #[test]
    fn dialog_worker_returns_without_waiting_and_delivers_choice_or_cancel_once() {
        for expected in [None, Some(PathBuf::from("selected.png"))] {
            let (release_tx, release_rx) = mpsc::channel();
            let (result_tx, result_rx) = mpsc::channel();
            let choice = expected.clone();
            start_dialog_worker(
                move || {
                    release_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("release dialog");
                    Ok(choice)
                },
                move |result| result_tx.send(result).expect("deliver result"),
            )
            .expect("start dialog worker");
            assert!(result_rx.try_recv().is_err());
            release_tx
                .send(())
                .expect("caller continued while dialog waited");
            assert_eq!(
                result_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("dialog result")
                    .expect("successful dialog"),
                expected
            );
            assert!(result_rx.recv_timeout(Duration::from_secs(1)).is_err());
        }
    }

    #[test]
    fn dialog_failure_and_worker_panic_are_reported_to_the_caller() {
        for panic in [false, true] {
            let (result_tx, result_rx) = mpsc::channel();
            start_dialog_worker(
                move || {
                    assert!(!panic, "simulated dialog worker panic");
                    Err::<Option<PathBuf>, _>(DialogError::OwnerUnavailable)
                },
                move |result| result_tx.send(result).expect("deliver error"),
            )
            .expect("start dialog worker");
            let error = result_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("failure result");
            if panic {
                assert!(matches!(error, Err(DialogError::ThreadStopped)));
            } else {
                assert!(matches!(error, Err(DialogError::OwnerUnavailable)));
            }
        }
    }
}
