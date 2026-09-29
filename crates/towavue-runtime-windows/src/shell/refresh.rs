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

    // SAFETY: use only on the subscribed private, non-persisting view on this
    // STA. SetSortColumns success is not completion: it can retain old rows
    // until SortDone. Even saved columns can accompany initial name-order rows.
    pub(super) unsafe fn apply_ready(
        &self,
        view: &IFolderView2,
        folder: &Path,
        events: &HiddenEnumeration,
        current: &impl Fn() -> bool,
    ) -> Option<()> {
        unsafe {
            let previous = Self::read(view)?;
            let same_columns = |a: &[SORTCOLUMN], b: &[SORTCOLUMN]| {
                a.len() == b.len()
                    && a.iter()
                        .zip(b)
                        .all(|(a, b)| a.propkey == b.propkey && a.direction == b.direction)
            };
            // Establish membership first: native row count can transiently be
            // zero even while a nonempty array is available.
            let deadline = Instant::now() + Duration::from_secs(2);
            let count = loop {
                if !current() || Instant::now() >= deadline {
                    return None;
                }
                pump_messages();
                if let Some(array) = view_order_array(view, folder) {
                    break array
                        .as_ref()
                        .map_or(Some(0), |array| array.GetCount().ok())?;
                }
                MsgWaitForMultipleObjectsEx(None, 10, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            };
            if count < 2 {
                return self.apply(view);
            }
            if previous.group != self.group || previous.ascending != self.ascending {
                let serial = events.enumerations.load(AtomicOrdering::Acquire);
                view.SetGroupBy(&self.group, self.ascending).ok()?;
                // Group changes complete with EnumDone, without SortDone.
                events.wait_serial(&events.enumerations, serial, current)?;
            }
            if same_columns(&Self::read(view)?.columns, &self.columns) {
                // Reapplying identical settings is a no-op. Complete a temporary
                // reverse first; overlapping requests can discard the restore.
                let mut reverse = self.columns.clone();
                let first = reverse.first_mut()?;
                first.direction = if first.direction == SORT_ASCENDING {
                    SORT_DESCENDING
                } else {
                    SORT_ASCENDING
                };
                let serial = events.sort_serial();
                view.SetSortColumns(&reverse).ok()?;
                events.wait_sort_after(serial, current)?;
            }
            let serial = events.sort_serial();
            view.SetSortColumns(&self.columns).ok()?;
            events.wait_sort_after(serial, current)?;
            let actual = Self::read(view)?;
            (same_columns(&actual.columns, &self.columns)
                && actual.group == self.group
                && actual.ascending == self.ascending)
                .then_some(())
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
