use super::*;

fn frame(context: &egui::Context, events: Vec<egui::Event>, enabled: bool) -> Layout {
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

        let thumb = thumb_rect(moved.scrollbar, moved.view);
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
        assert!((resized.view.start - moved.view.start).abs() < 1e-6);
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
