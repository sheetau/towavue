use std::path::PathBuf;
use std::thread;

use thiserror::Error;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};
use windows::Win32::UI::Shell::{
    FILEOPENDIALOGOPTIONS, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT,
    FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog, FileSaveDialog, IFileOpenDialog,
    IFileSaveDialog, SIGDN_FILESYSPATH,
};
use windows::core::PCWSTR;

const ERROR_CANCELLED_HRESULT: u32 = 0x8007_04c7;

#[derive(Debug, Error)]
pub enum DialogError {
    #[error("the file-dialog thread could not start: {0}")]
    Thread(#[from] std::io::Error),
    #[error("the file-dialog thread stopped unexpectedly")]
    ThreadStopped,
    #[error("Windows file dialog failed: {0}")]
    Windows(#[from] windows::core::Error),
    #[error("Windows returned an invalid UTF-16 path: {0}")]
    InvalidPath(#[from] std::string::FromUtf16Error),
}

pub fn pick_media_file() -> Result<Option<PathBuf>, DialogError> {
    show_dialog(false)
}

pub fn pick_folder() -> Result<Option<PathBuf>, DialogError> {
    show_dialog(true)
}

pub fn pick_export_file(suggested_name: &str) -> Result<Option<PathBuf>, DialogError> {
    let suggested_name = suggested_name.to_owned();
    thread::Builder::new()
        .name("towavue-save-dialog-sta".into())
        .spawn(move || save_dialog_thread(&suggested_name))?
        .join()
        .map_err(|_| DialogError::ThreadStopped)?
}

fn show_dialog(folder: bool) -> Result<Option<PathBuf>, DialogError> {
    thread::Builder::new()
        .name("towavue-file-dialog-sta".into())
        .spawn(move || dialog_thread(folder))?
        .join()
        .map_err(|_| DialogError::ThreadStopped)?
}

fn dialog_thread(folder: bool) -> Result<Option<PathBuf>, DialogError> {
    // SAFETY: all dialog COM objects are created, used, and dropped on this thread.
    unsafe {
        OleInitialize(None)?;
        let result = show_initialized_dialog(folder);
        OleUninitialize();
        result
    }
}

fn save_dialog_thread(suggested_name: &str) -> Result<Option<PathBuf>, DialogError> {
    // SAFETY: all dialog COM objects are created, used, and dropped on this thread.
    unsafe {
        OleInitialize(None)?;
        let result = show_initialized_save_dialog(suggested_name);
        OleUninitialize();
        result
    }
}

unsafe fn show_initialized_dialog(folder: bool) -> Result<Option<PathBuf>, DialogError> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL)?;
        let mut options: FILEOPENDIALOGOPTIONS =
            FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_FILEMUSTEXIST;
        if folder {
            options |= FOS_PICKFOLDERS;
        }
        dialog.SetOptions(options)?;
        if let Err(error) = dialog.Show(None) {
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
) -> Result<Option<PathBuf>, DialogError> {
    unsafe {
        let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_ALL)?;
        dialog.SetOptions(FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_OVERWRITEPROMPT)?;
        let wide = suggested_name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        dialog.SetFileName(PCWSTR(wide.as_ptr()))?;
        if let Err(error) = dialog.Show(None) {
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
