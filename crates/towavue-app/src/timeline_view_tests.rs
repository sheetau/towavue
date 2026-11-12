use super::*;

fn time(seconds: f64) -> MediaTime {
    MediaTime::from_nanoseconds((seconds * 1e9).round() as i64)
}

struct DragFixture {
    context: egui::Context,
    id: egui::Id,
    position: MediaTime,
    selection: Option<TimeRange>,
    fps: f64,
}

impl DragFixture {
    fn new(start: f64, span: f64, fps: f64) -> Self {
        let context = crate::fonts::test_context();
        context.global_style_mut(crate::chrome::style);
        let id = egui::Id::new("reviewed-drag");
        context.data_mut(|data| {
            data.insert_temp(
                id.with("time-viewport"),
                View {
                    start,
                    span,
                    duration: 600.0,
                },
            )
        });
        Self {
            context,
            id,
            position: time(start + span * 0.5),
            selection: None,
            fps,
        }
    }

    fn draw(
        &self,
        events: Vec<egui::Event>,
        shift: bool,
        repeat: bool,
    ) -> (Layout<'static>, crate::time_selection::Output, Vec<f32>) {
        let mut result = None;
        let output = self.context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 300.0))),
                events,
                modifiers: egui::Modifiers {
                    shift,
                    ..Default::default()
                },
                ..Default::default()
            },
            |ui| {
                let layout = show(
                    ui,
                    self.id,
                    Rect::from_min_max(egui::pos2(5.0, 100.0), egui::pos2(635.0, 220.0)),
                    time(600.0),
                    self.fps,
                    true,
                );
                let response =
                    ui.allocate_rect(layout.interaction(), egui::Sense::click_and_drag());
                let output = crate::time_selection::show_in_view(
                    ui,
                    &response,
                    time(600.0),
                    self.position,
                    self.selection,
                    None,
                    true,
                    layout,
                );
                paint(
                    ui,
                    layout,
                    self.fps,
                    output.display_position,
                    output.display_selection,
                );
                result = Some((layout, output));
                if repeat && self.context.current_pass_index() == 0 {
                    self.context.request_discard("verify one pan per frame");
                }
            },
        );
        let (layout, result) = result.expect("frame");
        let mut markers = Vec::new();
        for shape in output.shapes {
            if let egui::Shape::Vec(shapes) = shape.shape {
                for shape in shapes {
                    if let egui::Shape::LineSegment { points, stroke } = shape
                        && stroke.color == chrome::FOREGROUND
                        && points[0].y == layout.ruler.top()
                        && points[1].y == layout.ruler.bottom()
                    {
                        markers.push(points[0].x);
                    }
                }
            }
        }
        (layout, result, markers)
    }

    fn press(&self, point: Pos2, secondary: bool) -> Layout<'static> {
        for _ in 0..3 {
            self.draw(vec![egui::Event::PointerMoved(point)], false, false);
        }
        self.draw(vec![pointer_button(point, true, secondary)], false, false)
            .0
    }
}

