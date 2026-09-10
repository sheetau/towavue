use super::*;

fn full<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &Application<N>,
    viewport: egui::Rect,
) -> egui::Rect {
    let (width, height, sar) = app
        .session
        .as_ref()
        .expect("session")
        .video_geometry()
        .expect("frame");
    let transform = app.visual_transform((width, height));
    crate::video_view::rect(
        viewport,
        (transform.size.0 as u32, transform.size.1 as u32),
        transform.pixel_aspect(sar),
        app.ui_context.as_ref().expect("context").pixels_per_point(),
        app.image_view,
    )
}

fn wheel(pos: egui::Pos2) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::MouseWheel {
            phase: egui::TouchPhase::Move,
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 30.0),
            modifiers: egui::Modifiers::CTRL,
        },
    ]
}

fn secondary(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

pub(super) fn exercise<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    software: bool,
) {
    let history = app.edits.clone();
    let saved_view = app.image_view;
    let generation = app.generation;
    let position = app.current_position();
    let context = app.ui_context.clone().expect("context");
    let density = context.pixels_per_point();
    let native_density = [density];
    for scale in if software {
        &[1.0, 1.25, 2.0][..]
    } else {
        &native_density[..]
    } {
        context.set_pixels_per_point(*scale);
        app.dispatch(CommandId::FitToWindow);
        frame(app, vec![]);
        frame(app, vec![]);
        let fit = app.video_rect.expect("Fit frame");
        let viewport = egui::Rect::from_center_size(fit.center(), app.image_viewport);
        let pointer = fit.center() + fit.size() * 0.12;
        frame_input(
            app,
            egui::Modifiers::CTRL,
            vec![egui::Event::PointerMoved(pointer)],
        );
        frame_input(app, egui::Modifiers::CTRL, wheel(pointer));
        assert!(matches!(app.image_view.zoom, ZoomMode::Custom(_)));
        let zoomed = full(app, viewport);
        assert!(zoomed.width() > fit.width());
        let before = (pointer - fit.min) / fit.size();
        let after = (pointer - zoomed.min) / zoomed.size();
        assert!(
            (before - after).length() < 0.0001,
            "cursor anchor {before:?}/{after:?}"
        );
        assert!(viewport.contains_rect(app.video_rect.expect("clipped zoom")));
        assert_eq!(
            app.edits, history,
            "Ctrl wheel does not adjust volume or edit pixels"
        );
        let view = app.image_view;
        let start = viewport.center();
        let end = start + egui::vec2(25.0, 15.0);
        frame(app, vec![egui::Event::PointerMoved(start)]);
        frame(app, vec![secondary(start, true)]);
        frame(
            app,
            vec![egui::Event::PointerMoved(end), secondary(end, false)],
        );
        assert_eq!(app.image_view.pan, (view.pan.0 + 25.0, view.pan.1 + 15.0));
        assert_eq!(app.image_view.selection, saved_view.selection);
        let panned = app.image_view;
        frame(
            app,
            vec![egui::Event::PointerMoved(start), secondary(start, true)],
        );
        frame(app, vec![egui::Event::PointerMoved(end)]);
        app.cancel_view_drag();
        frame(app, vec![secondary(end, false)]);
        assert_eq!(app.image_view, panned, "cancel restores the original pan");
        for command in [
            CommandId::ActualSize,
            CommandId::ZoomIn,
            CommandId::ZoomOut,
            CommandId::CoverWindow,
            CommandId::FitToWindow,
        ] {
            app.dispatch(command);
            frame(app, vec![]);
            assert!(app.video_rect.is_some());
            assert_eq!(app.edits, history);
            assert_eq!(app.image_view.selection, saved_view.selection);
        }
        app.dispatch(CommandId::ZoomIn);
        frame(app, vec![]);
        let before = app.image_view;
        for overlay in 0..3 {
            match overlay {
                0 => app.palette_open = true,
                1 => app.grid_open = true,
                _ => app.filmstrip_open = true,
            }
            frame_input(app, egui::Modifiers::CTRL, wheel(start));
            assert_eq!(app.image_view, before, "overlay {overlay}");
            app.palette_open = false;
            app.grid_open = false;
            app.filmstrip_open = false;
            frame(app, vec![]);
        }
        app.dispatch(CommandId::FreeRotateVideo);
        frame_input(app, egui::Modifiers::CTRL, wheel(start));
        assert_eq!(app.image_view, before, "modal blocks background zoom");
        let token = app.video_rotation_dialog.as_ref().expect("dialog").token;
        app.handle_ui_action(UiAction::FinishVideoRotation(token, None));
        frame(app, vec![]);
        frame(app, vec![]);
        assert_eq!(
            app.image_view, before,
            "angle preview returns to zoomed view"
        );
        if software && *scale == 1.0 {
            let source = app.path.clone().expect("source");
            let baseline = source.with_file_name("rotated.mp4");
            let target = source.with_file_name("zoomed-view.mp4");
            let tab = app.tabs.active().expect("tab").id;
            towavue_runtime_windows::export_media(&towavue_runtime_windows::ExportRequest {
                source,
                target: target.clone(),
                kind: MediaKind::Video,
                operations: app.edits[&tab].operations().to_vec(),
                hardware_encode: false,
            })
            .expect("export while zoomed");
            let frames = |path: &std::path::Path| {
                let mut frames = Vec::new();
                towavue_runtime_windows::decode_file(path, |output| {
                    if let towavue_runtime_windows::DecodeOutput::Video(frame) = output {
                        frames.push((frame.width, frame.height, frame.rgba));
                    }
                    true
                })
                .expect("reopen export");
                frames
            };
            let expected = frames(&baseline);
            assert_eq!(expected.len(), 5);
            assert_eq!(
                frames(&target),
                expected,
                "display zoom does not crop saved pixels"
            );
        }
        app.dispatch(CommandId::ToggleTimeline);
        frame(app, vec![]);
        let closed = app.image_view;
        app.dispatch(CommandId::ActualSize);
        frame_input(app, egui::Modifiers::CTRL, wheel(start));
        assert_eq!(
            app.image_view, closed,
            "viewing mode retains and cannot change zoom"
        );
        app.fullscreen = true;
        frame(app, vec![]);
        assert_eq!(
            app.image_view, closed,
            "fullscreen retains the physical zoom and pan"
        );
        assert_eq!(app.image_viewport, egui::vec2(640.0, 480.0));
        app.fullscreen = false;
        app.dispatch(CommandId::ToggleTimeline);
        frame(app, vec![]);
        app.image_view.pan = (10_000.0, 10_000.0);
        frame(app, vec![]);
        assert!(
            app.video_rect.is_none(),
            "offscreen video does not draw over chrome"
        );
        frame(
            app,
            vec![egui::Event::PointerMoved(start), secondary(start, true)],
        );
        assert!(
            matches!(app.view_drag, Some(ViewDrag::Pan { .. })),
            "empty viewport still allows recovery pan"
        );
        app.cancel_view_drag();
        frame(app, vec![secondary(start, false)]);
        app.dispatch(CommandId::FitToWindow);
        frame(app, vec![]);
        assert!(app.video_rect.is_some());
    }
    context.set_pixels_per_point(density);
    app.image_view = saved_view;
    frame(app, vec![]);
    frame(app, vec![]);
    assert_eq!(app.edits, history);
    assert_eq!(app.generation, generation);
    if software {
        assert_eq!(app.current_position(), position);
    }
    if !software {
        assert_eq!(
            app.session
                .as_ref()
                .expect("hardware")
                .metrics()
                .cpu_transfer_count,
            0
        );
    }
    eprintln!(
        "PASS video view: cursor zoom, clipped UV, pan/cancel, commands, overlay/modal/timeline ownership, offscreen recovery and unchanged edits/generation"
    );
}
