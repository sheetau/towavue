use egui::{Color32, FontId, Id, Rect, Response, Ui};
use std::path::PathBuf;
use towavue_core::MediaKind;

const GAP: f32 = 16.0;
pub(super) const INSET: f32 = 18.0;
pub(super) const SEARCH_MIN: f32 = 100.0;
pub(super) const SEARCH_MAX: f32 = 250.0;

pub(super) fn text_height(ui: &Ui) -> f32 {
    ui.text_style_height(&egui::TextStyle::Body)
}

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
    pub values: [usize; 4],
}

impl Counts {
    pub fn invalidate(&mut self) {
        self.valid = false;
    }
    pub fn update(&mut self, paths: &[PathBuf], query: &str) {
        if self.valid && self.query == query {
            return;
        }
        self.values = [0; 4];
        for path in paths
            .iter()
            .filter(|path| crate::welcome::matches(path, query))
        {
            if let Some(kind) = MediaKind::from_path(path) {
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

/// A text-only control: underline belongs to the label, never its surrounding hit area.
pub(super) fn text_button(ui: &Ui, rect: Rect, id: Id, label: &str, selected: bool) -> Response {
    text_button_color(ui, rect, id, label, selected, Color32::WHITE)
}

pub(super) fn text_button_color(
    ui: &Ui,
    rect: Rect,
    id: Id,
    label: &str,
    selected: bool,
    color: Color32,
) -> Response {
    let rect = Rect::from_center_size(rect.center(), egui::vec2(rect.width(), text_height(ui)));
    let response = ui.interact(rect, id, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            response.enabled(),
            selected,
            label,
        )
    });
    let mut text = egui::RichText::new(label).color(color);
    if selected || response.hovered() || response.has_focus() {
        text = text.underline();
    }
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

/// A populated query remains visible after blur; Escape clears and collapses it.
pub(super) fn search(ui: &mut Ui, rect: Rect, id: Id, label: &str, query: &mut String) -> bool {
    let state = id.with("editing");
    let requested = ui.is_enabled()
        && ui.data_mut(|data| {
            data.remove_temp::<bool>(Id::new("browser-search-focus"))
                .unwrap_or(false)
        });
    if requested {
        ui.data_mut(|data| data.insert_temp(state, true));
        ui.memory_mut(|memory| memory.request_focus(id));
    }
    if ui.is_enabled()
        && ui.memory(|memory| memory.has_focus(id))
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        query.clear();
        ui.memory_mut(|memory| memory.surrender_focus(id));
        ui.data_mut(|data| data.insert_temp(state, false));
        ui.ctx().request_discard("Collapse search after Escape");
        return true;
    }
    let mut editing = !query.is_empty()
        || (ui
            .data(|data| data.get_temp::<bool>(state))
            .unwrap_or(false)
            && ui.memory(|memory| memory.has_focus(id)));
    let mut opened = requested;
    if !editing {
        let title = crate::localization::text(ui.ctx(), crate::localization::Text::FilmstripSearch);
        let width = ui
            .painter()
            .layout_no_wrap(
                title.into(),
                egui::TextStyle::Body.resolve(ui.style()),
                Color32::WHITE,
            )
            .size()
            .x;
        let button = Rect::from_min_max(
            egui::pos2((rect.right() - width).max(rect.left()), rect.top()),
            rect.max,
        );
        if text_button(ui, button, id.with("button"), title, false).clicked() {
            editing = true;
            opened = true;
        }
    }
    let mut changed = false;
    if editing {
        let rect = Rect::from_center_size(
            rect.center(),
            egui::vec2(rect.width(), crate::chrome::INPUT_HEIGHT),
        );
        let response = ui.put(rect, |ui: &mut Ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(rect));
            ui.spacing_mut().text_edit_width = f32::INFINITY;
            ui.visuals_mut().override_text_color = Some(Color32::WHITE);
            crate::resize::unframed_text_input_with_id(ui, id, "", query)
        });
        let pixel = 1.0 / ui.ctx().pixels_per_point();
        let bottom = (rect.bottom() / pixel).round() * pixel;
        ui.painter().rect_filled(
            Rect::from_min_max(
                egui::pos2((rect.left() / pixel).round() * pixel, bottom - pixel),
                egui::pos2((rect.right() / pixel).round() * pixel, bottom),
            ),
            0.0,
            Color32::WHITE,
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, response.enabled(), label)
        });
        if opened {
            response.request_focus();
        }
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
        changed = response.changed();
    }
    ui.data_mut(|data| data.insert_temp(state, editing));
    if opened {
        ui.ctx()
            .request_discard("Replace Search with its focused editor");
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

struct FilterGeometry {
    widths: [f32; 4],
    numbers: [std::sync::Arc<egui::Galley>; 4],
    visible: [bool; 4],
    space: f32,
    row: f32,
}

impl FilterGeometry {
    fn new(ui: &Ui, counts: &[usize; 4]) -> Self {
        let font = egui::TextStyle::Body.resolve(ui.style());
        Self {
            widths: labels(ui).map(|(label, _)| {
                ui.painter()
                    .layout_no_wrap(label.into(), font.clone(), Color32::WHITE)
                    .size()
                    .x
            }),
            numbers: counts.map(|count| {
                ui.painter().layout_no_wrap(
                    count.to_string(),
                    FontId::proportional(10.0),
                    Color32::PLACEHOLDER,
                )
            }),
            visible: visible_filters(counts),
            space: ui
                .painter()
                .layout_no_wrap(" ".into(), font, Color32::WHITE)
                .size()
                .x,
            row: text_height(ui),
        }
    }

    fn size(&self) -> egui::Vec2 {
        let indices: Vec<_> = (0..4).filter(|&i| self.visible[i]).collect();
        let gaps = indices.len().saturating_sub(1) as f32;
        egui::vec2(
            indices
                .iter()
                .map(|&i| self.widths[i] + self.space + self.numbers[i].size().x)
                .sum::<f32>()
                + GAP * gaps,
            self.row,
        )
    }
}

pub(super) fn filter_size(ui: &Ui, counts: &[usize; 4]) -> egui::Vec2 {
    FilterGeometry::new(ui, counts).size()
}

pub(super) fn filters(
    ui: &mut Ui,
    rect: Rect,
    counts: &[usize; 4],
    current: &mut Option<MediaKind>,
) {
    normalize_filter(counts, current);
    let geometry = FilterGeometry::new(ui, counts);
    let gaps = geometry
        .visible
        .iter()
        .filter(|&&visible| visible)
        .count()
        .saturating_sub(1) as f32;
    let content_width = geometry.size().x - GAP * gaps;
    let gap = if gaps > 0.0 {
        ((rect.width() - content_width) / gaps).clamp(0.0, GAP)
    } else {
        0.0
    };
    let mut x = rect.right() - content_width - gap * gaps;
    let single = counts[1..].iter().filter(|&&count| count > 0).count() == 1;
    for (index, (label, kind)) in labels(ui).into_iter().enumerate() {
        if !geometry.visible[index] {
            continue;
        }
        let right = x + geometry.widths[index];
        let name = Rect::from_min_size(
            egui::pos2(x, rect.top()),
            egui::vec2(geometry.widths[index], geometry.row),
        );
        let response = text_button(
            ui,
            name,
            ui.id().with(("media-type", index)),
            label,
            *current == kind || (single && current.is_none()),
        );
        if response.clicked() {
            *current = kind;
            ui.ctx().request_repaint();
        }
        ui.painter().galley(
            egui::pos2(
                right + geometry.space,
                name.center().y - geometry.numbers[index].size().y * 0.5 - 3.0,
            ),
            geometry.numbers[index].clone(),
            Color32::WHITE,
        );
        x = right + geometry.space + geometry.numbers[index].size().x + gap;
    }
}
