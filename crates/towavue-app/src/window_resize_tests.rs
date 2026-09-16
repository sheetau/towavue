use super::*;
use winit::dpi::PhysicalSize;

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
