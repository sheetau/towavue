use egui::{Color32, Context, Rect, Response};

fn gesture_id() -> egui::Id {
    egui::Id::new("seek-gesture")
}

pub fn is_active(context: &Context) -> bool {
    context.data(|data| data.get_temp::<egui::Id>(gesture_id()).is_some())
}

pub fn cancel(context: &Context) -> bool {
    let active = context.data_mut(|data| {
        let active = data.get_temp::<egui::Id>(gesture_id());
        data.remove::<egui::Id>(gesture_id());
        active
    });
    if active.is_some() && context.dragged_id() == active {
        context.stop_dragging();
    }
    active.is_some()
}

pub fn commit_position(response: &Response) -> Option<egui::Pos2> {
    let context = &response.ctx;
    let (pressed, release, interrupted) = context.input(|input| {
        (
            input.pointer.button_pressed(egui::PointerButton::Primary),
            input.events.iter().rev().find_map(|event| match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } => Some(*pos),
                _ => None,
            }),
            !input.focused
                || input.key_pressed(egui::Key::Escape)
                || input.events.contains(&egui::Event::WindowFocused(false)),
        )
    });
    if interrupted || !response.enabled() || egui::Popup::is_any_open(context) {
        cancel(context);
        return None;
    }
    if pressed
        && (response.is_pointer_button_down_on()
            || response.clicked_by(egui::PointerButton::Primary))
    {
        context.data_mut(|data| data.insert_temp(gesture_id(), response.id));
    }
    let owned = context.data(|data| data.get_temp::<egui::Id>(gesture_id())) == Some(response.id);
    let commit = (owned
        && release.is_some()
        && (response.clicked() || response.drag_stopped_by(egui::PointerButton::Primary)))
        || (response.clicked() && release.is_none());
    if owned && release.is_some() {
        context.data_mut(|data| data.remove::<egui::Id>(gesture_id()));
    }
    commit
        .then(|| {
            release
                .or(response.interact_pointer_pos())
                .or(response.hover_pos())
        })
        .flatten()
}

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
            let commit = commit_position(&response);
            let dragging = response.dragged_by(egui::PointerButton::Primary) && is_active(context);
            let active = response.hovered() || dragging;
            let progress = if dragging {
                response
                    .interact_pointer_pos()
                    .map_or(progress, |p| ratio(rect, p.x))
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
