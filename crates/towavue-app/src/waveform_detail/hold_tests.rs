use super::*;
use towavue_core::{TimeRange, TimelineEdit};

fn range(start: u64, end: u64) -> TimeRange {
    TimeRange::new(
        media_time(Duration::from_millis(start)),
        media_time(Duration::from_millis(end)),
    )
    .expect("range")
}

#[test]
fn released_gain_keeps_detailed_paint_through_refinement_and_repeated_edits() {
    let Some(root) = crate::tests::isolated_test_root(
        "waveform_detail::hold_tests::released_gain_keeps_detailed_paint_through_refinement_and_repeated_edits",
    ) else {
        return;
    };
    for kind in [MediaKind::Audio, MediaKind::Video] {
        for density in [1.0, 1.25, 2.0] {
            let mut app = Application::new(None, |_| {}).expect("app");
            let context = fonts::test_context();
            context.set_pixels_per_point(density);
            app.ui_context = Some(context.clone());
            let path = root.join("controlled-envelope.wav");
            app.tabs.open_new(path.clone(), kind);
            app.path = Some(path);
            app.media_kind = Some(kind);
            app.media_duration = Some(Duration::from_secs(10));
            app.state = PlaybackState::Paused;
            app.timeline_open = true;
            app.time_selection = Some(range(2000, 6000));
            app.waveform = Some(context.load_texture(
                "coarse",
                egui::ColorImage::filled([16, 4], Color32::WHITE),
                TextureOptions::LINEAR,
            ));
            let coarse = app.waveform.as_ref().expect("overview").id();
            let frame = |app: &mut Application<_>, events| {
                let mut actions = Vec::new();
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(640.0, 300.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        app.draw_timeline(ui, &mut actions);
                        if context.current_pass_index() == 0 {
                            context.request_discard("release continuity");
                        }
                    },
                );
                (output, actions)
            };
            for _ in 0..3 {
                frame(&mut app, vec![]);
            }
            let columns = app.waveform_detail.key.as_ref().expect("key").columns;
            app.waveform_detail.values = Some(
                (0..columns)
                    .map(|column| [0.13, 0.4, 1.5, 0.9][column as usize % 4])
                    .collect::<Vec<_>>()
                    .into(),
            );
            app.waveform_detail.started = true;
            app.waveform_detail.finished = true;
            let painted = |output: &egui::FullOutput| {
                assert!(
                    !output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Mesh(mesh) if mesh.texture_id == coarse)),
                    "never flash the coarse overview"
                );
                output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Mesh(mesh)
                            if !mesh.vertices.is_empty()
                                && mesh.vertices.iter().all(|vertex| vertex.color == color()) =>
                        {
                            Some(mesh.clone())
                        }
                        _ => None,
                    })
                    .expect("detailed waveform")
            };
            let output = frame(&mut app, vec![]).0;
            let rect = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Rect(shape)
                        if shape.fill == chrome::BORDER
                            && shape.rect.width() > 400.0
                            && shape.rect.height() > 50.0 =>
                    {
                        Some(shape.rect)
                    }
                    _ => None,
                })
                .expect("timeline bounds");
            assert_eq!(color(), Color32::from_white_alpha(64));
            let start = egui::pos2(rect.left() + rect.width() * 0.4, rect.center().y);
            let end = egui::pos2(start.x, rect.top());
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            frame(
                &mut app,
                vec![egui::Event::PointerMoved(start), button(start, true)],
            );
            let dragging = painted(&frame(&mut app, vec![egui::Event::PointerMoved(end)]).0);
            let (released, actions) = frame(&mut app, vec![button(end, false)]);
            assert_eq!(painted(&released).vertices, dragging.vertices);
            assert!(matches!(
                actions.as_slice(),
                [UiAction::TimeAdjustment(
                    _,
                    _,
                    _,
                    TimelineEdit::ScaleVolume(_, 2.0)
                )]
            ));
            for action in actions {
                app.handle_ui_action(action);
            }
            let held = painted(&frame(&mut app, vec![]).0);
            assert_eq!(held.vertices, dragging.vertices, "first post-commit frame");
            assert!(app.waveform_detail.held_gain.is_some());
            assert!(app.waveform_detail.values.is_none());
            // Keep the completion queued while verifying settling and an in-flight job.
            app.waveform_detail.started = true;
            app.waveform_detail.finished = false;
            let obsolete = app.waveform_detail.key.clone().expect("old request");
            for _ in 0..3 {
                assert_eq!(painted(&frame(&mut app, vec![]).0).vertices, held.vertices);
            }
            let next = range(3123, 6789);
            let preview = app
                .detailed_waveform(&context, rect, Some((next, 0.5)))
                .expect("next preview");
            app.push_edit(EditOperation::Timeline(TimelineEdit::ScaleVolume(
                next, 0.5,
            )));
            let committed = app
                .detailed_waveform(&context, rect, None)
                .expect("repeated commit");
            assert_eq!(
                committed.vertices, preview.vertices,
                "retain fractional-column cuts and unclipped amplitudes"
            );
            app.install_detailed_waveform(
                app.media_generation,
                obsolete,
                Ok(vec![0.0; columns as usize]),
            );
            assert_eq!(
                app.detailed_waveform(&context, rect, None)
                    .expect("reject stale result")
                    .vertices,
                committed.vertices
            );
            let retained = app.waveform_detail.take_retained();
            app.waveform_detail = retained;
            assert_eq!(
                app.detailed_waveform(&context, rect, None)
                    .expect("restored hold")
                    .vertices,
                committed.vertices
            );
            app.waveform_detail.started = true;
            let key = app.waveform_detail.key.clone().expect("current request");
            app.install_detailed_waveform(
                app.media_generation,
                key,
                Err("controlled refinement failure".into()),
            );
            assert_eq!(
                app.detailed_waveform(&context, rect, None)
                    .expect("failure retains exact preview")
                    .vertices,
                committed.vertices
            );
            app.waveform_detail.restart_pending();
            app.waveform_detail.started = true;
            let key = app.waveform_detail.key.clone().expect("accepted request");
            app.install_detailed_waveform(
                app.media_generation,
                key,
                Ok(vec![0.1; columns as usize]),
            );
            assert!(app.waveform_detail.held_gain.is_none());
            assert_ne!(
                app.detailed_waveform(&context, rect, None)
                    .expect("refined result")
                    .vertices,
                committed.vertices
            );
        }
    }
}

