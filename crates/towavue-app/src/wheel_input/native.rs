use egui::{Context, Event, Id, RawInput, Vec2};
use towavue_runtime_windows::{WheelScrollSettings, wheel_scroll_settings};

#[derive(Clone)]
struct NativeFrame {
    frame: u64,
    events: std::sync::Arc<[Event]>,
    settings: WheelScrollSettings,
}

#[derive(Clone)]
struct ScrollArea {
    id: Id,
    layer: egui::LayerId,
    bounds: egui::Rect,
    size: Vec2,
    axes: [bool; 2],
    only_direction: bool,
    offset: Vec2,
    maximum: Vec2,
}

#[derive(Clone, Default)]
struct ScrollAreas {
    frame: u64,
    pass: u64,
    areas: Vec<ScrollArea>,
}

pub fn record_scroll_area<R>(ui: &egui::Ui, output: &egui::scroll_area::ScrollAreaOutput<R>) {
    let context = ui.ctx();
    let mut bounds = output.inner_rect;
    let axes = std::array::from_fn(|axis| {
        // The pinned egui adapter already uses these axis IDs for scrollbar input.
        context
            .read_response(output.id.with(axis))
            .is_some_and(|bar| {
                bounds = bounds.union(bar.rect);
                bar.enabled() && output.content_size[axis] > output.inner_rect.size()[axis]
            })
    });
    let area = ScrollArea {
        id: output.id,
        layer: ui.layer_id(),
        bounds: bounds.intersect(ui.clip_rect()),
        size: output.inner_rect.size(),
        axes,
        only_direction: ui.style().always_scroll_the_only_direction && axes[0] != axes[1],
        offset: output.state.offset,
        maximum: (output.content_size - output.inner_rect.size()).max(Vec2::ZERO),
    };
    let frame = context.cumulative_frame_nr();
    let pass = context.cumulative_pass_nr();
    context.data_mut(|data| {
        let recorded = data.get_temp_mut_or_default::<ScrollAreas>(Id::new("native-scroll-areas"));
        if recorded.frame != frame || recorded.pass != pass {
            recorded.frame = frame;
            recorded.pass = pass;
            recorded.areas.clear();
        }
        recorded.areas.retain(|previous| previous.id != area.id);
        if ui.is_enabled() && axes.iter().any(|enabled| *enabled) {
            // Children finish before parents, so nested areas get first refusal.
            recorded.areas.push(area);
        }
    });
}

fn target_size(
    areas: &[ScrollArea],
    position: Option<egui::Pos2>,
    layer: Option<egui::LayerId>,
    event: &Event,
) -> Option<Vec2> {
    let position = position?;
    let Event::MouseWheel {
        delta, modifiers, ..
    } = event
    else {
        return None;
    };
    let delta = if modifiers.shift {
        egui::vec2(delta.x + delta.y, 0.0)
    } else {
        *delta
    };
    areas.iter().find_map(|area| {
        if Some(area.layer) != layer || !area.bounds.contains(position) {
            return None;
        }
        let can_scroll = (0..2).any(|axis| {
            let delta = if area.only_direction {
                delta.x + delta.y
            } else {
                delta[axis]
            };
            area.axes[axis]
                && ((delta > 0.0 && area.offset[axis] > 0.0)
                    || (delta < 0.0 && area.offset[axis] < area.maximum[axis]))
        });
        can_scroll.then(|| {
            if area.only_direction {
                Vec2::splat(if area.axes[0] {
                    area.size.x
                } else {
                    area.size.y
                })
            } else {
                area.size
            }
        })
    })
}

pub fn prepare_native_input(context: &Context, input: &mut RawInput) {
    let settings = input
        .events
        .iter()
        .any(|event| {
            matches!(
                event,
                Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    ..
                }
            )
        })
        .then(wheel_scroll_settings);
    prepare(context, input, settings);
}

