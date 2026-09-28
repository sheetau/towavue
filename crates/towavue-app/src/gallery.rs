use std::collections::HashSet;
use std::path::PathBuf;

use towavue_core::MediaKind;

use crate::localization::{Language, Text};
use crate::{Application, UiAction, gallery_rail, welcome};

type LocalDate = (u16, u16, u16);

/// Derived UI data only. Invalidate on history delivery/clear; source files are
/// never queried here. Query/type changes rebuild the filtered projection once.
#[derive(Default)]
pub(super) struct Listing {
    revision: u64,
    source_valid: bool,
    filter_valid: bool,
    available: Vec<PathBuf>,
    filtered: Vec<PathBuf>,
    warmable: Vec<usize>,
    months: Vec<(Option<(u16, u16)>, usize)>,
    query: String,
    filter: Option<MediaKind>,
    visible_dates: Option<[Option<LocalDate>; 2]>,
}

impl Listing {
    pub fn status_date(&self, language: Language) -> Option<String> {
        self.visible_dates.map(|[first, last]| {
            let format = |date: Option<LocalDate>| {
                date.map_or_else(
                    || Text::DateUnknown.in_language(language).into(),
                    |(year, month, day)| {
                        towavue_core::localization::formatted::calendar_day(
                            language,
                            gallery_rail::month_name(month, language),
                            year,
                            day,
                        )
                    },
                )
            };
            if first == last {
                format(first)
            } else {
                format!("{} - {}", format(first), format(last))
            }
        })
    }

    pub(super) fn contains(
        &self,
        path: &std::path::Path,
        revision: u64,
        query: &str,
        filter: Option<MediaKind>,
    ) -> bool {
        self.source_valid
            && self.filter_valid
            && self.revision == revision
            && self.query == query
            && self.filter == filter
            && self.filtered.iter().any(|item| item == path)
    }

    pub fn invalidate(&mut self) {
        self.source_valid = false;
        self.visible_dates = None;
    }
}