fn pointer_button(pos: Pos2, pressed: bool, secondary: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        pressed,
        button: if secondary {
            egui::PointerButton::Secondary
        } else {
            egui::PointerButton::Primary
        },
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn frame_grid_handles_fractional_rates_and_maximum_zoom() {
    for fps in [30.0, 30000.0 / 1001.0] {
        let fixture = DragFixture::new(100.0, 640.0 / fps / 50.0, fps);
        let layout = fixture.draw(vec![], false, false).0;
        assert!((f64::from(layout.ruler.width()) / layout.view.span / fps - 50.0).abs() < 0.01);
        assert_eq!(
            tick_step(
                layout.view.span / f64::from(layout.ruler.width()) * 50.0,
                fps
            ),
            (1.0 / fps, true)
        );
        let origin = egui::pos2(320.0, layout.ruler.center().y);
        fixture.press(origin, false);
        let moved = fixture
            .draw(
                vec![egui::Event::PointerMoved(origin + egui::vec2(37.0, 0.0))],
                false,
                false,
            )
            .1;
        let frames = moved.display_position.as_seconds_f64() * fps;
        assert!((frames - frames.round()).abs() < 1e-6);
        let last = layout.quantize(time(600.0)).as_seconds_f64() * fps;
        assert!((last - last.round()).abs() < 1e-6);
        assert!(layout.quantize(time(600.0)) <= time(600.0));
    }
}

#[test]
fn moving_selection_snaps_either_edge_only_while_shift_is_held() {
    for edge in [200.0, 400.0] {
        let mut fixture = DragFixture::new(0.0, 600.0, 30.0);
        fixture.position = time(if edge == 200.0 { 100.0 } else { 500.0 });
        fixture.selection = TimeRange::new(time(200.0), time(400.0));
        let layout = fixture.draw(vec![], false, false).0;
        let origin = egui::pos2(layout.view.x(layout.waveform, 300.0), 135.0);
        fixture.press(origin, true);
        let target = origin
            + egui::vec2(
                layout.waveform.width()
                    * ((fixture.position.as_seconds_f64() - edge) / 600.0) as f32
                    - 2.0,
                0.0,
            );
        let (_, snapped, markers) =
            fixture.draw(vec![egui::Event::PointerMoved(target)], true, false);
        let range = snapped.display_selection.expect("selection");
        if edge == 400.0 {
            assert_eq!(range.end(), time(500.0));
        } else {
            assert_eq!(range.start(), time(100.0));
        }
        assert_eq!(range.duration(), time(200.0));
        assert!(markers.is_empty());
        if edge == 400.0 {
            let free = fixture
                .draw(vec![], false, false)
                .1
                .display_selection
                .expect("free selection");
            assert!(free.end() < time(500.0));
        }
    }
}

#[test]
fn resize_marker_follows_the_grabbed_edge_and_reverse_creation_hides_it() {
    let mut fixture = DragFixture::new(0.0, 600.0, 30.0);
    fixture.selection = TimeRange::new(time(200.0), time(400.0));
    let layout = fixture.draw(vec![], false, false).0;
    let origin = egui::pos2(layout.view.x(layout.waveform, 200.0) + 4.0, 130.0);
    fixture.press(origin, false);
    let target = origin + egui::vec2(30.0, 0.0);
    let (layout, result, markers) =
        fixture.draw(vec![egui::Event::PointerMoved(target)], false, false);
    let edge = layout.view.x(
        layout.ruler,
        result
            .display_selection
            .expect("resized")
            .start()
            .as_seconds_f64(),
    );
    assert_eq!(markers.len(), 1);
    assert!((markers[0] - edge).abs() < 0.01);
    assert!((markers[0] - target.x).abs() > 3.9);
    let fixture = DragFixture::new(0.0, 600.0, 30.0);
    let origin = egui::pos2(400.0, 135.0);
    fixture.press(origin, false);
    let (_, result, markers) = fixture.draw(
        vec![egui::Event::PointerMoved(egui::pos2(250.0, 135.0))],
        false,
        false,
    );
    assert!(result.display_selection.is_some());
    assert!(markers.is_empty());
}

#[test]
fn edge_autoscroll_keeps_selection_origin_and_advances_once_per_frame() {
    for repeat in [false, true] {
        let mut fixture = DragFixture::new(100.0, 100.0, 30.0);
        fixture.position = time(100.0);
        let origin = egui::pos2(320.0, 135.0);
        fixture.press(origin, false);
        let (_, first, _) = fixture.draw(
            vec![egui::Event::PointerMoved(egui::pos2(645.0, 135.0))],
            false,
            repeat,
        );
        let (before, _, _) = fixture.draw(vec![], false, repeat);
        let (after, result, _) = fixture.draw(vec![], false, repeat);
        assert!(after.view.start > before.view.start);
        assert!(after.view.start - before.view.start <= 100.0 / 60.0 + 0.01);
        assert_eq!(
            result.display_selection.expect("selection").start(),
            first.display_selection.expect("initial selection").start()
        );
        assert_eq!(
            edge_scroll(
                632.0,
                Rect::from_min_max(Pos2::ZERO, egui::pos2(640.0, 300.0))
            ),
            0.0
        );
        assert_eq!(
            edge_scroll(
                636.0,
                Rect::from_min_max(Pos2::ZERO, egui::pos2(640.0, 300.0))
            ),
            0.5
        );
        assert_eq!(
            edge_scroll(
                700.0,
                Rect::from_min_max(Pos2::ZERO, egui::pos2(640.0, 300.0))
            ),
            1.0
        );
    }
}

#[test]
fn secondary_scrollbar_drag_does_not_change_view() {
    let context = crate::fonts::test_context();
    let layout = frame(&context, vec![], true);
    let layout = frame(&context, wheel(layout.waveform.center(), 100.0, true), true);
    let point = ScrollScale::new(layout.scrollbar, layout.view.duration, layout.minimum)
        .thumb(layout.view)
        .left_center();
    frame(&context, vec![egui::Event::PointerMoved(point)], true);
    frame(&context, vec![pointer_button(point, true, true)], true);
    let moved = frame(
        &context,
        vec![egui::Event::PointerMoved(point + egui::vec2(50.0, 0.0))],
        true,
    );
    assert_eq!(moved.view.start, layout.view.start);
    assert_eq!(moved.view.span, layout.view.span);
    let released = frame(&context, vec![pointer_button(point, false, true)], true);
    assert_eq!(released.view.span, layout.view.span);
}

fn frame(context: &egui::Context, events: Vec<egui::Event>, enabled: bool) -> Layout<'static> {
    let mut layout = None;
    let _ = context.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 300.0))),
            events,
            ..Default::default()
        },
        |ui| {
            let result = show(
                ui,
                egui::Id::new("viewport-test"),
                Rect::from_min_max(egui::pos2(20.0, 100.0), egui::pos2(620.0, 200.0)),
                MediaTime::from_nanoseconds(600_000_000_000),
                30.0,
                enabled,
            );
            paint(ui, result, 30.0, MediaTime::ZERO, None);
            layout = Some(result);
        },
    );
    layout.expect("layout")
}

