use super::*;
use towavue_core::{ResampleFilter, VideoResize, VideoRotation};
use towavue_runtime_windows::VideoOrientation;

#[test]
fn scrub_geometry_preserves_ordered_crop_rotation_flip_and_resample() {
    let crop = PixelCrop {
        x: 20,
        y: 10,
        width: 60,
        height: 40,
    };
    let geometry = Geometry::new(
        (100, 80),
        2.0,
        VideoOrientation::default(),
        &[
            EditOperation::Crop(crop),
            EditOperation::RotateClockwise,
            EditOperation::FlipHorizontal,
        ],
    );
    assert_eq!(geometry.size, vec2(40.0, 60.0));
    assert_eq!(geometry.aspect, 0.5);
    assert_eq!(geometry.source_aspect, 2.5);
    for (position, uv) in &geometry.vertices {
        assert!((position.x - (uv.y * 80.0 - 10.0)).abs() < 0.0001);
        assert!((position.y - (uv.x * 100.0 - 20.0)).abs() < 0.0001);
    }
    let rotation = VideoRotation::new(450, (100, 80), 2.0).expect("rotation");
    let canvas = rotation.size();
    let resize =
        VideoResize::new((80, 60), ResampleFilter::Bicubic, (40, 30), 1.0).expect("resize");
    let geometry = Geometry::new(
        (100, 80),
        2.0,
        VideoOrientation::default(),
        &[
            EditOperation::RotateVideo(rotation),
            EditOperation::Crop(PixelCrop {
                x: canvas.0 / 2 - 20,
                y: canvas.1 / 2 - 15,
                width: 40,
                height: 30,
            }),
            EditOperation::ResizeVideo(resize),
            EditOperation::FlipVertical,
        ],
    );
    assert_eq!(geometry.size, vec2(80.0, 60.0));
    assert_eq!(geometry.aspect, 1.0);
    assert!(geometry.vertices.len() >= 4);
    for (position, uv) in &geometry.vertices {
        assert!(Rect::from_min_max(pos2(-0.001, -0.001), pos2(80.001, 60.001)).contains(*position));
        assert!(Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)).contains(*uv));
    }
    let geometry = Geometry::new(
        (100, 80),
        1.0,
        VideoOrientation::default(),
        &[
            EditOperation::RotateClockwise,
            EditOperation::RotateCounterclockwise,
            EditOperation::FlipHorizontal,
            EditOperation::FlipHorizontal,
        ],
    );
    for (position, uv) in geometry.vertices {
        assert_eq!(position.to_vec2(), uv.to_vec2() * vec2(100.0, 80.0));
    }
}

#[test]
fn scrub_mesh_uses_cell_uvs_and_preserves_rotated_empty_corners() {
    let context = fonts::test_context();
    let texture = context.load_texture(
        "scrub-test",
        egui::ColorImage::filled([960, 640], Color32::WHITE),
        egui::TextureOptions::LINEAR,
    );
    let uv = Rect::from_min_max(pos2(0.25, 0.5), pos2(0.5, 0.75));
    let rect = Rect::from_min_size(pos2(50.0, 80.0), vec2(800.0, 600.0));
    let rotation = VideoRotation::new(450, (100, 80), 1.0).expect("rotation");
    let geometry = Geometry::new(
        (100, 80),
        1.0,
        VideoOrientation::default(),
        &[EditOperation::RotateVideo(rotation)],
    );
    let mesh = geometry.mesh(&texture, uv, true, rect);
    assert_eq!(mesh.indices.len(), 6);
    assert!(
        mesh.vertices
            .iter()
            .all(|v| uv.contains(v.uv) && rect.contains(v.pos))
    );
    assert!(
        mesh.vertices
            .iter()
            .all(|v| v.pos.distance(rect.min) > 20.0)
    );
    assert_eq!(mesh.texture_id, texture.id());
    let simple = Geometry::new((160, 90), 1.0, VideoOrientation::default(), &[]);
    let padded = simple.mesh(&texture, uv, true, rect);
    let fallback = simple.mesh(&texture, uv, false, rect);
    assert!(padded.vertices[0].uv.y > fallback.vertices[0].uv.y);
    assert_eq!(padded.vertices[0].uv.x, fallback.vertices[0].uv.x);
}

