use super::*;
use winit::dpi::PhysicalSize;

#[test]
fn native_resize_coalesces_transitions_and_draws_interactive_changes() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let Some(_) = tests::isolated_test_root(
        "window_resize_tests::native_resize_coalesces_transitions_and_draws_interactive_changes",
    ) else {
        return;
    };
    struct Trial;
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let visible = std::env::var_os("TOWAVUE_FULLSCREEN_BACKGROUND_VISIBLE").is_some();
            let window = Arc::new(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_visible(visible)
                            .with_active(false)
                            .with_inner_size(PhysicalSize::new(640, 400)),
                    )
                    .expect("hidden resize test window"),
            );
            let caption = NativeCaption::new(window.clone()).expect("caption");
            let mut renderer = FrameRenderer::with_native_caption(&caption).expect("D3D11 surface");
            renderer.verification_track_transition_background();
            let initial = window.inner_size();
            renderer
                .resize_surface(initial.width, initial.height)
                .expect("initial size");
            let context = fonts::test_context();
            let state = egui_winit::State::new(
                context.clone(),
                egui::ViewportId::ROOT,
                &window,
                Some(window.scale_factor() as f32),
                window.theme(),
                Some(renderer.max_texture_side()),
            );
            let mut app = Application::new(None, |_| {}).expect("app");
            app.window = Some(window.clone());
            app.window_size = Some(initial);
            app.native_caption = Some(caption);
            app.renderer = Some(renderer);
            app.ui_context = Some(context);
            app.ui_state = Some(state);
            for (interactive, delta) in [(false, 32), (true, 32), (true, -16), (false, -16)] {
                app.native_caption
                    .as_ref()
                    .expect("caption")
                    .verification_size_move(interactive);
                let old_size = window.inner_size();
                app.render_frame();
                let target = PhysicalSize::new(
                    old_size.width.checked_add_signed(delta).expect("width"),
                    old_size
                        .height
                        .checked_add_signed(delta / 2)
                        .expect("height"),
                );
                let _ = window.request_inner_size(target);
                let actual = window.inner_size();
                assert_ne!(actual, old_size);
                app.window_event(event_loop, window.id(), WindowEvent::Resized(actual));
                let held = app
                    .renderer
                    .as_mut()
                    .expect("renderer")
                    .verification_surface_rgba()
                    .expect("resize surface");
                let expected = if interactive { actual } else { old_size };
                assert_eq!(
                    held.len(),
                    (expected.width * expected.height * 4) as usize,
                    "interactive resize must draw the new client before returning; ordinary notifications stay coalesced"
                );
                assert!(app.playback_error.is_none(), "{:?}", app.playback_error);
                app.renderer
                    .as_mut()
                    .expect("renderer")
                    .clear([0.25, 0.5, 0.75, 1.0])
                    .expect("sentinel");
                let sentinel = app
                    .renderer
                    .as_mut()
                    .expect("renderer")
                    .verification_surface_rgba()
                    .expect("sentinel pixels");
                for size in [actual, PhysicalSize::new(0, 0)] {
                    app.window_event(event_loop, window.id(), WindowEvent::Resized(size));
                    assert!(
                        app.renderer
                            .as_mut()
                            .expect("renderer")
                            .verification_surface_rgba()
                            .expect("unchanged pixels")
                            == sentinel,
                        "duplicate and zero sizes must not draw synchronously"
                    );
                }
            }
            let mut transitions = vec![(false, false), (false, true), (false, false)];
            if visible {
                transitions.extend([(true, true), (true, false), (false, true), (false, false)]);
            }
            let mut black_frames = 0;
            for (maximized, fullscreen) in transitions {
                if visible && fullscreen && !app.fullscreen {
                    window.set_maximized(maximized);
                }
                app.renderer
                    .as_mut()
                    .expect("renderer")
                    .clear([0.25, 0.5, 0.75, 1.0])
                    .expect("known buffer");
                let before = app
                    .renderer
                    .as_mut()
                    .expect("renderer")
                    .verification_surface_rgba()
                    .expect("readback");
                // Intermediate restore/work-area/minimize notifications must not
                // replace the surface before the final native client is drawable.
                for size in [(480, 300), (0, 0), (800, 600), (800, 600)] {
                    app.resize_window(PhysicalSize::new(size.0, size.1));
                    assert!(
                        app.renderer
                            .as_mut()
                            .expect("renderer")
                            .verification_surface_rgba()
                            .expect("held buffer")
                            == before,
                        "a size notification must not discard the buffer before redraw"
                    );
                }
                let changed = fullscreen != app.fullscreen;
                let transition_started = Instant::now();
                if changed {
                    app.set_fullscreen(fullscreen);
                    assert_eq!(app.fullscreen_from_maximized, maximized);
                } else {
                    app.set_fullscreen(fullscreen);
                    let _ = window.request_inner_size(PhysicalSize::new(960, 576));
                }
                let transition_elapsed = transition_started.elapsed();
                let actual = window.inner_size();
                app.resize_window(actual);
                if visible && changed && !maximized {
                    black_frames += 1;
                } else {
                    assert_eq!(
                        app.renderer
                            .as_mut()
                            .expect("renderer")
                            .verification_surface_rgba()
                            .expect("unchanged buffer"),
                        before,
                        "maximized, hidden and no-op transitions retain pixels"
                    );
                }
                let (submitted, black, present_elapsed) = app
                    .renderer
                    .as_ref()
                    .expect("renderer")
                    .verification_transition_background()
                    .expect("tracking");
                assert_eq!(submitted, black_frames);
                assert!(
                    black,
                    "every transition pixel must be opaque black before Present"
                );
                eprintln!(
                    "FULLSCREEN background: visible={visible} maximized={maximized} enabled={fullscreen} changed={changed} black_frames={submitted} transition_ms={:.3} last_present_flush_ms={:.3}",
                    transition_elapsed.as_secs_f64() * 1000.0,
                    present_elapsed.as_secs_f64() * 1000.0
                );
                app.render_frame();
                assert!(app.playback_error.is_none(), "{:?}", app.playback_error);
                let after = app
                    .renderer
                    .as_mut()
                    .expect("renderer")
                    .verification_surface_rgba()
                    .expect("new buffer");
                assert_eq!(after.len(), (actual.width * actual.height * 4) as usize);
                assert_eq!(app.window_size, Some(actual));
            }
            eprintln!(
                "PASS interactive resize draws the actual new client before returning; old GPU buffer otherwise survives intermediate/zero/duplicate size notifications; redraw uses the final native client across normal/fullscreen/restore. Visibility is opt-in; buffer readback is not compositor capture."
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut Trial)
        .expect("trial");
}