fn wheel(point: Pos2, delta: f32, alt: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(point),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            phase: egui::TouchPhase::Move,
            delta: egui::vec2(0.0, delta),
            modifiers: egui::Modifiers {
                alt,
                ..Default::default()
            },
        },
    ]
}

#[test]
fn wheel_zoom_pan_and_bar_resize_share_one_bounded_viewport_at_each_density() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.set_pixels_per_point(density);
        context.global_style_mut(crate::chrome::style);
        let initial = frame(&context, vec![], true);
        assert_eq!(initial.view.span, 600.0);
        assert_eq!(initial.waveform.top() - initial.ruler.bottom(), GAP);
        assert_eq!(initial.scrollbar.top() - initial.waveform.bottom(), GAP);
        let point = initial.waveform.center();
        frame(&context, vec![egui::Event::PointerMoved(point)], true);
        let zoomed = frame(&context, wheel(point, 100.0, true), true);
        assert!(zoomed.view.span < 300.0);
        assert!((zoomed.view.start + zoomed.view.span * 0.5 - 300.0).abs() < 1e-6);
        let moved = frame(&context, wheel(point, -30.0, false), true);
        assert_eq!(moved.view.span, zoomed.view.span);
        assert!(moved.view.start > zoomed.view.start);
        let blocked = frame(&context, wheel(point, 100.0, true), false);
        assert_eq!(blocked.view.span, moved.view.span);

        let thumb =
            ScrollScale::new(moved.scrollbar, moved.view.duration, moved.minimum).thumb(moved.view);
        let start = thumb.right_center() - egui::vec2(1.0, 0.0);
        frame(&context, vec![egui::Event::PointerMoved(start)], true);
        frame(
            &context,
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            true,
        );
        let resized = frame(
            &context,
            vec![egui::Event::PointerMoved(start - egui::vec2(40.0, 0.0))],
            true,
        );
        let resized_thumb =
            ScrollScale::new(resized.scrollbar, resized.view.duration, resized.minimum)
                .thumb(resized.view);
        assert!((resized_thumb.left() - thumb.left()).abs() < 0.001);
        assert!(resized.view.span < moved.view.span);
        let cancelled = frame(
            &context,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            true,
        );
        assert_eq!(cancelled.view.span, moved.view.span);
    }
}

