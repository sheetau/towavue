use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use thiserror::Error;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FILEOPENDIALOGOPTIONS, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT,
    FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog, FileSaveDialog, IFileOpenDialog,
    IFileSaveDialog, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetCursorPos, IDNO, IDOK, IDRETRY, IDYES, MB_DEFBUTTON2, MB_DEFBUTTON3,
    MB_ICONWARNING, MB_OK, MB_RETRYCANCEL, MB_YESNOCANCEL, MessageBoxW,
};
use windows::core::{PCWSTR, w};

const ERROR_CANCELLED_HRESULT: u32 = 0x8007_04c7;

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
}

#[derive(Clone, Debug)]
pub enum FileDialogKind {
    OpenFile,
    OpenFolder,
    SaveFile { suggested_name: String },
    SaveAudio { suggested_name: String },
}

/// Keeps the owner alive until its modal dialog closes, without blocking the caller.
pub fn pick_path(
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
            dialog_thread(native_owner, kind)
        },
        notify,
    )
}

#[derive(Clone, Copy)]
pub enum PromptButtons {
    Ok,
    RetryCancel,
    YesNoCancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptResponse {
    Ok,
    Retry,
    Yes,
    No,
    Cancel,
}

/// Shows a GPU-independent modal prompt while retaining its owner on the worker.
pub fn show_prompt(
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
            let flags = match buttons {
                PromptButtons::Ok => MB_OK,
                PromptButtons::RetryCancel => MB_RETRYCANCEL | MB_DEFBUTTON2,
                PromptButtons::YesNoCancel => MB_YESNOCANCEL | MB_DEFBUTTON3,
            } | MB_ICONWARNING;
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
            FileDialogKind::OpenFile => show_initialized_dialog(false, owner_handle),
            FileDialogKind::OpenFolder => show_initialized_dialog(true, owner_handle),
            FileDialogKind::SaveFile { suggested_name } => {
                show_initialized_save_dialog(&suggested_name, owner_handle, false)
            }
            FileDialogKind::SaveAudio { suggested_name } => {
                show_initialized_save_dialog(&suggested_name, owner_handle, true)
            }
        }
    }
}

unsafe fn show_initialized_dialog(
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

unsafe fn show_initialized_save_dialog(
    suggested_name: &str,
    owner: HWND,
    audio_only: bool,
) -> Result<Option<PathBuf>, DialogError> {
    // The caller owns the STA until this function returns; interfaces never cross threads.
    unsafe {
        let dialog = prepare_save_dialog(suggested_name, audio_only)?;
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

// The caller keeps its initialized STA alive and drops the returned interface on that thread.
unsafe fn prepare_save_dialog(
    suggested_name: &str,
    audio_only: bool,
) -> Result<IFileSaveDialog, DialogError> {
    unsafe {
        let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_ALL)?;
        dialog.SetOptions(FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_OVERWRITEPROMPT)?;
        if audio_only {
            // Static UTF-16 filter strings outlive this STA-owned dialog and its Show call.
            dialog.SetTitle(w!("Export audio only (video edits remain unchanged)"))?;
            dialog.SetFileTypes(&audio_file_types())?;
            dialog.SetFileTypeIndex(1)?;
            dialog.SetDefaultExtension(w!("wav"))?;
        }
        let wide = suggested_name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        dialog.SetFileName(PCWSTR(wide.as_ptr()))?;
        Ok(dialog)
    }
}

fn audio_file_types() -> [COMDLG_FILTERSPEC; 7] {
    [
        COMDLG_FILTERSPEC {
            pszName: w!("WAV audio (*.wav)"),
            pszSpec: w!("*.wav"),
        },
        COMDLG_FILTERSPEC {
            pszName: w!("FLAC audio (*.flac)"),
            pszSpec: w!("*.flac"),
        },
        COMDLG_FILTERSPEC {
            pszName: w!("MP3 audio (*.mp3)"),
            pszSpec: w!("*.mp3"),
        },
        COMDLG_FILTERSPEC {
            pszName: w!("M4A / AAC audio (*.m4a)"),
            pszSpec: w!("*.m4a"),
        },
        COMDLG_FILTERSPEC {
            pszName: w!("AAC audio (*.aac)"),
            pszSpec: w!("*.aac"),
        },
        COMDLG_FILTERSPEC {
            pszName: w!("Ogg / Opus audio (*.ogg)"),
            pszSpec: w!("*.ogg"),
        },
        COMDLG_FILTERSPEC {
            pszName: w!("Opus audio (*.opus)"),
            pszSpec: w!("*.opus"),
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn audio_save_dialog_accepts_seven_types_unicode_name_and_preserves_overwrite_prompt_without_showing()
     {
        thread::spawn(|| {
            // This owned worker initializes an STA, never calls Show or changes focus,
            // frees the returned filename and drops every interface before the apartment.
            unsafe {
                OleInitialize(None).expect("STA");
                let _apartment = DialogApartment;
                let dialog =
                    prepare_save_dialog("動画.final-audio.wav", true).expect("audio save dialog");
                let options = dialog.GetOptions().expect("options");
                assert_eq!(options & FOS_OVERWRITEPROMPT, FOS_OVERWRITEPROMPT);
                assert_eq!(options & FOS_PATHMUSTEXIST, FOS_PATHMUSTEXIST);
                assert_eq!(dialog.GetFileTypeIndex().expect("default type"), 1);
                for index in 1..=audio_file_types().len() as u32 {
                    dialog.SetFileTypeIndex(index).expect("supported type");
                    assert_eq!(dialog.GetFileTypeIndex().expect("selected type"), index);
                }
                let name = dialog.GetFileName().expect("suggested name");
                let text = name.to_string();
                CoTaskMemFree(Some(name.0.cast()));
                assert_eq!(text.expect("UTF-16"), "動画.final-audio.wav");
                let regular =
                    prepare_save_dialog("image-export.png", false).expect("regular save dialog");
                assert_eq!(
                    regular.GetOptions().expect("regular options") & FOS_OVERWRITEPROMPT,
                    FOS_OVERWRITEPROMPT
                );
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