pub(crate) fn exercise<N: Fn(AppEvent) + Send + Sync + 'static>(app: &mut Application<N>) {
    app.edits.clear();
    app.media_duration = Some(Duration::from_secs(2));
    app.timeline_open = false;
    let context = fonts::test_context();
    app.ui_context = Some(context.clone());
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(500.0, 300.0));
    let bar = Rect::from_min_size(pos2(0.0, 260.0), vec2(500.0, 40.0));
    let start = pos2(100.0, 260.0);
    let end = pos2(400.0, 260.0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };
    let frame = |app: &mut Application<N>, events, discard| {
        let mut actions = Vec::new();
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ui| {
                app.video_scrub_seen = false;
                app.video_scrub_surface = Some((
                    ui.painter().clone(),
                    Rect::from_min_max(screen.min, pos2(500.0, 250.0)),
                ));
                app.video_rect = Some(screen);
                app.draw_seek_bar(ui.ctx(), bar, None, &mut actions);
                app.draw_video_scrub();
                if discard && context.current_pass_index() == 0 {
                    context.request_discard("scrub release survives discarded pass");
                }
            },
        );
        (actions, output)
    };
    for playing in [false, true] {
        for cancel in 0..3 {
            app.state = PlaybackState::Paused;
            app.seek_to(media_time(Duration::from_millis(200)));
            let deadline = Instant::now() + Duration::from_secs(5);
            while app
                .session
                .as_ref()
                .expect("session")
                .video_refresh_pending()
                && Instant::now() < deadline
            {
                app.load_next_frame();
                app.advance_media();
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(
                !app.session
                    .as_ref()
                    .expect("session")
                    .video_refresh_pending()
            );
            assert!(app.scrub_pause(!playing));
            frame(app, vec![], false);
            frame(app, vec![], false);
            let generation = app.generation;
            assert!(
                frame(
                    app,
                    vec![egui::Event::PointerMoved(start), button(start, true)],
                    false
                )
                .0
                .is_empty()
            );
            assert!(app.video_scrub.is_none(), "a press is not a scrub");
            assert!(
                frame(app, vec![egui::Event::PointerMoved(end)], false)
                    .0
                    .is_empty()
            );
            assert_eq!(app.state, PlaybackState::Paused);
            let frozen = app.current_position();
            assert_eq!(app.generation, generation, "drag must not seek");
            assert!(
                app.video_scrub
                    .as_ref()
                    .is_some_and(|scrub| scrub.phase == Phase::Dragging)
            );
            let texture = context.load_texture(
                "scrub-sample",
                egui::ColorImage::filled([240, 160], Color32::RED),
                egui::TextureOptions::LINEAR,
            );
            let texture_id = texture.id();
            app.update_scrub_sample(Some((
                texture,
                Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                true,
            )));
            let (_, output) = frame(app, vec![], false);
            assert!(
                app.video_rect.is_none(),
                "main surface uses the low-resolution mesh"
            );
            assert!(context.tessellate(output.shapes, output.pixels_per_point).iter().any(|primitive| matches!(&primitive.primitive, egui::epaint::Primitive::Mesh(mesh) if mesh.texture_id == texture_id)));
            assert_eq!(app.current_position(), frozen);
            if cancel != 0 {
                frame(
                    app,
                    vec![if cancel == 1 {
                        egui::Event::Key {
                            key: egui::Key::Escape,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        }
                    } else {
                        egui::Event::WindowFocused(false)
                    }],
                    false,
                );
                assert!(app.video_scrub.is_none());
                assert_eq!(
                    app.state,
                    if playing {
                        PlaybackState::Playing
                    } else {
                        PlaybackState::Paused
                    }
                );
                assert!(frame(app, vec![button(end, false)], false).0.is_empty());
                frame(app, vec![egui::Event::WindowFocused(true)], false);
                assert_eq!(app.generation, generation);
            } else {
                let (actions, _) = frame(
                    app,
                    vec![button(end, false), egui::Event::PointerMoved(start)],
                    true,
                );
                assert!(matches!(
                    actions.as_slice(),
                    [UiAction::CommitVideoScrub(_)]
                ));
                assert!(
                    app.video_scrub
                        .as_ref()
                        .is_some_and(|scrub| scrub.phase == Phase::Committing)
                );
                for action in actions {
                    app.handle_ui_action(action);
                }
                assert_eq!(app.generation, generation.next());
                assert!(app.current_position().as_seconds_f64() > 1.5);
                assert_eq!(
                    app.state,
                    if playing {
                        PlaybackState::Playing
                    } else {
                        PlaybackState::Paused
                    }
                );
                assert!(frame(app, vec![], false).0.is_empty());
                assert!(
                    app.video_scrub
                        .as_ref()
                        .is_some_and(|scrub| scrub.phase == Phase::AwaitingFrame)
                );
                let deadline = Instant::now() + Duration::from_secs(5);
                while app
                    .session
                    .as_ref()
                    .expect("session")
                    .video_refresh_pending()
                    && Instant::now() < deadline
                {
                    app.load_next_frame();
                    app.advance_media();
                    std::thread::sleep(Duration::from_millis(5));
                }
                frame(app, vec![], false);
                assert!(
                    app.video_scrub.is_none(),
                    "the actual refreshed frame retires the scrub texture"
                );
                assert!(app.video_rect.is_some());
            }
        }
    }
    app.begin_video_scrub();
    app.queue_video_scrub(media_time(Duration::from_secs(2)), &mut Vec::new());
    app.commit_video_scrub(media_time(Duration::from_secs(2)));
    assert_eq!(
        app.state,
        PlaybackState::Paused,
        "EOF never resumes playback"
    );
    app.cancel_video_scrub();
    eprintln!(
        "PASS video scrub: frozen transport, mesh, one release seek, multi-pass, cancel and EOF"
    );
}
