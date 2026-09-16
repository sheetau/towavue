use crate::timeline_input;
use egui::{Context, Rect, Response};

#[cfg(test)]
pub fn show(
    context: &Context,
    status: Rect,
    progress: f32,
    parent: Option<egui::LayerId>,
    enabled: bool,
    video: bool,
) -> (Response, Option<egui::Pos2>, bool) {
    let (response, drag) = show_drag(context, status, progress, parent, enabled, video);
    let commit = if drag.released { drag.position } else { None };
    (response, commit, drag.open_timeline)
}

pub fn show_drag(
    context: &Context,
    status: Rect,
    progress: f32,
    parent: Option<egui::LayerId>,
    enabled: bool,
    video: bool,
) -> (Response, timeline_input::Drag) {
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
            let (_, response) = ui.allocate_exact_size(
                egui::vec2(status.width(), 12.0),
                egui::Sense::click_and_drag(),
            );
            show_control(ui, response, progress, video)
        })
        .inner
}

/// The same control within an existing card layer, without a separate Area or preview.
pub fn inline(
    ui: &mut egui::Ui,
    rect: Rect,
    id: egui::Id,
    progress: f32,
) -> (Response, timeline_input::Drag) {
    let response = ui.interact(rect, id, egui::Sense::click_and_drag());
    show_control(ui, response, progress, false)
}

