use super::*;
use crate::export::formats::{Choices, Format};
use windows::Win32::Foundation::S_FALSE;
use windows::Win32::System::Ole::IOleWindow;
use windows::Win32::UI::Shell::{
    FDE_OVERWRITE_RESPONSE, FDE_SHAREVIOLATION_RESPONSE, FDEOR_DEFAULT, FDESVR_DEFAULT,
    IFileDialog, IFileDialogEvents, IFileDialogEvents_Impl, IShellItem,
};
use windows::core::{Interface, Ref};

pub(super) enum SaveFilter {
    Media { choices: Choices, audio_only: bool },
    Frame,
}

/// Lives and drops on the caller's initialized STA. The dialog owns its advised
/// callback; that callback owns values only, avoiding a COM reference cycle.
pub(super) struct PreparedSave {
    dialog: IFileSaveDialog,
    choices: Choices,
    cookie: u32,
    // Keep every UTF-16 filter string alive through Show and Unadvise.
    _filters: Vec<(Vec<u16>, Vec<u16>)>,
}

impl std::ops::Deref for PreparedSave {
    type Target = IFileSaveDialog;
    fn deref(&self) -> &Self::Target {
        &self.dialog
    }
}

impl Drop for PreparedSave {
    fn drop(&mut self) {
        // SAFETY: all construction/Show/teardown happens within DialogApartment.
        unsafe {
            let _ = self.dialog.Unadvise(self.cookie);
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

pub(super) unsafe fn prepare_save_dialog(
    suggested: &str,
    filter: SaveFilter,
) -> Result<PreparedSave, DialogError> {
    let (choices, title) = match filter {
        SaveFilter::Media {
            choices,
            audio_only,
        } => (
            choices,
            if audio_only {
                "Export audio only (video edits remain unchanged)"
            } else {
                "Export as"
            },
        ),
        SaveFilter::Frame => (
            Choices::new(vec![Format::FramePng], std::path::Path::new("frame.png"))?,
            "Export current edited frame",
        ),
    };
    let filters: Vec<_> = choices
        .formats
        .iter()
        .map(|format| {
            let pattern = format
                .extensions()
                .iter()
                .map(|ext| format!("*.{ext}"))
                .collect::<Vec<_>>()
                .join(";");
            (
                wide(&format!("{} ({pattern})", format.label())),
                wide(&pattern),
            )
        })
        .collect();
    let specs: Vec<_> = filters
        .iter()
        .map(|(name, pattern)| COMDLG_FILTERSPEC {
            pszName: PCWSTR(name.as_ptr()),
            pszSpec: PCWSTR(pattern.as_ptr()),
        })
        .collect();
    // SAFETY: caller owns the STA; strings remain alive in PreparedSave, which
    // unadvises before releasing its native interface and filter storage.
    unsafe {
        let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_ALL)?;
        dialog.SetOptions(FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_OVERWRITEPROMPT)?;
        dialog.SetTitle(PCWSTR(wide(title).as_ptr()))?;
        dialog.SetFileTypes(&specs)?;
        dialog.SetFileTypeIndex(choices.initial as u32 + 1)?;
        // Windows updates this extension when the user changes File type.
        // Preserve a supported original alias (e.g. .jpeg or .apng) initially.
        dialog.SetDefaultExtension(PCWSTR(wide(&choices.default_extension).as_ptr()))?;
        dialog.SetFileName(PCWSTR(wide(&choices.filename(suggested)).as_ptr()))?;
        let events: IFileDialogEvents = SaveEvents {
            choices: choices.clone(),
        }
        .into();
        let cookie = dialog.Advise(&events)?;
        Ok(PreparedSave {
            dialog,
            choices,
            cookie,
            _filters: filters,
        })
    }
}

pub(super) unsafe fn show_initialized_save_dialog(
    suggested: &str,
    owner: HWND,
    filter: SaveFilter,
) -> Result<Option<PathBuf>, DialogError> {
    // SAFETY: caller retains the owner and STA until the native dialog closes.
    unsafe {
        let prepared = prepare_save_dialog(suggested, filter)?;
        if let Err(error) = prepared.Show(Some(owner)) {
            if error.code().0 as u32 == ERROR_CANCELLED_HRESULT {
                return Ok(None);
            }
            return Err(error.into());
        }
        let path = result_path(&prepared.dialog.cast()?)?;
        prepared
            .choices
            .validate(prepared.GetFileTypeIndex()?, &path)
            .map_err(DialogError::InvalidExportChoice)?;
        Ok(Some(path))
    }
}

unsafe fn result_path(dialog: &IFileDialog) -> Result<PathBuf, DialogError> {
    unsafe {
        let value = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = value.to_string().map(PathBuf::from);
        CoTaskMemFree(Some(value.0.cast()));
        Ok(path?)
    }
}

#[windows::core::implement(IFileDialogEvents, Agile = false)]
struct SaveEvents {
    choices: Choices,
}

impl IFileDialogEvents_Impl for SaveEvents_Impl {
    fn OnFileOk(&self, dialog: Ref<IFileDialog>) -> windows_core::Result<()> {
        let dialog = dialog.ok()?;
        // SAFETY: Shell invokes this STA-local sink with a live dialog. GetResult
        // is valid in OnFileOk; all acquired interfaces stay in this callback.
        unsafe {
            let choice = dialog
                .GetFileTypeIndex()
                .map_err(DialogError::from)
                .and_then(|index| Ok((index, result_path(dialog)?)))
                .map_err(|error| error.to_string())
                .and_then(|(index, path)| self.choices.validate(index, &path));
            if let Err(message) = choice {
                let owner = dialog
                    .cast::<IOleWindow>()
                    .and_then(|window| window.GetWindow())
                    .ok();
                MessageBoxW(
                    owner,
                    PCWSTR(wide(&message).as_ptr()),
                    w!("Export file type"),
                    MB_OK | MB_ICONWARNING,
                );
                // S_FALSE rejects this filename and leaves the native dialog open.
                return Err(windows_core::Error::from_hresult(S_FALSE));
            }
        }
        Ok(())
    }
    fn OnFolderChanging(
        &self,
        _: Ref<IFileDialog>,
        _: Ref<IShellItem>,
    ) -> windows_core::Result<()> {
        Ok(())
    }
    fn OnFolderChange(&self, _: Ref<IFileDialog>) -> windows_core::Result<()> {
        Ok(())
    }
    fn OnSelectionChange(&self, _: Ref<IFileDialog>) -> windows_core::Result<()> {
        Ok(())
    }
    fn OnTypeChange(&self, _: Ref<IFileDialog>) -> windows_core::Result<()> {
        Ok(())
    }
    fn OnShareViolation(
        &self,
        _: Ref<IFileDialog>,
        _: Ref<IShellItem>,
    ) -> windows_core::Result<FDE_SHAREVIOLATION_RESPONSE> {
        Ok(FDESVR_DEFAULT)
    }
    fn OnOverwrite(
        &self,
        _: Ref<IFileDialog>,
        _: Ref<IShellItem>,
    ) -> windows_core::Result<FDE_OVERWRITE_RESPONSE> {
        Ok(FDEOR_DEFAULT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_formats_native_dialog_keeps_initial_aliases_and_scopes_file_type_validation() {
        std::thread::spawn(|| unsafe {
            OleInitialize(None).expect("owned STA");
            let _apartment = DialogApartment;
            for (source, initial, extension) in [
                ("image-export.jpeg", 2, "jpeg"),
                ("image-export.apng", 1, "apng"),
                ("image-export.unknown", 1, "png"),
            ] {
                let choices = Choices::new(
                    vec![Format::Png, Format::Jpeg, Format::Webp],
                    std::path::Path::new(source),
                )
                .expect("formats");
                let dialog = prepare_save_dialog(
                    source,
                    SaveFilter::Media {
                        choices,
                        audio_only: false,
                    },
                )
                .expect("native save dialog");
                assert_eq!(dialog.GetFileTypeIndex().expect("selected type"), initial);
                let name = dialog.GetFileName().expect("name");
                let text = name.to_string();
                CoTaskMemFree(Some(name.0.cast()));
                let text = text.expect("UTF-16 filename");
                assert!(text.ends_with(extension), "{text}");
                assert!(
                    dialog
                        .choices
                        .validate(initial, std::path::Path::new(&text))
                        .is_ok()
                );
                for index in 1..=3 {
                    dialog.SetFileTypeIndex(index).expect("type switch");
                    assert_eq!(dialog.GetFileTypeIndex().expect("new type"), index);
                    let target = PathBuf::from("export")
                        .with_extension(dialog.choices.formats[index as usize - 1].extensions()[0]);
                    assert!(dialog.choices.validate(index, &target).is_ok());
                }
                assert!(
                    dialog
                        .choices
                        .validate(3, std::path::Path::new("wrong.jpg"))
                        .is_err()
                );
                // Drop unadvises before the STA guard; repeated dialogs do not
                // retain a callback or a previous document's type restrictions.
            }
            let frame = prepare_save_dialog("frame.png", SaveFilter::Frame).expect("PNG frame");
            assert!(
                frame
                    .choices
                    .validate(1, std::path::Path::new("frame.apng"))
                    .is_err()
            );
            assert_eq!(windows_core::Error::from_hresult(S_FALSE).code(), S_FALSE);
        })
        .join()
        .expect("native dialog worker");
    }
}