#[test]
fn long_media_retains_frame_precision_without_large_virtual_coordinates() {
    let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 60.0));
    let mut view = View::default();
    view.fit(86400.0, 1.0);
    view.zoom(1e-20, 0.9, 1.0);
    assert!((view.x(rect, view.start + 1.0 / 30.0) - 30.0).abs() < 0.001);
    assert!(view.x(rect, view.start - 1.0) < rect.left());
    assert!(view.x(rect, view.start + view.span + 1.0) > rect.right());
}

#[test]
fn cti_shift_snapping_is_live_and_ruler_hover_waits_for_the_next_pointer_move() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.set_pixels_per_point(density);
        context.global_style_mut(crate::chrome::style);
        let time = |seconds: i64| MediaTime::from_nanoseconds(seconds * 1_000_000_000);
        let selection = TimeRange::new(time(200), time(400));
        let origin = egui::pos2(320.0, 108.0);
        let near_start = egui::pos2(222.0, 108.0);
        let render = |events, shift| {
            let mut result = crate::time_selection::Output::default();
            let mut ruler = Rect::NOTHING;
            let output = context.run_ui(
                egui::RawInput {
                    events,
                    modifiers: egui::Modifiers {
                        shift,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                |ui| {
                    let layout = show(
                        ui,
                        egui::Id::new("snap-control"),
                        Rect::from_min_max(egui::pos2(20.0, 100.0), egui::pos2(620.0, 200.0)),
                        time(600),
                        30.0,
                        true,
                    );
                    ruler = layout.ruler;
                    let response =
                        ui.allocate_rect(layout.interaction(), egui::Sense::click_and_drag());
                    result = crate::time_selection::show_in_view(
                        ui,
                        &response,
                        time(600),
                        time(300),
                        selection,
                        None,
                        true,
                        layout,
                    );
                    paint(
                        ui,
                        layout,
                        30.0,
                        result.display_position,
                        result.display_selection,
                    );
                },
            );
            let hover = output.shapes.iter().any(|shape| match &shape.shape {
                egui::Shape::Vec(shapes) => shapes.iter().any(|shape| matches!(shape,
                    egui::Shape::LineSegment { points, stroke } if stroke.color == chrome::FOREGROUND
                        && points[0].y == ruler.top() && points[1].y == ruler.bottom())),
                _ => false,
            });
            (result, hover)
        };
        for _ in 0..3 {
            render(vec![egui::Event::PointerMoved(origin)], false);
        }
        let button = |pos, pressed, shift| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers {
                shift,
                ..Default::default()
            },
        };
        assert!(!render(vec![button(origin, true, false)], false).1);
        let (snapped, hover) = render(vec![egui::Event::PointerMoved(near_start)], true);
        assert_eq!(snapped.display_position, time(200));
        assert!(!hover);
        let (free, _) = render(vec![], false);
        assert!(
            free.display_position > time(200),
            "releasing Shift removes the magnet immediately"
        );
        let (released, hover) = render(vec![button(near_start, false, true)], true);
        assert_eq!(released.seek, Some(time(200)));
        assert!(!hover);
        assert!(!render(vec![], false).1);
        assert!(render(vec![egui::Event::PointerMoved(origin)], false).1);
    }
}