impl<Notify: Fn(crate::AppEvent) + Send + Sync + 'static> Application<Notify> {
    pub(super) fn gallery_active(&self) -> bool {
        self.media_kind.is_none()
            && self.tabs.gallery().is_some()
            && self.tabs.active_id() == self.tabs.gallery()
    }

    pub(super) fn gallery_preparation_policy(
        &self,
        context: &egui::Context,
    ) -> crate::filmstrip::PreparationPolicy {
        use crate::filmstrip::PreparationPolicy;
        if self.image_loading
            || self.image_edit_pending
            || !self.image_sequence.steps.is_empty()
            || !self.image_loader.is_idle()
            || self.active_export.is_some()
            || self.pending_folder.is_some()
            || self.modal_input_blocked()
            || self.palette_open
            || self.grid_open
            || egui::Popup::is_any_open(context)
            || context.input(|input| !input.raw.hovered_files.is_empty())
            || (context.input(|input| input.pointer.any_down()) && !gallery_rail::dragging(context))
        {
            PreparationPolicy::Paused
        } else if gallery_rail::dragging(context)
            || self.retained_playback.values().any(|saved| {
                !saved.prepared_only
                    && matches!(
                        saved.state,
                        towavue_core::PlaybackState::Loading | towavue_core::PlaybackState::Playing
                    )
            })
        {
            PreparationPolicy::Visible
        } else {
            PreparationPolicy::All
        }
    }

    pub(super) fn prepare_gallery(&mut self, context: &egui::Context) {
        let listing = &self.gallery_listing;
        if self.gallery_preparation_policy(context) == crate::filmstrip::PreparationPolicy::All
            && listing.source_valid
            && listing.filter_valid
            && listing.query == self.gallery_search
            && listing.filter == self.gallery_filter
        {
            self.filmstrip
                .prepare_recent(&listing.filtered, &listing.warmable, listing.revision);
        } else {
            self.filmstrip.pause_warming();
        }
    }

    pub(super) fn draw_gallery(
        &mut self,
        ui: &mut egui::Ui,
        enabled: bool,
        actions: &mut Vec<UiAction>,
    ) {
        let policy = self.gallery_preparation_policy(ui.ctx());
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
            revision,
            filtered,
            warmable,
            months,
            query: old_query,
            filter: old_filter,
            filter_valid,
            visible_dates,
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
                            // Build once per projection, not while advancing a UI-side cursor.
                            // A long BMP-only history must not create a full-history scan on each frame.
                            warmable.clear();
                            warmable.extend(filtered.iter().enumerate().filter_map(
                                |(index, path)| {
                                    (MediaKind::from_path(path).is_some()
                                        && !path.extension().is_some_and(|extension| {
                                            extension.eq_ignore_ascii_case("bmp")
                                        }))
                                    .then_some(index)
                                },
                            ));
                            months.clear();
                            let mut seen = HashSet::new();
                            for (index, path) in filtered.iter().enumerate() {
                                let date = self
                                    .recent_dates
                                    .get(path)
                                    .map(|&(year, month, _)| (year, month));
                                if seen.insert(date) {
                                    months.push((date, index));
                                }
                            }
                            *old_query = query.to_owned();
                            *old_filter = filter;
                            *filter_valid = true;
                            *revision = revision.wrapping_add(1);
                        }
                        if filtered.is_empty() && !available.is_empty() {
                            ui.label(crate::localization::text(
                                ui.ctx(),
                                Text::GalleryNoMatchingFiles,
                            ));
                        }
                        let grid = self.filmstrip.show_recent_with_policy(
                            ui,
                            filtered,
                            *revision,
                            ui.is_enabled(),
                            actions,
                            policy,
                        );
                        let dates = (!grid.visible.is_empty()).then(|| {
                            // Only the first and last visible records are needed. Focus
                            // overscan and cached thumbnails do not affect this range.
                            [grid.visible.start, grid.visible.end - 1]
                                .map(|index| self.recent_dates.get(&filtered[index]).copied())
                        });
                        if *visible_dates != dates {
                            *visible_dates = dates;
                            // Status is laid out before media input in this frame.
                            ui.ctx().request_repaint();
                        }
                        months
                            .iter()
                            .map(|&(date, index)| gallery_rail::Month {
                                date,
                                offset: if index == 0 {
                                    0.0
                                } else {
                                    crate::filmstrip::GALLERY_GAP + grid.offset(index)
                                },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_range_formats_single_days_cross_months_and_unknown_visits() {
        let mut listing = Listing::default();
        assert_eq!(listing.status_date(Language::English), None);
        for (dates, english, japanese) in [
            (
                [Some((2026, 9, 23)); 2],
                "September 23, 2026",
                "2026\u{5e74}9\u{6708}23\u{65e5}",
            ),
            (
                [Some((2026, 9, 23)), Some((2026, 9, 22))],
                "September 23, 2026 - September 22, 2026",
                "2026\u{5e74}9\u{6708}23\u{65e5} - 2026\u{5e74}9\u{6708}22\u{65e5}",
            ),
            (
                [Some((2026, 1, 1)), Some((2025, 12, 31))],
                "January 1, 2026 - December 31, 2025",
                "2026\u{5e74}1\u{6708}1\u{65e5} - 2025\u{5e74}12\u{6708}31\u{65e5}",
            ),
            (
                [Some((2026, 9, 23)), None],
                "September 23, 2026 - Date unknown",
                "2026\u{5e74}9\u{6708}23\u{65e5} - \u{65e5}\u{4ed8}\u{4e0d}\u{660e}",
            ),
            (
                [None; 2],
                "Date unknown",
                "\u{65e5}\u{4ed8}\u{4e0d}\u{660e}",
            ),
        ] {
            listing.visible_dates = Some(dates);
            assert_eq!(
                listing.status_date(Language::English).as_deref(),
                Some(english)
            );
            assert_eq!(
                listing.status_date(Language::Japanese).as_deref(),
                Some(japanese)
            );
        }
        listing.invalidate();
        assert_eq!(listing.status_date(Language::English), None);
    }
}
