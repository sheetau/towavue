use crate::hover_help::HoverHelp;
use egui::{Color32, FontId, Id, Rect, Response, Ui};
use std::path::PathBuf;
use towavue_core::MediaKind;

pub(super) fn query_error(
    expression: &towavue_core::FileQuery,
    gallery: bool,
) -> Option<crate::localization::Text> {
    use crate::localization::Text;
    use towavue_core::FileQueryError;
    expression
        .error()
        .map(|error| match error {
            FileQueryError::Syntax => Text::FileQuerySyntax,
            FileQueryError::Date => Text::FileQueryDate,
            FileQueryError::UnsupportedProperty => Text::FileQueryProperty,
            FileQueryError::TooComplex => Text::FileQueryComplexity,
        })
        .or_else(|| {
            (!gallery && expression.uses_gallery_date()).then_some(Text::FileQueryGalleryOnly)
        })
}

#[cfg(test)]
pub(super) const INSET: f32 = 3.0;
pub(super) const SEARCH_MIN: f32 = 100.0;
pub(super) const SEARCH_MAX: f32 = 250.0;

pub(super) fn visible_filters(counts: &[usize; 4]) -> [bool; 4] {
    let kinds = counts[1..].iter().filter(|&&count| count > 0).count();
    [kinds != 1, counts[1] > 0, counts[2] > 0, counts[3] > 0]
}

pub(super) fn normalize_filter(counts: &[usize; 4], current: &mut Option<MediaKind>) {
    let index = match current {
        None => 0,
        Some(MediaKind::Image) => 1,
        Some(MediaKind::Video) => 2,
        Some(MediaKind::Audio) => 3,
    };
    if counts[index] == 0 {
        *current = None;
    }
}

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Counts {
    query: String,
    valid: bool,
    dates: std::collections::BTreeMap<PathBuf, towavue_core::SearchDate>,
    today: Option<towavue_core::SearchDate>,
    pub values: [usize; 4],
}

impl Counts {
    pub fn invalidate(&mut self) {
        self.valid = false;
    }
    pub fn gallery_dates(
        &mut self,
        dates: &std::collections::BTreeMap<PathBuf, towavue_core::SearchDate>,
    ) {
        self.dates.clone_from(dates);
        self.invalidate();
    }
    pub fn update(&mut self, paths: &[PathBuf], query: &str) {
        let today = towavue_runtime_windows::current_local_date();
        if self.valid && self.query == query && self.today == today {
            return;
        }
        self.today = today;
        let expression = towavue_core::FileQuery::new(query, today);
        self.values = [0; 4];
        for path in paths {
            if expression.matches_gallery(path, self.dates.get(path).copied())
                && let Some(kind) = MediaKind::from_path(path)
            {
                self.add(kind);
            }
        }
        self.query = query.into();
        self.valid = true;
    }
    pub fn add(&mut self, kind: MediaKind) {
        self.values[0] += 1;
        self.values[match kind {
            MediaKind::Image => 1,
            MediaKind::Video => 2,
            MediaKind::Audio => 3,
        }] += 1;
    }
}

pub(super) fn text_button_color(
    ui: &Ui,
    rect: Rect,
    id: Id,
    label: &str,
    selected: bool,
    color: Color32,
) -> Response {
    let response = ui.interact(rect, id, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            response.enabled(),
            selected,
            label,
        )
    });
    let hot = response.hovered() || response.has_focus();
    if hot {
        ui.painter()
            .rect_filled(rect, crate::chrome::RADIUS_XS, crate::chrome::ACTIVE_TAB);
    }
    let color = if hot {
        crate::chrome::FOREGROUND
    } else {
        color
    };
    let text = egui::RichText::new(label)
        .size(crate::chrome::TEXT_LG)
        .color(color);
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        rect.width(),
        egui::TextStyle::Body,
    );
    ui.painter()
        .with_clip_rect(ui.clip_rect().intersect(rect.expand(1.0)))
        .galley(
            egui::pos2(rect.left(), rect.center().y - galley.size().y * 0.5),
            galley,
            color,
        );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The active Gallery/Filmstrip consumes this request when its header is drawn.
