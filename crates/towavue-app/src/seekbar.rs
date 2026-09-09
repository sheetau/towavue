use crate::timeline_input;
use egui::{Context, Rect, Response};

pub fn show(
    context: &Context,
    status: Rect,
    progress: f32,
    parent: Option<egui::LayerId>,
    enabled: bool,
) -> (Response, Option<egui::Pos2>) {
    let area = egui::Area::new("compact-seek-bar".into());
    if let Some(parent) = parent {
        context.set_sublayer(parent, area.layer());
    }
    area.order(egui::Order::Middle)
        .enabled(enabled)
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
            let active = response.hovered() || response.has_focus() || dragging;
            let progress = if dragging {
                drag.position.map_or(progress, |p| compact_ratio(rect, p.x))
            } else {
                progress
            };
            let height = if active {
                4.0
            } else {
                1.0 / context.pixels_per_point()
            };
            let travel = if active { compact_travel(rect) } else { rect };
            let track = Rect::from_center_size(rect.center(), egui::vec2(travel.width(), height));
            let x = egui::lerp(travel.x_range(), progress.clamp(0.0, 1.0));
            ui.painter().rect_filled(
                track,
                0.0,
                if active {
                    crate::chrome::HOVER
                } else {
                    crate::chrome::BORDER
                },
            );
            ui.painter().rect_filled(
                Rect::from_min_max(track.min, egui::pos2(x, track.bottom())),
                0.0,
                crate::chrome::FOREGROUND,
            );
            if active {
                ui.painter().circle_filled(
                    egui::pos2(x, rect.center().y),
                    compact_radius(rect),
                    crate::chrome::FOREGROUND,
                );
            }
            (
                response.on_hover_cursor(egui::CursorIcon::PointingHand),
                commit,
            )
        })
        .inner
}

pub fn value_input(
    response: &Response,
    label: &str,
    value: f64,
    range: std::ops::RangeInclusive<f64>,
    step: f64,
    enabled: bool,
) -> Option<f64> {
    use egui::accesskit::{Action, ActionData, Orientation, TreeId};
    if response.has_focus() {
        response
            .ctx
            .data_mut(|data| data.insert_temp(egui::Id::new("seek-value-control"), response.id));
    }
    let enabled = enabled && response.enabled() && !egui::Popup::is_any_open(&response.ctx);
    let value = value.clamp(*range.start(), *range.end());
    response.widget_info(|| egui::WidgetInfo::slider(enabled, value, label));
    response.ctx.accesskit_node_builder(response.id, |node| {
        node.set_orientation(Orientation::Horizontal);
        node.set_min_numeric_value(*range.start());
        node.set_max_numeric_value(*range.end());
        node.set_numeric_value_step(step);
        if enabled {
            node.add_action(Action::SetValue);
            if value < *range.end() {
                node.add_action(Action::Increment);
            }
            if value > *range.start() {
                node.add_action(Action::Decrement);
            }
        }
    });
    if !enabled {
        return None;
    }
    let focused = response.has_focus();
    if focused {
        response.ctx.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    ..Default::default()
                },
            )
        });
    }
    let mut target = value;
    let mut keyboard_value = false;
    response.ctx.input_mut(|input| {
        input.events.retain(|event| {
            let next = match event {
                egui::Event::AccessKitActionRequest(request)
                    if request.target_tree == TreeId::ROOT
                        && request.target_node == response.id.accesskit_id() =>
                {
                    match (&request.action, &request.data) {
                        (Action::SetValue, Some(ActionData::NumericValue(value)))
                            if value.is_finite() =>
                        {
                            *value
                        }
                        (Action::Increment, _) => target + step,
                        (Action::Decrement, _) => target - step,
                        _ => return true,
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if focused && *modifiers == egui::Modifiers::NONE => {
                    let next = match key {
                        egui::Key::ArrowLeft => target - step,
                        egui::Key::ArrowRight => target + step,
                        egui::Key::Home => *range.start(),
                        egui::Key::End => *range.end(),
                        _ => return true,
                    };
                    keyboard_value = true;
                    next
                }
                _ => return true,
            };
            target = next.clamp(*range.start(), *range.end());
            false
        })
    });
    if keyboard_value {
        // The first focused frame may not yet have installed egui's focus-lock filter.
        response
            .ctx
            .memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
    }
    if target == value {
        return None;
    }
    // A direct value change supersedes a pending pointer gesture, including its later release.
    timeline_input::cancel(&response.ctx);
    Some(target)
}

pub fn has_value_focus(context: &Context) -> bool {
    context
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new("seek-value-control")))
        .is_some_and(|id| context.memory(|memory| memory.has_focus(id)))
}

pub fn ratio(rect: Rect, x: f32) -> f32 {
    ((x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0)
}

fn compact_radius(rect: Rect) -> f32 {
    (rect.width() * 0.5).clamp(0.0, 4.0)
}

fn compact_travel(rect: Rect) -> Rect {
    rect.shrink2(egui::vec2(compact_radius(rect), 0.0))
}

pub fn compact_ratio(rect: Rect, x: f32) -> f32 {
    let travel = compact_travel(rect);
    ((x - travel.left()) / travel.width().max(f32::EPSILON)).clamp(0.0, 1.0)
}

pub fn preview_tooltip(response: &Response, ratio: f32) -> egui::Tooltip<'static> {
    let anchor = egui::pos2(
        egui::lerp(response.rect.x_range(), ratio),
        response.rect.top(),
    );
    let mut tooltip = egui::Tooltip::for_enabled(response)
        .width(160.0)
        .layout(egui::Layout::top_down(egui::Align::Center));
    tooltip.popup = tooltip
        .popup
        .at_position(anchor)
        .align(egui::RectAlign::TOP)
        .align_alternatives(&[]);
    tooltip
}

