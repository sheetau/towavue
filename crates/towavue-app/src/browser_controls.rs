use egui::{Color32, FontId, Id, Rect, Response, Ui};
use std::path::PathBuf;
use towavue_core::MediaKind;

const ROW: f32 = 22.0;
const GAP: f32 = 16.0;

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
    let color = if ui.is_enabled() {
        Color32::WHITE
    } else {
        crate::chrome::MUTED
    };
    text_button_color(ui, rect, id, label, selected, color)
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

/// A populated query remains visible after blur; Escape clears and collapses it.
pub(super) fn search(ui: &mut Ui, rect: Rect, id: Id, label: &str, query: &mut String) -> bool {
    let state = id.with("editing");
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
    let mut opened = false;
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
        let response = ui.put(rect, |ui: &mut Ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(rect));
            ui.spacing_mut().text_edit_width = f32::INFINITY;
            crate::resize::unframed_text_input_with_id(ui, id, "", query)
        });
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

pub(super) fn filter_size(ui: &Ui, counts: &[usize; 4], vertical: bool) -> egui::Vec2 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let space = ui
        .painter()
        .layout_no_wrap(" ".into(), font.clone(), Color32::WHITE)
        .size()
        .x;
    let widths = labels(ui).map(|(label, _)| {
        ui.painter()
            .layout_no_wrap(label.into(), font.clone(), Color32::WHITE)
            .size()
            .x
    });
    let numbers = counts.map(|count| {
        ui.painter()
            .layout_no_wrap(
                count.to_string(),
                FontId::proportional(10.0),
                Color32::WHITE,
            )
            .size()
            .x
    });
    if vertical {
        egui::vec2(
            widths.into_iter().fold(0.0, f32::max)
                + space
                + numbers.into_iter().fold(0.0, f32::max),
            ROW * 4.0,
        )
    } else {
        egui::vec2(
            widths.into_iter().sum::<f32>()
                + numbers.into_iter().sum::<f32>()
                + space * 4.0
                + GAP * 3.0,
            ROW,
        )
    }
}

pub(super) fn filters(
    ui: &mut Ui,
    rect: Rect,
    counts: &[usize; 4],
    current: &mut Option<MediaKind>,
    vertical: bool,
) {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let space = ui
        .painter()
        .layout_no_wrap(" ".into(), font.clone(), Color32::WHITE)
        .size()
        .x;
    let widths = labels(ui).map(|(label, _)| {
        ui.painter()
            .layout_no_wrap(label.into(), font.clone(), Color32::WHITE)
            .size()
            .x
    });
    let numbers = counts.map(|count| {
        ui.painter().layout_no_wrap(
            count.to_string(),
            FontId::proportional(10.0),
            Color32::PLACEHOLDER,
        )
    });
    let number_width = numbers
        .iter()
        .map(|galley| galley.size().x)
        .fold(0.0, f32::max);
    let gap = ((rect.width()
        - widths.iter().sum::<f32>()
        - numbers.iter().map(|n| n.size().x).sum::<f32>()
        - space * 4.0)
        / 3.0)
        .clamp(0.0, GAP);
    let mut x = rect.left();
    for (index, (label, kind)) in labels(ui).into_iter().enumerate() {
        let y = rect.top() + if vertical { index as f32 * ROW } else { 0.0 };
        let right = if vertical {
            rect.right() - number_width - space
        } else {
            x + widths[index]
        };
        let name = Rect::from_min_size(
            egui::pos2(right - widths[index], y),
            egui::vec2(widths[index], ROW),
        );
        let active = counts[index] > 0 || kind.is_none();
        let response = ui
            .add_enabled_ui(active, |ui| {
                text_button(
                    ui,
                    name,
                    ui.id().with(("media-type", index)),
                    label,
                    *current == kind,
                )
            })
            .inner;
        if response.clicked() {
            *current = kind;
            ui.ctx().request_repaint();
        }
        let color = if active && ui.is_enabled() {
            Color32::WHITE
        } else {
            crate::chrome::MUTED
        };
        ui.painter()
            .with_clip_rect(ui.clip_rect().intersect(rect))
            .galley(
                egui::pos2(
                    right + space,
                    name.center().y - numbers[index].size().y * 0.5 - 3.0,
                ),
                numbers[index].clone(),
                color,
            );
        x = right + space + numbers[index].size().x + gap;
    }
}
