use std::collections::HashSet;
use std::path::PathBuf;

use towavue_core::MediaKind;

use crate::{Application, UiAction, gallery_rail, welcome};

/// Derived UI data only. Invalidate on history delivery/clear; source files are
/// never queried here. Query/type changes rebuild the filtered projection once.
#[derive(Default)]
pub(super) struct Listing {
    source_valid: bool,
    filter_valid: bool,
    available: Vec<PathBuf>,
    filtered: Vec<PathBuf>,
    months: Vec<(Option<(u16, u16)>, usize)>,
    query: String,
    filter: Option<MediaKind>,
}

impl Listing {
    pub fn invalidate(&mut self) {
        self.source_valid = false;
    }
}

impl<Notify: Fn(crate::AppEvent) + Send + Sync + 'static> Application<Notify> {
    pub(super) fn draw_gallery(
        &mut self,
        ui: &mut egui::Ui,
        enabled: bool,
        actions: &mut Vec<UiAction>,
    ) {
        let listing = &mut self.gallery_listing;
        if !listing.source_valid {
            let missing: HashSet<_> = self.gallery_missing_files.iter().collect();
            listing.available = self
                .recent_paths
                .iter()
                .filter(|path| !missing.contains(path))
                .cloned()
                .collect();
            listing.source_valid = true;
            listing.filter_valid = false;
        }
        let Listing {
            available,
            filtered,
            months,
            query: old_query,
            filter: old_filter,
            filter_valid,
            ..
        } = listing;
        if let Some(command) = ui
            .push_id(("gallery", self.tabs.gallery()), |ui| {
                welcome::show(
                    ui,
                    &self.shortcuts,
                    &mut self.gallery_search,
                    &mut self.gallery_filter,
                    available,
                    enabled,
                    |ui, query, filter| {
                        if !*filter_valid || *old_query != query || *old_filter != filter {
                            *filtered = available
                                .iter()
                                .filter(|path| {
                                    (query.trim().is_empty() || welcome::matches(path, query))
                                        && filter.is_none_or(|kind| {
                                            MediaKind::from_path(path) == Some(kind)
                                        })
                                })
                                .cloned()
                                .collect();
                            months.clear();
                            let mut seen = HashSet::new();
                            for (index, path) in filtered.iter().enumerate() {
                                let date = self.recent_months.get(path).copied();
                                if seen.insert(date) {
                                    months.push((date, index));
                                }
                            }
                            *old_query = query.to_owned();
                            *old_filter = filter;
                            *filter_valid = true;
                        }
                        if filtered.is_empty() && !available.is_empty() {
                            ui.label("No matching files.");
                        }
                        let grid =
                            self.filmstrip
                                .show_recent(ui, filtered, ui.is_enabled(), actions);
                        months
                            .iter()
                            .map(|&(date, index)| gallery_rail::Month {
                                date,
                                offset: grid.offset(index),
                            })
                            .collect()
                    },
                )
            })
            .inner
        {
            actions.push(UiAction::Command(command));
        }
    }
}