#[test]
fn ruler_labels_and_scrollbar_caps_keep_the_reviewed_geometry() {
    let context = crate::fonts::test_context();
    context.global_style_mut(crate::chrome::style);
    let id = egui::Id::new("ruler-geometry");
    context.data_mut(|data| {
        data.insert_temp(
            id.with("time-viewport"),
            View {
                start: 100.0,
                span: 2.0,
                duration: 600.0,
            },
        )
    });
    let mut ruler = Rect::NOTHING;
    let mut scrollbar = Rect::NOTHING;
    let output = context.run_ui(Default::default(), |ui| {
        let layout = show(
            ui,
            id,
            Rect::from_min_max(egui::pos2(20.0, 100.0), egui::pos2(620.0, 200.0)),
            MediaTime::from_nanoseconds(600_000_000_000),
            30.0,
            true,
        );
        ruler = layout.ruler;
        scrollbar = layout.scrollbar;
        assert_eq!(
            layout.waveform.left(),
            15.0,
            "continuing content reaches the outer edge"
        );
        assert_eq!(layout.waveform.right(), 625.0);
        paint(ui, layout, 30.0, MediaTime::ZERO, None);
    });
    let mut ticks = Vec::new();
    let mut labels = Vec::new();
    for shape in &output.shapes {
        if let egui::Shape::Vec(shapes) = &shape.shape {
            for shape in shapes {
                match shape {
                    egui::Shape::LineSegment { points, stroke }
                        if stroke.color == chrome::FAINT =>
                    {
                        ticks.push(points[0].x)
                    }
                    egui::Shape::Text(text) => labels.push(text),
                    _ => {}
                }
            }
        }
    }
    assert!(!ticks.is_empty());
    assert_eq!(ticks.len(), labels.len(), "overlap never removes labels");
    assert!(ticks.windows(2).all(|pair| pair[1] - pair[0] >= 49.9));
    for (x, text) in ticks.into_iter().zip(labels) {
        let bounds = text.galley.rect.translate(text.pos.to_vec2());
        let expected = x - bounds.width() * 0.5;
        assert!((bounds.left() - expected).abs() < 0.01);
        assert!((text.pos.y + text.galley.mesh_bounds.top() - ruler.top()).abs() < 0.01);
        assert_eq!(
            text.galley.job.sections[0].format.font_id.size,
            chrome::TEXT_SM
        );
    }
    let caps: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if rect.fill == chrome::FAINT && scrollbar.contains_rect(rect.rect) =>
            {
                Some(rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(caps.len(), 2);
    assert!(
        caps.iter()
            .all(|cap| cap.rect.size() == egui::vec2(5.0, 10.0))
    );
}

#[test]
fn ruler_and_gap_seek_in_edited_time_without_selecting_or_painting_an_offscreen_cti() {
    for gap in [false, true] {
        let context = crate::fonts::test_context();
        let id = egui::Id::new("ruler-input");
        context.data_mut(|data| {
            data.insert_temp(
                id.with("time-viewport"),
                View {
                    start: 100.0,
                    span: 200.0,
                    duration: 600.0,
                },
            )
        });
        let mut results = Vec::new();
        let mut target = Pos2::ZERO;
        for pass in 0..5 {
            let events = if pass >= 3 {
                vec![
                    egui::Event::PointerMoved(target),
                    egui::Event::PointerButton {
                        pos: target,
                        button: egui::PointerButton::Primary,
                        pressed: pass == 3,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            } else {
                Vec::new()
            };
            let output = context.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    let layout = show(
                        ui,
                        id,
                        Rect::from_min_max(egui::pos2(20.0, 100.0), egui::pos2(620.0, 200.0)),
                        MediaTime::from_nanoseconds(600_000_000_000),
                        30.0,
                        true,
                    );
                    target = egui::pos2(
                        layout.ruler.center().x,
                        if gap {
                            layout.ruler.bottom() + GAP * 0.5
                        } else {
                            layout.ruler.center().y
                        },
                    );
                    let response =
                        ui.allocate_rect(layout.interaction(), egui::Sense::click_and_drag());
                    results.push(crate::time_selection::show_in_view(
                        ui,
                        &response,
                        MediaTime::from_nanoseconds(600_000_000_000),
                        MediaTime::ZERO,
                        None,
                        None,
                        true,
                        layout,
                    ));
                },
            );
            if pass < 3 {
                assert!(
                    !output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default())),
                    "an offscreen CTI does not appear at the viewport edge"
                );
            }
        }
        let result = results.last().expect("press");
        assert_eq!(
            result.seek,
            Some(MediaTime::from_nanoseconds(200_000_000_000))
        );
        assert!(result.selection.is_none() && result.edit.is_none());
    }
}