pub(super) fn request_search_focus(context: &egui::Context) {
    context.data_mut(|data| data.insert_temp(Id::new("browser-search-focus"), true));
}

pub(super) fn search_active(ui: &Ui, id: Id, query: &str) -> bool {
    !query.is_empty()
        || ui
            .data(|data| data.get_temp::<bool>(id.with("editing")))
            .unwrap_or(false)
}

/// Search replaces the adjacent status content until explicitly closed.
pub(super) fn search(ui: &mut Ui, rect: Rect, id: Id, label: &str, query: &mut String) -> bool {
    let requested = ui.is_enabled()
        && ui.data_mut(|data| {
            data.remove_temp::<bool>(Id::new("browser-search-focus"))
                .unwrap_or(false)
        });
    let mut editing = search_active(ui, id, query) || requested;
    let button_rect = Rect::from_min_size(rect.min, egui::vec2(24.0, 24.0));
    let button = ui
        .push_id(id.with("button"), |ui| {
            crate::status_bar::icon_at(
                ui,
                button_rect,
                crate::fluent::Kind::Search,
                editing,
                crate::localization::Text::FilmstripSearch
                    .in_language(crate::localization::language(ui.ctx())),
            )
        })
        .inner;
    let opened = requested || (button.clicked() && !editing);
    editing |= opened;
    let mut changed = false;
    if editing {
        let input = Rect::from_min_max(
            egui::pos2(rect.left() + 26.0, rect.top()),
            egui::pos2(
                (rect.left() + 26.0 + SEARCH_MAX + 29.0).min(rect.right()),
                rect.top() + 24.0,
            ),
        );
        let close = Rect::from_min_size(
            egui::pos2((input.right() - 24.0).max(input.left()), input.top()),
            egui::vec2(24.0, 24.0),
        );
        let text_rect = Rect::from_min_max(
            input.min + egui::vec2(crate::chrome::TAB_PADDING - 4.0, 0.0),
            egui::pos2((close.left() - 2.0).max(input.left()), input.bottom()),
        );
        let response = ui.place(text_rect, |ui: &mut Ui| {
            ui.style_mut().override_font_id = Some(FontId::proportional(crate::chrome::TEXT_LG));
            ui.spacing_mut().text_edit_width = f32::INFINITY;
            ui.visuals_mut().override_text_color = Some(crate::chrome::FOREGROUND);
            crate::resize::unframed_text_input_with_id(ui, id, "", query)
        });
        let response = response.help_text(crate::localization::text(
            ui.ctx(),
            crate::localization::Text::FileQueryHelp,
        ));
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, response.enabled(), label)
        });
        if opened {
            response.request_focus();
        }
        changed = response.changed();
        if response.has_focus() {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                        ..Default::default()
                    },
                )
            });
        }
        let close_button = crate::status_bar::icon_at(
            ui,
            close,
            crate::fluent::Kind::Dismiss,
            false,
            crate::localization::text(ui.ctx(), crate::localization::Text::Cancel),
        );
        if close_button.clicked()
            || (response.has_focus()
                && ui
                    .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)))
        {
            changed |= !query.is_empty();
            query.clear();
            editing = false;
            ui.memory_mut(|memory| memory.surrender_focus(id));
            ui.ctx().request_discard("Restore browser status content");
        }
    }
    ui.data_mut(|data| data.insert_temp(id.with("editing"), editing));
    if opened {
        ui.ctx().request_discard("Show browser search editor");
    }
    changed
}