fn show_control(
    ui: &egui::Ui,
    response: Response,
    progress: f32,
    video: bool,
) -> (Response, timeline_input::Drag) {
    let context = ui.ctx();
    let rect = response.rect;
    let enabled = response.enabled();
    let drag = if video {
        timeline_input::video_seek_drag(&response)
    } else {
        timeline_input::seek_drag(&response)
    };
    // The app applies the committed seek after painting. Keep its position
    // for every pass of this release frame, without replaying the action.
    let frame = context.cumulative_frame_nr();
    let release = context.data_mut(|data| {
        let id = response.id.with("release-progress");
        if drag.released
            && let Some(position) = drag.position
        {
            data.insert_temp(id, (frame, compact_ratio(rect, position.x)));
        }
        data.get_temp::<(u64, f32)>(id)
            .filter(|(saved, _)| *saved == frame && enabled)
            .map(|(_, progress)| progress)
    });
    let preview = release.or_else(|| {
        drag.dragging
            .then(|| drag.position.map(|p| compact_ratio(rect, p.x)))
            .flatten()
    });
    let active = response.hovered() || response.has_focus() || preview.is_some();
    let progress = preview.unwrap_or(progress);
    let height = if active {
        4.0
    } else {
        1.0 / context.pixels_per_point()
    };
    let travel = if active { compact_travel(rect) } else { rect };
    // Only the handle's center is inset. Track/progress keep their full
    // width when hovered so expanding the bar does not shorten its ends.
    let track = Rect::from_center_size(rect.center(), egui::vec2(rect.width(), height));
    let x = egui::lerp(travel.x_range(), progress.clamp(0.0, 1.0));
    let progress_x = egui::lerp(rect.x_range(), progress.clamp(0.0, 1.0));
    ui.painter().rect_filled(
        track,
        0.0,
        if active {
            crate::chrome::HOVER
        } else {
            crate::chrome::BORDER
        },
    );
    if response.hovered()
        && let Some(pointer) = response.hover_pos()
    {
        ui.painter().rect_filled(
            Rect::from_min_max(
                track.min,
                egui::pos2(pointer.x.clamp(track.left(), track.right()), track.bottom()),
            ),
            0.0,
            egui::Color32::from_white_alpha(64),
        );
    }
    ui.painter().rect_filled(
        Rect::from_min_max(track.min, egui::pos2(progress_x, track.bottom())),
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
        drag,
    )
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
    if enabled {
        crate::tab_focus::observe_pointer_control(
            response,
            ("media-value", response.layer_id.id, label),
        );
    }
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

pub fn preview_tooltip(response: &Response, ratio: f32) -> crate::media_preview::Preview {
    crate::media_preview::Preview::seek(response, ratio)
}

pub fn item_index(ratio: f32, count: usize) -> usize {
    (ratio.clamp(0.0, 1.0) * count.saturating_sub(1) as f32).round() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_progress_is_between_background_and_playback_without_committing() {
        for density in [1.0, 1.25, 2.0] {
            for value in [0.0, 0.25, 1.0] {
                for (pointer, enabled, hover) in [
                    (egui::pos2(0.0, 270.0), true, true),
                    (egui::pos2(350.0, 270.0), true, true),
                    (egui::pos2(500.0, 270.0), true, true),
                    (egui::pos2(350.0, 200.0), true, false),
                    (egui::pos2(350.0, 270.0), false, false),
                ] {
                    let context = Context::default();
                    context.set_pixels_per_point(density);
                    let status =
                        Rect::from_min_max(egui::pos2(0.0, 270.0), egui::pos2(500.0, 300.0));
                    let mut output = egui::FullOutput::default();
                    for frame in 0..3 {
                        output = context.run_ui(
                            egui::RawInput {
                                time: Some(f64::from(frame)),
                                screen_rect: Some(Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(500.0, 300.0),
                                )),
                                events: vec![egui::Event::PointerMoved(pointer)],
                                ..Default::default()
                            },
                            |_| {
                                let (_, commit, open) =
                                    show(&context, status, value, None, enabled, true);
                                assert!(commit.is_none() && !open);
                            },
                        );
                    }
                    let rectangles: Vec<_> = output
                        .shapes
                        .iter()
                        .filter_map(|shape| match &shape.shape {
                            egui::Shape::Rect(rect) => Some(rect),
                            _ => None,
                        })
                        .collect();
                    let preview = rectangles
                        .iter()
                        .position(|rect| rect.fill == egui::Color32::from_white_alpha(64));
                    assert_eq!(
                        preview.is_some(),
                        hover,
                        "pointer {pointer:?}, enabled {enabled}, density {density}, value {value}"
                    );
                    if let Some(index) = preview {
                        let track = rectangles[index - 1];
                        let preview = rectangles[index];
                        let played = rectangles[index + 1];
                        assert_eq!(track.fill, crate::chrome::HOVER);
                        assert_eq!(track.rect.x_range(), status.x_range());
                        assert_eq!(played.fill, crate::chrome::FOREGROUND);
                        assert_eq!(preview.rect.left(), track.rect.left());
                        assert_eq!(
                            preview.rect.right(),
                            pointer.x.clamp(track.rect.left(), track.rect.right())
                        );
                        assert_eq!(preview.rect.y_range(), track.rect.y_range());
                        assert_eq!(played.rect.right(), 500.0 * value);
                        assert_eq!(played.rect.y_range(), preview.rect.y_range());
                    }
                }
            }
        }
    }

    #[test]
    fn seek_drag_keeps_the_thumbnail_and_time_visible_without_hover() {
        for video in [false, true] {
            let context = crate::fonts::test_context();
            let screen = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 300.0));
            let status = Rect::from_min_size(egui::pos2(0.0, 270.0), egui::vec2(500.0, 30.0));
            let start = egui::pos2(80.0, 270.0);
            let end = egui::pos2(360.0, 200.0);
            let button = |position, pressed| egui::Event::PointerButton {
                pos: position,
                pressed,
                button: egui::PointerButton::Primary,
                modifiers: egui::Modifiers::NONE,
            };
            let frame = |events, enabled| {
                let mut visible = false;
                let mut commit = None;
                let _ = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        ..Default::default()
                    },
                    |_| {
                        let (response, drag) =
                            show_drag(&context, status, 0.2, None, enabled, video);
                        let mut other = response.clone();
                        other.id = response.id.with("unrelated-widget");
                        assert!(!timeline_input::is_dragging(&other));
                        if drag.released {
                            commit = drag.position;
                        }
                        visible = preview_tooltip(&response, 0.7)
                            .show(|ui| {
                                ui.allocate_space(egui::vec2(160.0, 90.0));
                                ui.monospace("00:21");
                            })
                            .is_some();
                    },
                );
                (visible, commit)
            };
            frame(vec![], true);
            frame(vec![], true);
            frame(
                vec![egui::Event::PointerMoved(start), button(start, true)],
                true,
            );
            assert!(
                !frame(vec![], true).0,
                "press alone does not force a tooltip"
            );
            assert!(
                frame(vec![egui::Event::PointerMoved(end)], true).0,
                "owned drag must show thumbnail and time outside the track"
            );
            assert!(frame(vec![], true).0, "holding the drag keeps the preview");
            assert!(
                !frame(vec![], false).0,
                "disabled input cancels the drag preview"
            );
            assert_eq!(frame(vec![button(end, false)], true).1, None);
        }
    }

    #[test]
    fn release_paints_the_committed_position_across_discarded_passes() {
        for density in [1.0, 1.25, 2.0] {
            for video in [false, true] {
                for discard in [false, true] {
                    for batched in [false, true] {
                        let context = Context::default();
                        let screen =
                            Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 300.0));
                        let status =
                            Rect::from_min_size(egui::pos2(0.0, 270.0), egui::vec2(500.0, 30.0));
                        let start = egui::pos2(100.0, 270.0);
                        let end = egui::pos2(400.0, 270.0);
                        let button = |pos, pressed| egui::Event::PointerButton {
                            pos,
                            pressed,
                            button: egui::PointerButton::Primary,
                            modifiers: egui::Modifiers::NONE,
                        };
                        let frame = |events, progress, discard| {
                            let mut commits = Vec::new();
                            let mut input = egui::RawInput {
                                time: Some(context.cumulative_frame_nr() as f64),
                                screen_rect: Some(screen),
                                events,
                                ..Default::default()
                            };
                            input
                                .viewports
                                .get_mut(&egui::ViewportId::ROOT)
                                .expect("viewport")
                                .native_pixels_per_point = Some(density);
                            let output = context.run_ui(input, |_| {
                                let (_, commit, open) =
                                    show(&context, status, progress, None, true, video);
                                assert!(!open);
                                commits.extend(commit);
                                if discard && context.current_pass_index() == 0 {
                                    context.request_discard(
                                        "seek release paint survives another pass",
                                    );
                                }
                            });
                            assert_eq!(output.pixels_per_point, density);
                            (commits, output)
                        };
                        frame(vec![], 0.2, false);
                        frame(vec![], 0.2, false);
                        let events = if batched {
                            vec![
                                egui::Event::PointerMoved(end),
                                button(end, true),
                                button(end, false),
                            ]
                        } else {
                            assert!(
                                frame(
                                    vec![egui::Event::PointerMoved(start), button(start, true)],
                                    0.2,
                                    false
                                )
                                .0
                                .is_empty()
                            );
                            assert!(
                                frame(vec![egui::Event::PointerMoved(end)], 0.2, false)
                                    .0
                                    .is_empty()
                            );
                            vec![button(end, false)]
                        };
                        let (commits, output) = frame(events, 0.2, discard);
                        assert_eq!(commits, vec![end], "one committed action");
                        let assert_position = |output: &egui::FullOutput, progress: f32| {
                            let circle = output
                                .shapes
                                .iter()
                                .find_map(|shape| match &shape.shape {
                                    egui::Shape::Circle(circle) => Some(circle),
                                    _ => None,
                                })
                                .expect("seek handle");
                            assert!(
                                (circle.center.x - (4.0 + 492.0 * progress)).abs() < 0.001,
                                "release must not paint the old transport position"
                            );
                            let played = output
                                .shapes
                                .iter()
                                .find_map(|shape| match &shape.shape {
                                    egui::Shape::Rect(rect)
                                        if rect.fill == crate::chrome::FOREGROUND =>
                                    {
                                        Some(rect)
                                    }
                                    _ => None,
                                })
                                .expect("played track");
                            assert!((played.rect.right() - 500.0 * progress).abs() < 0.001);
                        };
                        let committed = (end.x - 4.0) / 492.0;
                        assert_position(&output, committed);
                        let (commits, output) = frame(vec![], committed, false);
                        assert!(commits.is_empty());
                        assert_position(&output, committed);
                        let (commits, output) = frame(vec![], 0.2, false);
                        assert!(commits.is_empty());
                        assert_position(&output, 0.2);
                    }
                }
            }
        }
    }

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
                            show(&context, status, value, None, true, false);
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
                    let (response, commit, _) = show(&context, status, 0.25, None, true, false);
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