#[test]
fn resize_fits_displayed_media_without_changing_edits_transport_or_background_view() {
    let Some(root) = tests::isolated_test_root(
        "window_resize_tests::resize_fits_displayed_media_without_changing_edits_transport_or_background_view",
    ) else {
        return;
    };
    for (kind, reading) in [
        (MediaKind::Image, false),
        (MediaKind::Image, true),
        (MediaKind::Video, false),
        (MediaKind::Audio, false),
    ] {
        let mut app = Application::new(None, |_| {}).expect("headless app");
        let background = app
            .tabs
            .open_new(root.join("background.png"), MediaKind::Image);
        app.path = Some(root.join("background.png"));
        app.image_view.zoom = ZoomMode::Custom(3.0);
        let retained = app.take_image_tab_state();
        let background_view = retained.view;
        app.retained_images.insert(background, retained);
        let path = root.join("active.media");
        let active = app.tabs.open_new(path.clone(), kind);
        app.path = Some(path.clone());
        app.media_kind = Some(kind);
        app.reading_mode = reading;
        app.state = PlaybackState::Playing;
        app.media_duration = Some(Duration::from_secs(30));
        app.clock = Some(PlaybackClock::new(media_time(Duration::from_secs(5)), 1.0));
        app.clock.as_mut().expect("clock").set_paused(true);
        app.edits.entry(active).or_default().push(
            if kind == MediaKind::Audio {
                EditOperation::SetVolume(0.4)
            } else {
                EditOperation::RotateClockwise
            },
            kind,
        );
        let history = app.edits[&active].operations().to_vec();
        let generation = app.generation;
        let media_generation = app.media_generation;
        let position = app.current_position();
        let reading_settings = app.reading_settings;
        app.resize_window(PhysicalSize::new(960, 576));
        // Ordinary resize, maximize-sized growth, and restore-sized shrink.
        for (size, zoom) in [
            (PhysicalSize::new(640, 480), ZoomMode::Actual),
            (PhysicalSize::new(1920, 1080), ZoomMode::Cover),
            (PhysicalSize::new(960, 576), ZoomMode::Custom(2.5)),
        ] {
            app.image_view = ImageViewState {
                zoom,
                pan: (20.0, -10.0),
                selection: Some(UnitRect::FULL),
            };
            app.view_drag = Some(ViewDrag::Pan {
                origin: egui::pos2(100.0, 100.0),
                before: (30.0, 40.0),
            });
            app.resize_window(size);
            assert_eq!(app.image_view.zoom, ZoomMode::Fit);
            assert_eq!(app.image_view.pan, (0.0, 0.0));
            assert_eq!(app.image_view.selection, Some(UnitRect::FULL));
            assert!(app.view_drag.is_none());
            assert!(
                !app.cancel_view_drag(),
                "a later release cannot restore old pan"
            );
            assert_eq!(app.path.as_ref(), Some(&path));
            assert_eq!(app.state, PlaybackState::Playing);
            assert_eq!(app.current_position(), position);
            assert_eq!(app.media_duration, Some(Duration::from_secs(30)));
            assert_eq!(app.edits[&active].operations(), history);
            assert_eq!(app.generation, generation);
            assert_eq!(app.media_generation, media_generation);
            assert_eq!(app.reading_mode, reading);
            assert_eq!(app.reading_settings, reading_settings);
            assert_eq!(app.retained_images[&background].view, background_view);
        }
    }
}