#[test]
fn maximum_zoom_wheel_is_an_exact_noop_at_both_ends_and_in_the_middle() {
    for density in [1.0, 1.25, 2.0] {
        for x in [20.0, 320.0, 620.0] {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            let initial = frame(&context, vec![], true);
            let point = egui::pos2(x, initial.waveform.center().y);
            frame(&context, vec![egui::Event::PointerMoved(point)], true);
            let zoomed = frame(&context, wheel(point, 10000.0, true), true);
            for _ in 0..12 {
                let held = frame(&context, wheel(point, 120.0, true), true);
                assert_eq!(held.view.start, zoomed.view.start);
                assert_eq!(held.view.span, zoomed.view.span);
            }
        }
    }
}

#[test]
fn scrollbar_inner_span_and_each_handle_share_a_continuous_inverse() {
    let bar = Rect::from_min_size(egui::pos2(5.0, 100.0), egui::vec2(630.0, 10.0));
    for duration in [2.0, 600.0, 1800.0, 86400.0] {
        let minimum = 640.0 / 30.0 / 50.0;
        let scale = ScrollScale::new(bar, duration, minimum);
        let mut view = View {
            start: (duration - minimum) * 0.6,
            span: minimum,
            duration,
        };
        let thumb = scale.thumb(view);
        assert!((thumb.width() - 10.0).abs() < 0.0001);
        for factor in [1.01, 1.2, 2.0, 5.0, 20.0] {
            let span = (minimum * factor).min(duration);
            assert!(scale.width(span) > 10.0);
            assert!((scale.span(scale.width(span)) - span).abs() < 1e-8);
        }
        if duration >= 1800.0 {
            assert!((scale.width(minimum * 5.0) - 11.0).abs() < 1e-6);
        }
        for edge in [-1, 1] {
            for distance in [0.25, 1.0, 3.0, 20.0] {
                let next = scale.drag(view, edge as f32 * distance, edge);
                let next_thumb = scale.thumb(next);
                assert!(next.span > minimum);
                assert!((next_thumb.width() - thumb.width() - distance).abs() < 0.0001);
                let stationary = if edge < 0 {
                    next_thumb.right() - thumb.right()
                } else {
                    next_thumb.left() - thumb.left()
                };
                assert!(stationary.abs() < 0.0001, "opposite handle stays fixed");
            }
        }
        view.span = duration;
        view.start = 0.0;
        assert_eq!(scale.thumb(view), bar);
    }
}

#[test]
fn long_selection_is_bounded_before_rounded_rectangle_tessellation() {
    let fixture = DragFixture::new(300.0, 640.0 / 30.0 / 50.0, 30.0);
    let mut selected = fixture;
    selected.selection = TimeRange::new(time(0.0), time(600.0));
    // Capture the selection shape before tessellation. Its huge time extent
    // must not become a huge, shallow path that loses the horizontal borders.
    let mut selected_rect = None;
    let output = selected.context.run_ui(egui::RawInput::default(), |ui| {
        let layout = show(
            ui,
            selected.id,
            Rect::from_min_max(egui::pos2(5.0, 100.0), egui::pos2(635.0, 220.0)),
            time(600.0),
            30.0,
            true,
        );
        let response = ui.allocate_rect(layout.interaction(), egui::Sense::click_and_drag());
        crate::time_selection::show_in_view(
            ui,
            &response,
            time(600.0),
            time(300.0),
            selected.selection,
            None,
            true,
            layout,
        );
        selected_rect = Some(layout.waveform);
    });
    let visible = selected_rect.expect("waveform layout");
    let outline = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if rect.stroke.color == chrome::FOREGROUND.gamma_multiply(0.25) =>
            {
                Some(rect)
            }
            _ => None,
        })
        .expect("selection outline");
    assert!(outline.rect.width() <= visible.width() + 10.0);
    assert_eq!(outline.rect.y_range(), visible.y_range());
    assert!(outline.rect.left() < visible.left() && outline.rect.right() > visible.right());
}