pub(super) fn labels(ui: &Ui) -> [(&'static str, Option<MediaKind>); 4] {
    use crate::localization::{self, Text};
    let language = localization::language(ui.ctx());
    [
        (Text::GalleryAll.in_language(language), None),
        (
            Text::GalleryImages.in_language(language),
            Some(MediaKind::Image),
        ),
        (
            Text::GalleryVideos.in_language(language),
            Some(MediaKind::Video),
        ),
        (
            Text::GalleryAudio.in_language(language),
            Some(MediaKind::Audio),
        ),
    ]
}

pub(super) fn filter_size(ui: &Ui, counts: &[usize; 4]) -> egui::Vec2 {
    let visible = visible_filters(counts);
    let widths: Vec<_> = labels(ui)
        .iter()
        .enumerate()
        .filter(|(i, _)| visible[*i])
        .map(|(i, (label, _))| crate::status_bar::width(ui, &format!("{label} {}", counts[i])))
        .collect();
    egui::vec2(
        widths.iter().sum::<f32>()
            + widths.len().saturating_sub(1) as f32 * crate::chrome::ITEM_GAP,
        24.0,
    )
}

pub(super) fn filters(
    ui: &mut Ui,
    rect: Rect,
    counts: &[usize; 4],
    current: &mut Option<MediaKind>,
) {
    normalize_filter(counts, current);
    let visible = visible_filters(counts);
    let single = counts[1..].iter().filter(|&&n| n > 0).count() == 1;
    let labels = labels(ui);
    let size = filter_size(ui, counts);
    if size.x > rect.width() {
        let label = labels
            .iter()
            .find(|(_, kind)| kind == current)
            .map_or(labels[0].0, |(label, _)| *label);
        let response = crate::status_bar::item_at(
            ui,
            rect,
            egui::RichText::new(label).size(crate::chrome::TEXT_LG),
            true,
        );
        crate::welcome::filter_menu::show(&response, |ui, chosen| {
            for (i, (label, kind)) in labels.iter().enumerate() {
                if visible[i]
                    && chosen(
                        &ui.selectable_label(*current == *kind, format!("{label} {}", counts[i])),
                    )
                {
                    *current = *kind;
                    ui.close();
                    ui.ctx().request_discard("Update media filter");
                }
            }
        });
        return;
    }
    let mut x = rect.right() - size.x;
    for (index, (label, kind)) in labels.into_iter().enumerate() {
        if !visible[index] {
            continue;
        }
        let selected = *current == kind || (single && current.is_none());
        let w = crate::status_bar::width(ui, &format!("{label} {}", counts[index]));
        let slot = Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, 24.0));
        let mut job = egui::text::LayoutJob::default();
        job.append(
            label,
            0.0,
            egui::TextFormat {
                font_id: FontId::proportional(crate::chrome::TEXT_LG),
                color: if selected {
                    crate::chrome::FOREGROUND
                } else {
                    crate::chrome::MUTED
                },
                ..Default::default()
            },
        );
        job.append(
            &format!(" {}", counts[index]),
            0.0,
            egui::TextFormat {
                font_id: FontId::proportional(crate::chrome::TEXT_LG),
                color: if selected {
                    crate::chrome::FAINT
                } else {
                    crate::chrome::DISABLED
                },
                ..Default::default()
            },
        );
        let response = crate::status_bar::item_at(ui, slot, job, !selected);
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                response.enabled(),
                selected,
                label,
            )
        });
        if response.clicked() {
            *current = kind;
            ui.ctx().request_discard("Update media filter");
        }
        x += w + crate::chrome::ITEM_GAP;
    }
}

/// Shared status layout keeps search and filters within the same 24-point row.
pub(super) fn gallery_status(
    ui: &mut Ui,
    id: Id,
    counts: &[usize; 4],
    query: &mut String,
    filter: &mut Option<MediaKind>,
    summary: egui::RichText,
) {
    use crate::localization::Text;
    let language = crate::localization::language(ui.ctx());
    let rect = ui.available_rect_before_wrap();
    let filter_width = crate::browser_controls::filter_size(ui, counts)
        .x
        .min((rect.width() - (31.0 + crate::browser_controls::SEARCH_MIN + 26.0)).max(0.0));
    let filters = egui::Rect::from_min_max(
        egui::pos2(rect.right() - filter_width, rect.top()),
        rect.max,
    );
    let search = egui::Rect::from_min_max(
        rect.min,
        egui::pos2((filters.left() - 2.0).max(rect.left()), rect.bottom()),
    );
    crate::browser_controls::search(
        ui,
        search,
        id,
        Text::GallerySearch.in_language(language),
        query,
    );
    if !crate::browser_controls::search_active(ui, id, query) {
        let label = egui::Rect::from_min_max(search.min + egui::vec2(26.0, 0.0), search.max);
        crate::status_bar::item_at(ui, label, summary, false);
    }
    crate::browser_controls::filters(ui, filters, counts, filter);
}