#[test]
fn initial_duplicate_minimized_and_unchanged_fullscreen_notifications_preserve_zoom() {
    let Some(_) = tests::isolated_test_root(
        "window_resize_tests::initial_duplicate_minimized_and_unchanged_fullscreen_notifications_preserve_zoom",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("headless app");
    let view = ImageViewState {
        zoom: ZoomMode::Custom(2.5),
        pan: (30.0, 40.0),
        selection: Some(UnitRect::FULL),
    };
    app.image_view = view;
    for size in [
        (960, 576),
        (960, 576),
        (0, 0),
        (0, 576),
        (960, 0),
        (960, 576),
    ] {
        app.resize_window(PhysicalSize::new(size.0, size.1));
        assert_eq!(app.image_view, view);
    }
    app.set_fullscreen(false);
    assert_eq!(app.image_view, view);
    for fullscreen in [true, false] {
        app.image_view = view;
        app.view_drag = Some(ViewDrag::Pan {
            origin: egui::Pos2::ZERO,
            before: (10.0, 20.0),
        });
        app.set_fullscreen(fullscreen);
        assert_eq!(app.image_view.zoom, ZoomMode::Fit);
        assert_eq!(app.image_view.pan, (0.0, 0.0));
        assert_eq!(app.image_view.selection, view.selection);
        assert!(app.view_drag.is_none());
        app.image_view = view;
        app.set_fullscreen(fullscreen);
        app.resize_window(PhysicalSize::new(960, 576));
        assert_eq!(app.image_view, view);
    }
}
