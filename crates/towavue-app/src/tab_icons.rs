use crate::fluent::Kind;
use egui::{Color32, Context, Rect, Response, Ui};
use std::path::Path;
use towavue_core::MediaKind;

pub(super) fn set(context: &Context, enabled: bool) {
    context.data_mut(|data| data.insert_temp(egui::Id::new("tab-icons"), enabled));
}

pub(super) fn enabled(context: &Context) -> bool {
    context
        .data(|data| data.get_temp(egui::Id::new("tab-icons")))
        .unwrap_or(true)
}

pub(super) fn media(path: Option<&Path>) -> Option<Kind> {
    Some(kind(MediaKind::from_path(path?)?))
}

pub(super) fn kind(kind: MediaKind) -> Kind {
    match kind {
        MediaKind::Image => Kind::Image,
        MediaKind::Video => Kind::Video,
        MediaKind::Audio => Kind::Audio,
    }
}

pub(super) fn title_color(ui: &Ui, response: &Response, active: bool) -> Color32 {
    if active {
        crate::chrome::FOREGROUND
    } else {
        ui.style().interact(response).fg_stroke.color
    }
}

pub(super) fn paint(ui: &Ui, rect: Rect, kind: Kind, color: Color32) {
    crate::fluent::paint(
        ui,
        Rect::from_center_size(rect.center(), egui::Vec2::splat(16.0)),
        kind,
        color,
    );
}

impl<N: Fn(crate::AppEvent) + Send + Sync + 'static> crate::Application<N> {
    pub(super) fn shows_tab_icons(&self) -> bool {
        self.view_preferences
            .as_ref()
            .map_or(self.tab_icons, |store| {
                store.value(towavue_runtime_windows::ViewPreference::TabIcons) != 0
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoration_shares_title_hit_target_and_text_color() {
        for density in [1.0, 1.25, 2.0] {
            for active in [false, true] {
                let context = crate::fonts::test_context();
                context.set_pixels_per_point(density);
                context.global_style_mut(crate::chrome::style);
                let rect = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(180.0, 28.0));
                let mut response = None;
                let mut draw = |events| {
                    context.run_ui(
                        egui::RawInput {
                            events,
                            ..Default::default()
                        },
                        |ui| {
                            response = Some(crate::chrome::tab_title_with_icon(
                                ui,
                                rect,
                                "Photo",
                                active,
                                None,
                                Some(Kind::Image),
                            ));
                        },
                    )
                };
                draw(vec![]);
                let point = egui::pos2(rect.left() + 12.0, rect.center().y);
                draw(vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
                let output = draw(vec![egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }]);
                assert!(
                    response.expect("title").clicked(),
                    "decoration activates the same tab title"
                );
                let icon = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Mesh(mesh) if rect.contains_rect(mesh.calc_bounds()) => {
                            Some(mesh)
                        }
                        _ => None,
                    })
                    .expect("icon");
                let title = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text() == "Photo" => Some(text),
                        _ => None,
                    })
                    .expect("title");
                let color = if active {
                    crate::chrome::FOREGROUND
                } else {
                    title.fallback_color
                };
                assert!(icon.vertices.iter().all(|vertex| vertex.color == color));
                assert_eq!(icon.calc_bounds().width(), 16.0);
            }
        }
    }
}