fn prepare(context: &Context, input: &mut RawInput, settings: Option<WheelScrollSettings>) {
    let id = Id::new("native-wheel-frame");
    context.data_mut(|data| data.remove::<NativeFrame>(id));
    let Some(settings) = settings else { return };
    // Keep the same event order/coordinates for media gestures. OS scroll settings
    // must not change zoom or volume, including when scrolling is disabled.
    let frame = context.cumulative_frame_nr();
    context.data_mut(|data| {
        data.insert_temp(
            id,
            NativeFrame {
                frame,
                events: input.events.as_slice().into(),
                settings,
            },
        )
    });
    let extent = input
        .screen_rect
        .unwrap_or_else(|| context.viewport_rect())
        .size();
    let speed = context.options(|options| options.input_options.line_scroll_speed);
    let areas = context
        .data(|data| data.get_temp::<ScrollAreas>(Id::new("native-scroll-areas")))
        .filter(|areas| {
            areas.frame.checked_add(1) == Some(frame)
                && areas.pass.checked_add(1) == Some(context.cumulative_pass_nr())
        })
        .map_or_else(Vec::new, |areas| areas.areas);
    let mut position = context.input(|input| input.pointer.hover_pos());
    for event in &mut input.events {
        match event {
            Event::PointerMoved(point) | Event::PointerButton { pos: point, .. } => {
                position = Some(*point)
            }
            Event::PointerGone | Event::WindowFocused(false) => position = None,
            _ => {}
        }
        if !matches!(
            event,
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                ..
            }
        ) {
            continue;
        }
        let layer = position.and_then(|point| context.layer_id_at(point));
        let target = target_size(&areas, position, layer, event).unwrap_or(extent);
        normalize(event, settings, speed, target);
    }
}

fn current(context: &Context) -> Option<NativeFrame> {
    let frame = context.cumulative_frame_nr();
    context
        .data(|data| data.get_temp::<NativeFrame>(Id::new("native-wheel-frame")))
        .filter(|native| native.frame == frame)
}

pub(super) fn original_events(context: &Context) -> Vec<Event> {
    current(context).map_or_else(
        || context.input(|input| input.events.clone()),
        |native| native.events.to_vec(),
    )
}

pub(super) fn scroll_event(context: &Context, mut event: Event, extent: Vec2) -> Event {
    if let Some(native) = current(context) {
        let speed = context.options(|options| options.input_options.line_scroll_speed);
        normalize(&mut event, native.settings, speed, extent);
    }
    event
}