pub fn item_index(ratio: f32, count: usize) -> usize {
    (ratio.clamp(0.0, 1.0) * count.saturating_sub(1) as f32).round() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_endpoints_match_handle_centers_without_changing_timeline_coordinates() {
        let rect = Rect::from_min_max(egui::pos2(20.0, 10.0), egui::pos2(220.0, 22.0));
        for (x, expected) in [
            (0.0, 0.0),
            (20.0, 0.0),
            (24.0, 0.0),
            (120.0, 0.5),
            (216.0, 1.0),
            (220.0, 1.0),
            (240.0, 1.0),
        ] {
            assert_eq!(compact_ratio(rect, x), expected);
        }
        assert_eq!(ratio(rect, 24.0), 0.02);
        for width in [0.0, 1.0, 4.0, 8.0, 8.25, 8.5, 16.0, 500.0] {
            let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 12.0));
            let travel = compact_travel(rect);
            assert!(travel.width() >= 0.0);
            assert!(compact_ratio(rect, 0.0).is_finite());
            for value in [0.0, 0.5, 1.0] {
                let x = egui::lerp(travel.x_range(), value);
                assert!(x - compact_radius(rect) >= 0.0);
                assert!(x + compact_radius(rect) <= width);
                if travel.width() > 0.0 {
                    assert!((compact_ratio(rect, x) - value).abs() < 0.00001);
                }
            }
        }
    }

    #[test]
    fn compact_handle_stays_inside_while_idle_progress_uses_full_width() {
        for density in [1.0, 1.25, 2.0] {
            for value in [0.0, 0.5, 1.0] {
                for active in [false, true] {
                    let context = Context::default();
                    let status =
                        Rect::from_min_max(egui::pos2(0.0, 270.0), egui::pos2(500.0, 300.0));
                    let mut output = egui::FullOutput::default();
                    for frame in 0..3 {
                        let mut input = egui::RawInput {
                            time: Some(frame as f64),
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(500.0, 300.0),
                            )),
                            events: vec![egui::Event::PointerMoved(egui::pos2(
                                250.0,
                                if active { 270.0 } else { 100.0 },
                            ))],
                            ..Default::default()
                        };
                        input
                            .viewports
                            .get_mut(&egui::ViewportId::ROOT)
                            .expect("viewport")
                            .native_pixels_per_point = Some(density);
                        output = context.run_ui(input, |_| {
                            show(&context, status, value, None, true);
                        });
                    }
                    let circle = output.shapes.iter().find_map(|shape| match &shape.shape {
                        egui::Shape::Circle(circle) => Some(circle),
                        _ => None,
                    });
                    if active {
                        let circle = circle.expect("hover handle");
                        assert_eq!(circle.center.x, 4.0 + 492.0 * value);
                        assert!(circle.center.x - circle.radius >= 0.0);
                        assert!(circle.center.x + circle.radius <= 500.0);
                        let hit =
                            Rect::from_min_max(egui::pos2(0.0, 264.0), egui::pos2(500.0, 276.0));
                        assert!((compact_ratio(hit, circle.center.x) - value).abs() < 0.00001);
                    } else {
                        assert!(circle.is_none());
                        let track = output
                            .shapes
                            .iter()
                            .find_map(|shape| match &shape.shape {
                                egui::Shape::Rect(rect) if rect.fill == crate::chrome::BORDER => {
                                    Some(rect.rect)
                                }
                                _ => None,
                            })
                            .expect("idle track");
                        assert_eq!(track.x_range(), status.x_range());
                        assert!((track.height() * density - 1.0).abs() < 0.0001);
                    }
                }
            }
        }
    }

    #[test]
    fn direct_value_cancels_pending_pointer_release() {
        let context = Context::default();
        context.enable_accesskit();
        let status = Rect::from_min_max(egui::pos2(0.0, 270.0), egui::pos2(500.0, 300.0));
        let frame = |events| {
            let mut result = None;
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(500.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |_| {
                    let (response, commit) = show(&context, status, 0.25, None, true);
                    let value = value_input(&response, "Position", 25.0, 0.0..=100.0, 5.0, true);
                    result = Some((response, commit, value));
                },
            );
            result.expect("seek response")
        };
        frame(vec![]);
        let (response, _, _) = frame(vec![]);
        let origin = response.rect.center();
        let button = |pressed| egui::Event::PointerButton {
            pos: origin,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(origin), button(true)]);
        assert!(timeline_input::is_active(&context));
        let (_, _, value) = frame(vec![egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::SetValue,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: response.id.accesskit_id(),
                data: Some(egui::accesskit::ActionData::NumericValue(50.0)),
            },
        )]);
        assert_eq!(value, Some(50.0));
        assert!(!timeline_input::is_active(&context));
        let (_, commit, value) = frame(vec![button(false)]);
        assert!(commit.is_none());
        assert!(value.is_none());
    }

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
