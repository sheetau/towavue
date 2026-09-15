use egui::{Context, Event, Id, RawInput, Vec2};
use towavue_runtime_windows::{WheelScrollSettings, wheel_scroll_settings};

#[derive(Clone)]
struct NativeFrame {
    frame: u64,
    events: std::sync::Arc<[Event]>,
    settings: WheelScrollSettings,
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
    for event in &mut input.events {
        normalize(event, settings, speed, extent);
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
