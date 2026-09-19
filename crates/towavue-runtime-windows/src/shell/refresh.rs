use super::*;
use windows::Win32::UI::Shell::{SHCNE_UPDATEDIR, SHCNF_FLUSHNOWAIT, SHCNF_IDLIST, SHChangeNotify};

pub(super) struct ViewOrder {
    columns: Vec<SORTCOLUMN>,
    pub(super) group: PROPERTYKEY,
    pub(super) ascending: bool,
}

impl ViewOrder {
    // SAFETY: view belongs to the caller's initialized STA. Only plain settings
    // are retained; copying them never changes a live view or its persisted bag.
    pub(super) unsafe fn read(view: &IFolderView2) -> Option<Self> {
        unsafe {
            let count = usize::try_from(view.GetSortColumnCount().ok()?).ok()?;
            let mut columns = vec![SORTCOLUMN::default(); count];
            view.GetSortColumns(&mut columns).ok()?;
            let mut group = PROPERTYKEY::default();
            let mut ascending = windows::core::BOOL::default();
            view.GetGroupBy(&mut group, Some(&mut ascending)).ok()?;
            Some(Self {
                columns,
                group,
                ascending: ascending.as_bool(),
            })
        }
    }

    // SAFETY: use only on an enumerated, private view protected against writes
    // to Explorer's saved state. Native Shell performs grouping and sorting.
    pub(super) unsafe fn apply(&self, view: &IFolderView2) -> Option<()> {
        unsafe {
            view.SetGroupBy(&self.group, self.ascending).ok()?;
            view.SetSortColumns(&self.columns).ok()?;
        }
        Some(())
    }
}

pub(super) fn notify_folder(folder: &OwnedPidl) {
    // SAFETY: the absolute PIDL remains owned through this synchronous copy.
    // Delivery is asynchronous; it is not proof that an existing view has sorted.
    unsafe {
        SHChangeNotify(
            SHCNE_UPDATEDIR,
            SHCNF_IDLIST | SHCNF_FLUSHNOWAIT,
            Some(folder.as_ptr().cast()),
            None,
        );
    }
}

/// Best-effort notification after actual publication, on the existing worker.
/// It cannot turn a successfully published file into a failed save.
pub(crate) fn notify_published_file(path: &Path) {
    let apartment = ShellApartment::new();
    if !apartment.0 {
        return;
    }
    if let Some(folder) = path
        .parent()
        .and_then(|p| canonical_shell_path(p).ok())
        .and_then(|p| parse_path(&p))
    {
        notify_folder(&folder);
    }
}
