use crate::timeline_input;
use egui::{Color32, Context, Rect, Response};

pub fn show(
    context: &Context,
    status: Rect,
    progress: f32,
    parent: Option<egui::LayerId>,
) -> (Response, Option<egui::Pos2>) {
    let area = egui::Area::new("compact-seek-bar".into());
    if let Some(parent) = parent {
        context.set_sublayer(parent, area.layer());
    }
    area.order(egui::Order::Middle)
        .movable(false)
        .fixed_pos(status.left_top() - egui::vec2(0.0, 6.0))
        .constrain(false)
        .show(context, |ui| {
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(status.width(), 12.0),
                egui::Sense::click_and_drag(),
            );
            let drag = timeline_input::seek_drag(&response);
            let commit = if drag.released { drag.position } else { None };
            let dragging = drag.dragging && !drag.released;
            let active = response.hovered() || dragging;
            let progress = if dragging {
                drag.position.map_or(progress, |p| ratio(rect, p.x))
            } else {
                progress
            };
            let height = if active {
                4.0
            } else {
                1.0 / context.pixels_per_point()
            };
            let track = Rect::from_center_size(rect.center(), egui::vec2(rect.width(), height));
            let x = egui::lerp(rect.x_range(), progress.clamp(0.0, 1.0));
            ui.painter().rect_filled(track, 0.0, Color32::from_gray(55));
            ui.painter().rect_filled(
                Rect::from_min_max(track.min, egui::pos2(x, track.bottom())),
                0.0,
                Color32::from_gray(190),
            );
            if active {
                ui.painter().circle_filled(
                    egui::pos2(x, rect.center().y),
                    4.0,
                    Color32::from_gray(230),
                );
            }
            (
                response.on_hover_cursor(egui::CursorIcon::PointingHand),
                commit,
            )
        })
        .inner
}

pub fn ratio(rect: Rect, x: f32) -> f32 {
    ((x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0)
}

pub fn item_index(ratio: f32, count: usize) -> usize {
    (ratio.clamp(0.0, 1.0) * count.saturating_sub(1) as f32).round() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seek_coordinates_clamp_and_cover_first_and_last_folder_items() {
        let rect = Rect::from_min_max(egui::pos2(20.0, 10.0), egui::pos2(220.0, 22.0));
        assert_eq!(ratio(rect, -20.0), 0.0);
        assert_eq!(ratio(rect, 120.0), 0.5);
        assert_eq!(ratio(rect, 300.0), 1.0);
        assert_eq!(item_index(0.0, 5), 0);
        assert_eq!(item_index(0.5, 5), 2);
        assert_eq!(item_index(1.0, 5), 4);
        assert_eq!(item_index(1.0, 1), 0);
    }
}