fn normalize(event: &mut Event, settings: WheelScrollSettings, speed: f32, extent: Vec2) {
    let Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta,
        modifiers,
        ..
    } = event
    else {
        return;
    };
    if modifiers.ctrl || modifiers.alt || modifiers.mac_cmd {
        return;
    }
    // egui's native default is one 40-point line per detent. Treat that as the
    // Windows default three lines, retaining the existing baseline and smoothing.
    let factor = |count: u32, page: f32| {
        if speed <= 0.0 {
            return 0.0;
        }
        if count == u32::MAX {
            page / speed
        } else {
            (count as f32 / 3.0).min(page / speed)
        }
    };
    delta.x *= factor(settings.characters, extent.x);
    delta.y *= factor(
        settings.lines,
        if modifiers.shift { extent.x } else { extent.y },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wheel(delta: Vec2, modifiers: egui::Modifiers) -> Event {
        Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta,
            phase: egui::TouchPhase::Move,
            modifiers,
        }
    }

    #[test]
    fn native_pages_follow_rendered_scroll_areas_and_ignore_discarded_or_missing_layout() {
        use crate::scroll_style::ScrollAreaStyle;
        for density in [1.0, 1.25, 2.0] {
            for (kind, horizontal) in [
                (0, false),
                (0, true),
                (1, false),
                (2, false),
                (2, true),
                (3, false),
            ] {
                let context = egui::Context::default();
                context.set_pixels_per_point(density);
                let mut time = 0.0;
                let mut frame = |events: Vec<Event>, size: Vec2, visible: bool, discard: bool| {
                    time += 0.016;
                    let has_wheel = events
                        .iter()
                        .any(|event| matches!(event, Event::MouseWheel { .. }));
                    let mut input = RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(500.0, 400.0),
                        )),
                        events,
                        time: Some(time),
                        ..Default::default()
                    };
                    prepare(
                        &context,
                        &mut input,
                        has_wheel.then_some(WheelScrollSettings {
                            lines: u32::MAX,
                            characters: u32::MAX,
                        }),
                    );
                    let mut area_output = None;
                    let _ = context.run_ui(input, |ui| {
                        ui.style_mut().always_scroll_the_only_direction = true;
                        if visible || (discard && context.current_pass_index() == 0) {
                            let area = egui::ScrollArea::new([horizontal, !horizontal])
                                .max_width(size.x)
                                .max_height(size.y)
                                .auto_shrink([false, false]);
                            let content = |ui: &mut egui::Ui| {
                                // Only the enabled axis overflows; otherwise egui can
                                // place the bar beyond this test window's visible clip.
                                ui.set_min_size(if horizontal {
                                    egui::vec2(1600.0, size.y)
                                } else {
                                    egui::vec2(size.x, 1600.0)
                                });
                            };
                            area_output = Some(match kind {
                                0 => area.show_styled(ui, content),
                                1 => area.show_rows_styled(ui, 20.0, 80, |ui, _| content(ui)),
                                2 => area.show_viewport_styled(ui, |ui, _| content(ui)),
                                3 => {
                                    let mut output = area
                                        .scroll_bar_visibility(
                                            egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                                        )
                                        .show_styled(ui, |ui| {
                                            content(ui);
                                            vec![crate::gallery_rail::Month {
                                                date: Some((2026, 9)),
                                                offset: 0.0,
                                            }]
                                        });
                                    let mut rail = output.inner_rect;
                                    rail.min.x = rail.right() - 32.0;
                                    crate::gallery_rail::show(ui, &mut output, rail);
                                    egui::scroll_area::ScrollAreaOutput {
                                        inner: (),
                                        id: output.id,
                                        state: output.state,
                                        content_size: output.content_size,
                                        inner_rect: output.inner_rect,
                                    }
                                }
                                _ => unreachable!(),
                            });
                        }
                        if discard && context.current_pass_index() == 0 {
                            context.request_discard("scroll area layout test");
                        }
                    });
                    area_output
                };
                let axis = usize::from(!horizontal);
                for size in [egui::vec2(220.0, 130.0), egui::vec2(180.0, 90.0)] {
                    let mut area = frame(vec![], size, true, true).expect("area");
                    for _ in 0..3 {
                        area = frame(vec![], size, true, false).expect("area");
                    }
                    let before = area.state.offset[axis];
                    let page = area.inner_rect.size()[axis];
                    frame(
                        vec![
                            Event::PointerMoved(area.inner_rect.center()),
                            wheel(egui::vec2(0.0, -1.0), egui::Modifiers::NONE),
                        ],
                        size,
                        true,
                        true,
                    );
                    for _ in 0..100 {
                        area = frame(vec![], size, true, false).expect("area");
                    }
                    assert!(
                        (area.state.offset[axis] - before - page).abs() < 0.01,
                        "kind {kind}, horizontal {horizontal}, density {density}: {} vs {page}",
                        area.state.offset[axis] - before
                    );
                    let before = area.state.offset[axis];
                    let gutter = context
                        .read_response(area.id.with(axis))
                        .expect("scrollbar")
                        .rect
                        .center();
                    frame(
                        vec![
                            Event::PointerMoved(gutter),
                            wheel(egui::vec2(0.0, -0.5), egui::Modifiers::NONE),
                        ],
                        size,
                        true,
                        false,
                    );
                    for _ in 0..100 {
                        area = frame(vec![], size, true, false).expect("area");
                    }
                    assert!(
                        (area.state.offset[axis] - before - page * 0.5).abs() < 0.01,
                        "gutter kind {kind}, horizontal {horizontal}, density {density}: {} vs {}, at {gutter:?}",
                        area.state.offset[axis] - before,
                        page * 0.5
                    );
                }
                // A first-pass-only area is not part of the completed layout.
                frame(vec![], egui::vec2(180.0, 90.0), false, true);
                let mut input = RawInput {
                    screen_rect: Some(context.viewport_rect()),
                    events: vec![wheel(egui::vec2(0.0, -1.0), egui::Modifiers::NONE)],
                    ..Default::default()
                };
                prepare(
                    &context,
                    &mut input,
                    Some(WheelScrollSettings {
                        lines: u32::MAX,
                        characters: u32::MAX,
                    }),
                );
                let Event::MouseWheel { delta, .. } = input.events[0] else {
                    panic!("wheel")
                };
                assert_eq!(
                    delta.y, -10.0,
                    "removed layout falls back to the 400-point viewport"
                );
            }
        }
    }

    #[test]
    fn scroll_target_respects_layer_gutter_nesting_and_directional_edges() {
        let layer = egui::LayerId::background();
        let outer = ScrollArea {
            id: Id::new("outer"),
            layer,
            bounds: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0)),
            size: egui::vec2(390.0, 300.0),
            axes: [false, true],
            only_direction: true,
            offset: Vec2::ZERO,
            maximum: egui::vec2(0.0, 1000.0),
        };
        let mut inner = ScrollArea {
            id: Id::new("inner"),
            bounds: egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(200.0, 100.0)),
            size: egui::vec2(190.0, 100.0),
            ..outer.clone()
        };
        let position = Some(egui::pos2(215.0, 50.0)); // scrollbar gutter
        let event = wheel(egui::vec2(0.0, -1.0), egui::Modifiers::NONE);
        assert_eq!(
            target_size(
                &[inner.clone(), outer.clone()],
                position,
                Some(layer),
                &event
            ),
            Some(Vec2::splat(100.0))
        );
        inner.offset.y = inner.maximum.y;
        assert_eq!(
            target_size(
                &[inner.clone(), outer.clone()],
                position,
                Some(layer),
                &event
            ),
            Some(Vec2::splat(300.0))
        );
        assert_eq!(
            target_size(
                &[inner, outer],
                position,
                Some(egui::LayerId::new(
                    egui::Order::Foreground,
                    Id::new("popup")
                )),
                &event
            ),
            None
        );
    }

    #[test]
    fn native_scroll_settings_keep_units_modifiers_axes_and_fractional_detents() {
        let extent = egui::vec2(600.0, 300.0);
        for (lines, expected) in [
            (0, 0.0),
            (1, 1.0 / 3.0),
            (3, 1.0),
            (9, 3.0),
            (100, 6.0),
            (u32::MAX, 6.0),
        ] {
            let settings = WheelScrollSettings {
                lines,
                characters: 6,
            };
            for delta in [egui::vec2(0.25, -0.5), egui::vec2(-1.0, 2.0)] {
                let mut event = wheel(delta, egui::Modifiers::NONE);
                normalize(&mut event, settings, 50.0, extent);
                assert_eq!(
                    event,
                    wheel(
                        egui::vec2(delta.x * 2.0, delta.y * expected),
                        egui::Modifiers::NONE
                    )
                );
                for modifiers in [
                    egui::Modifiers::CTRL,
                    egui::Modifiers::ALT,
                    egui::Modifiers::MAC_CMD,
                ] {
                    let original = wheel(delta, modifiers);
                    let mut event = original.clone();
                    normalize(&mut event, settings, 50.0, extent);
                    assert_eq!(event, original);
                }
                for unit in [egui::MouseWheelUnit::Point, egui::MouseWheelUnit::Page] {
                    let mut event = wheel(delta, egui::Modifiers::NONE);
                    if let Event::MouseWheel { unit: actual, .. } = &mut event {
                        *actual = unit;
                    }
                    let original = event.clone();
                    normalize(&mut event, settings, 50.0, extent);
                    assert_eq!(event, original);
                }
            }
        }
        let mut event = wheel(egui::vec2(1.0, -0.5), egui::Modifiers::SHIFT);
        normalize(
            &mut event,
            WheelScrollSettings {
                lines: u32::MAX,
                characters: u32::MAX,
            },
            50.0,
            extent,
        );
        assert_eq!(event, wheel(egui::vec2(12.0, -6.0), egui::Modifiers::SHIFT));
    }

    #[test]
    fn native_frame_preserves_volume_zoom_and_target_scroll_without_replaying_raw_input() {
        for density in [1.0, 1.25, 2.0] {
            let context = egui::Context::default();
            context.set_pixels_per_point(density);
            let rect = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(200.0, 120.0));
            let mut scroll = crate::wheel_input::Scroll::default();
            let mut time = 0.0;
            let mut frame = |events: Vec<Event>, settings, discard: bool| {
                time += 0.016;
                let mut input = RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 300.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                };
                prepare(&context, &mut input, settings);
                let mut result = (Vec::new(), Vec::new(), Vec2::ZERO, Vec2::ZERO);
                let _ = context.run_ui(input, |ui| {
                    let target =
                        ui.interact(rect, Id::new("native-wheel-target"), egui::Sense::hover());
                    result.0 = crate::wheel_input::volume_deltas(
                        &context,
                        std::slice::from_ref(&target),
                        None,
                    );
                    result.1 = crate::wheel_input::image_events(&context, &target);
                    if context.current_pass_index() == 0 {
                        result.2 += context.input(|input| input.smooth_scroll_delta());
                    }
                    result.3 += scroll.delta(ui, rect, true);
                    if discard && context.current_pass_index() == 0 {
                        context.request_discard("native wheel ownership");
                    }
                });
                result
            };
            for _ in 0..3 {
                frame(vec![Event::PointerMoved(rect.center())], None, false);
            }
            // Change settings between frames in one context, including disabled scrolling.
            for (lines, window_points, target_points) in [
                (0, 0.0, 0.0),
                (1, 40.0 / 3.0, 40.0 / 3.0),
                (3, 40.0, 40.0),
                (12, 160.0, 120.0),
                (u32::MAX, 300.0, 120.0),
            ] {
                let settings = Some(WheelScrollSettings {
                    lines,
                    characters: 3,
                });
                let result = frame(
                    vec![wheel(egui::vec2(0.0, -1.0), egui::Modifiers::NONE)],
                    settings,
                    true,
                );
                assert_eq!(result.0, vec![-1.0], "volume retains raw detents");
                assert_eq!(result.1.len(), 1);
                let crate::wheel_input::ViewWheel::Pan(pan) = result.1[0].1 else {
                    panic!("pan")
                };
                assert!((pan.y + target_points).abs() < 0.0001);
                let (mut global, mut local) = (result.2.y, result.3.y);
                for _ in 0..100 {
                    let idle = frame(vec![], None, false);
                    assert!(idle.0.is_empty() && idle.1.is_empty());
                    global += idle.2.y;
                    local += idle.3.y;
                }
                assert!(
                    (global + window_points).abs() < 0.001,
                    "egui scroll: {global} vs {window_points}"
                );
                assert!(
                    (local + target_points).abs() < 0.001,
                    "target scroll: {local} vs {target_points}"
                );
                let zoom = frame(
                    vec![wheel(egui::vec2(0.0, 1.0), egui::Modifiers::CTRL)],
                    settings,
                    false,
                );
                assert!(zoom.0.is_empty());
                let crate::wheel_input::ViewWheel::Zoom(factor) = zoom.1[0].1 else {
                    panic!("zoom")
                };
                let expected = context.options(|options| {
                    (options.input_options.scroll_zoom_speed
                        * options.input_options.line_scroll_speed)
                        .exp()
                });
                assert_eq!(factor, expected);
                for _ in 0..100 {
                    frame(vec![], None, false);
                }
            }
        }
    }
}
