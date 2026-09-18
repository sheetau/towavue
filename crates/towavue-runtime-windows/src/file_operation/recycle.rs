use super::{FileOperationError, wide};
use std::path::Path;
use windows::Win32::Foundation::E_ABORT;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};
use windows::Win32::UI::Shell::{
    FILEOPERATION_FLAGS, FOF_NO_CONNECTED_ELEMENTS, FOF_NO_UI, FOF_NORECURSION, FOFX_ADDUNDORECORD,
    FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FileOperation, IFileOperation,
    IFileOperationProgressSink, IFileOperationProgressSink_Impl, IShellItem,
    SHCreateItemFromParsingName, TSF_DELETE_RECYCLE_IF_POSSIBLE,
};
use windows::core::PCWSTR;

// The Shell can propose a permanent delete when recycling is unavailable. This
// STA-local sink rejects that proposal before any file removal, even with UI off.
#[windows::core::implement(IFileOperationProgressSink, Agile = false)]
struct RecycleGuard;

impl IFileOperationProgressSink_Impl for RecycleGuard_Impl {
    fn StartOperations(&self) -> windows_core::Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, _hrresult: windows_core::HRESULT) -> windows_core::Result<()> {
        Ok(())
    }
    fn PreRenameItem(
        &self,
        _dwflags: u32,
        _psiitem: windows_core::Ref<IShellItem>,
        _psznewname: &windows_core::PCWSTR,
    ) -> windows_core::Result<()> {
        Ok(())
    }
    fn PostRenameItem(
        &self,
        _dwflags: u32,
        _psiitem: windows_core::Ref<IShellItem>,
        _psznewname: &windows_core::PCWSTR,
        _hrrename: windows_core::HRESULT,
        _psinewlycreated: windows_core::Ref<IShellItem>,
    ) -> windows_core::Result<()> {
        Ok(())
    }
    fn PreMoveItem(
        &self,
        _dwflags: u32,
        _psiitem: windows_core::Ref<IShellItem>,
        _psidestinationfolder: windows_core::Ref<IShellItem>,
        _psznewname: &windows_core::PCWSTR,
    ) -> windows_core::Result<()> {
        Ok(())
    }
    fn PostMoveItem(
        &self,
        _dwflags: u32,
        _psiitem: windows_core::Ref<IShellItem>,
        _psidestinationfolder: windows_core::Ref<IShellItem>,
        _psznewname: &windows_core::PCWSTR,
        _hrmove: windows_core::HRESULT,
        _psinewlycreated: windows_core::Ref<IShellItem>,
    ) -> windows_core::Result<()> {
        Ok(())
    }
    fn PreCopyItem(
        &self,
        _dwflags: u32,
        _psiitem: windows_core::Ref<IShellItem>,
        _psidestinationfolder: windows_core::Ref<IShellItem>,
        _psznewname: &windows_core::PCWSTR,
    ) -> windows_core::Result<()> {
        Ok(())
    }
    fn PostCopyItem(
        &self,
        _dwflags: u32,
        _psiitem: windows_core::Ref<IShellItem>,
        _psidestinationfolder: windows_core::Ref<IShellItem>,
        _psznewname: &windows_core::PCWSTR,
        _hrcopy: windows_core::HRESULT,
        _psinewlycreated: windows_core::Ref<IShellItem>,
    ) -> windows_core::Result<()> {
        Ok(())
    }
    fn PreDeleteItem(
        &self,
        dwflags: u32,
        _psiitem: windows_core::Ref<IShellItem>,
    ) -> windows_core::Result<()> {
        if dwflags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0 {
            Err(E_ABORT.into())
        } else {
            Ok(())
        }
    }
    fn PostDeleteItem(
        &self,
        _dwflags: u32,
        _psiitem: windows_core::Ref<IShellItem>,
        hrdelete: windows_core::HRESULT,
        _psinewlycreated: windows_core::Ref<IShellItem>,
    ) -> windows_core::Result<()> {
        hrdelete.ok()
    }
    fn PreNewItem(
        &self,
        _dwflags: u32,
        _psidestinationfolder: windows_core::Ref<IShellItem>,
        _psznewname: &windows_core::PCWSTR,
    ) -> windows_core::Result<()> {
        Ok(())
    }
    fn PostNewItem(
        &self,
        _dwflags: u32,
        _psidestinationfolder: windows_core::Ref<IShellItem>,
        _psznewname: &windows_core::PCWSTR,
        _psztemplatename: &windows_core::PCWSTR,
        _dwfileattributes: u32,
        _hrnew: windows_core::HRESULT,
        _psinewitem: windows_core::Ref<IShellItem>,
    ) -> windows_core::Result<()> {
        Ok(())
    }
    fn UpdateProgress(&self, _iworktotal: u32, _iworksofar: u32) -> windows_core::Result<()> {
        Ok(())
    }
    fn ResetTimer(&self) -> windows_core::Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> windows_core::Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> windows_core::Result<()> {
        Ok(())
    }
}

pub(super) fn recycle(path: &Path) -> Result<(), FileOperationError> {
    run(
        path,
        FOF_NO_UI
            | FOF_NO_CONNECTED_ELEMENTS
            | FOF_NORECURSION
            | FOFX_RECYCLEONDELETE
            | FOFX_ADDUNDORECORD
            | FOFX_EARLYFAILURE,
    )
}

pub(super) fn run(path: &Path, flags: FILEOPERATION_FLAGS) -> Result<(), FileOperationError> {
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            // SAFETY: created only after successful initialization on this thread.
            unsafe { OleUninitialize() };
        }
    }
    let name = wide(path)?;
    // SAFETY: this operation worker owns one STA. All Shell interfaces and the
    // non-agile callback are dropped before the apartment, on the same thread.
    unsafe {
        OleInitialize(None)?;
        let _apartment = Apartment;
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None)?;
        let operation: IFileOperation =
            CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
        let guard: IFileOperationProgressSink = RecycleGuard.into();
        operation.SetOperationFlags(flags)?;
        operation.DeleteItem(&item, &guard)?;
        operation.PerformOperations()?;
        if operation.GetAnyOperationsAborted()?.as_bool() {
            return Err(FileOperationError::NotCompleted);
        }
    }
    Ok(())
}