#[test]
fn decoded_nanosecond_pts_and_ruler_ticks_share_a_physical_pixel() {
    for density in [1.0_f32, 1.25, 2.0] {
        for fps in [25.0, 30.0, 30000.0 / 1001.0] {
            let rect = Rect::from_min_size(egui::pos2(5.0, 0.0), egui::vec2(630.0, 60.0));
            for first in [0_u64, 300, 3000000] {
                let view = View {
                    start: first as f64 / fps,
                    span: 630.0 / fps / 50.0,
                    duration: 864000.0,
                };
                for frame in first..first + 12 {
                    let seconds = frame as f64 / fps;
                    let pts = time(seconds);
                    let tick =
                        crate::time_selection::cti_x(rect, view.x(rect, seconds), 1.0 / density);
                    let cti = crate::time_selection::cti_x(
                        rect,
                        view.x(rect, pts.as_seconds_f64()),
                        1.0 / density,
                    );
                    assert!(
                        (tick - cti).abs() < 0.01,
                        "density={density} fps={fps} frame={frame} tick={tick} cti={cti}"
                    );
                }
            }
        }
    }
}

#[test]
fn offset_video_pts_share_the_grid_after_trim_and_stretch() {
    let Some(_root) = crate::tests::isolated_test_root(
        "timeline_view::input_tests::offset_video_pts_share_the_grid_after_trim_and_stretch",
    ) else {
        return;
    };
    let path = std::env::temp_dir().join(format!(
        "towavue-offset-grid-{}.nut",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let ffmpeg = std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg"))
        .join("bin/ffmpeg.exe");
    let output = crate::tests::hidden_command(ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=size=32x24:rate=60:duration=0.3",
            "-f",
            "lavfi",
            "-i",
            "sine=sample_rate=48000:duration=0.4",
            "-vf",
            "settb=1/90000,setpts=PTS+3240",
            "-enc_time_base:v",
            "1/90000",
            "-fps_mode",
            "passthrough",
            "-c:v",
            "ffv1",
            "-c:a",
            "pcm_s16le",
            "-output_ts_offset",
            "5",
        ])
        .arg(&path)
        .output()
        .expect("fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    verify_source_grid(&path, &[0.05, 0.15], Some(time(0.036)));
    std::fs::remove_file(path).expect("remove owned fixture");
}

#[test]
#[ignore = "read-only owner reference; set TOWAVUE_FRAME_GRID_REFERENCE"]
fn reference_video_pts_share_the_rendered_frame_grid() {
    let path = std::path::PathBuf::from(
        std::env::var_os("TOWAVUE_FRAME_GRID_REFERENCE").expect("reference path"),
    );
    verify_source_grid(&path, &[0.1, 10.0, 600.0, 1700.0], None);
}

fn verify_source_grid(
    path: &std::path::Path,
    positions: &[f64],
    expected_origin: Option<MediaTime>,
) {
    use winit::application::ApplicationHandler;
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::platform::windows::EventLoopBuilderExtWindows;
    struct Trial<'a> {
        path: &'a std::path::Path,
        positions: &'a [f64],
        origin: Option<MediaTime>,
    }
    impl ApplicationHandler for Trial<'_> {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(winit::window::Window::default_attributes().with_visible(false))
                .expect("hidden window");
            let renderer = towavue_runtime_windows::FrameRenderer::new(&window).expect("D3D11");
            verify_source_grid_on_device(
                self.path,
                self.positions,
                self.origin,
                renderer.graphics_device(),
            );
            event_loop.exit();
        }
        fn window_event(
            &mut self,
            _: &ActiveEventLoop,
            _: winit::window::WindowId,
            _: winit::event::WindowEvent,
        ) {
        }
    }
    let event_loop = EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("event loop");
    event_loop
        .run_app(&mut Trial {
            path,
            positions,
            origin: expected_origin,
        })
        .expect("reference grid check");
}