#[test]
fn gain_handoff_requires_the_exact_committed_preview_and_matching_source_geometry() {
    let plan =
        EditTimeline::new(media_time(Duration::from_secs(10)), Default::default()).expect("plan");
    let key = Key {
        path: "source.wav".into(),
        plan,
        rate: 1.0,
        volume: 1.0,
        columns: 4,
    };
    let preview = (range(2123, 6345), 0.5);
    let detail = Detail {
        key: Some(Arc::new(key.clone())),
        values: Some(Arc::from([0.2, 1.5, 0.4, 0.8])),
        last_gain_preview: Some(preview),
        ..Default::default()
    };
    let mut next = key.clone();
    assert!(
        next.plan
            .apply(TimelineEdit::ScaleVolume(preview.0, preview.1))
    );
    assert!(detail.hold_committed_gain(&next).is_some());
    for change in 0..6 {
        let mut changed = next.clone();
        match change {
            0 => changed.path = "other.wav".into(),
            1 => changed.columns += 1,
            2 => changed.rate = 2.0,
            3 => changed.volume = 0.5,
            4 => changed.plan = key.plan.clone(), // Cancellation/Undo.
            _ => {
                changed.plan.apply(TimelineEdit::Delete(range(0, 1000)));
            }
        }
        assert!(detail.hold_committed_gain(&changed).is_none());
    }
    let cancelled = Detail {
        last_gain_preview: None,
        ..detail
    };
    assert!(cancelled.hold_committed_gain(&next).is_none());
}