fn verify_source_grid_on_device(
    path: &std::path::Path,
    positions: &[f64],
    expected_origin: Option<MediaTime>,
    device: towavue_runtime_windows::GraphicsDevice,
) {
    use towavue_runtime_windows::{PlaybackSession, adjacent_video_frame};

    let session = PlaybackSession::open_paused(
        path,
        device,
        0.0,
        1.0,
        towavue_core::PlaybackRange::default(),
        |_| {},
    )
    .expect("session");
    let fps = session.source_video_grid_rate().expect("grid rate");
    let origin = session.source_video_grid_origin();
    if let Some(expected) = expected_origin {
        assert_eq!(origin, expected);
    }
    for &position in positions {
        for forward in [false, true] {
            let pts = adjacent_video_frame(path, time(position), forward, None, &|| false)
                .expect("query")
                .expect("adjacent PTS");
            for density in [1.0, 1.25, 2.0] {
                let fixture = DragFixture::new(
                    (pts.as_seconds_f64() - 0.08).max(0.0),
                    640.0 / fps / 50.0,
                    fps,
                );
                fixture.context.set_pixels_per_point(density);
                let mut cti = 0.0;
                let output = fixture.context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            Pos2::ZERO,
                            egui::vec2(640.0, 300.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        let layout = show(
                            ui,
                            fixture.id,
                            Rect::from_min_max(egui::pos2(5.0, 100.0), egui::pos2(635.0, 220.0)),
                            time(2200.0),
                            fps,
                            true,
                        )
                        .with_source_grid(origin, None);
                        assert!(
                            (layout.quantize(pts).as_nanoseconds() - pts.as_nanoseconds()).abs()
                                <= 2
                        );
                        cti = crate::time_selection::cti_x(
                            layout.ruler,
                            layout.view.x(layout.ruler, pts.as_seconds_f64()),
                            1.0 / density,
                        );
                        paint(ui, layout, fps, pts, None);
                    },
                );
                let aligned = output.shapes.iter().any(|shape| match &shape.shape {
                    egui::Shape::Vec(shapes) => shapes.iter().any(|shape| matches!(shape,
                        egui::Shape::LineSegment {points,stroke} if stroke.color==chrome::FAINT && (points[0].x-cti).abs()<0.01)),
                    _ => false,
                });
                assert!(
                    aligned,
                    "PTS {} must share a physical pixel center with a ruler tick at {density}",
                    pts.as_seconds_f64()
                );
            }
            let end = position + 1.0;
            let mut plan =
                towavue_core::EditTimeline::new(time(end), towavue_core::PlaybackRange::default())
                    .expect("plan");
            assert!(plan.apply(towavue_core::TimelineEdit::Keep(
                TimeRange::new(time(0.01), time(end)).expect("trim")
            )));
            assert!(plan.apply(towavue_core::TimelineEdit::Stretch(
                TimeRange::new(MediaTime::ZERO, plan.duration()).expect("range"),
                time((end - 0.01) * 1.25)
            )));
            let edited = plan.edited_time(pts).expect("mapped frame");
            let fixture = DragFixture::new(0.0, 0.1, fps);
            let layout = fixture
                .draw(vec![], false, false)
                .0
                .with_source_grid(origin, Some(&plan));
            assert!(
                (layout.quantize(edited).as_nanoseconds() - edited.as_nanoseconds()).abs() <= 2,
                "edited PTS"
            );
        }
    }
}

#[test]
fn trimmed_grid_labels_keep_edited_time_instead_of_source_frame_numbers() {
    let fixture = DragFixture::new(0.0, 640.0 / 60.0 / 50.0, 60.0);
    let plan = towavue_core::EditTimeline::new(
        time(101.0),
        towavue_core::PlaybackRange {
            start: time(100.0),
            end: Some(time(101.0)),
        },
    )
    .expect("trim");
    let output = fixture.context.run_ui(egui::RawInput::default(), |ui| {
        let layout = show(
            ui,
            fixture.id,
            Rect::from_min_max(egui::pos2(5.0, 100.0), egui::pos2(635.0, 220.0)),
            time(1.0),
            60.0,
            true,
        )
        .with_source_grid(time(0.036), Some(&plan));
        paint(ui, layout, 60.0, time(0.1), None);
    });
    let labels: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Vec(shapes) => Some(shapes),
            _ => None,
        })
        .flatten()
        .filter_map(|shape| match shape {
            egui::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect();
    assert!(!labels.is_empty());
    assert!(
        labels
            .iter()
            .all(|label| !label.contains(':') || *label == "0:00f"),
        "first edited second: {labels:?}"
    );
}
