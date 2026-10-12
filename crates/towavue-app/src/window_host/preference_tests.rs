use super::*;
use towavue_runtime_windows::{VideoExportQuality, ViewPreference, ViewPreferences};

fn host(path: &Path) -> WindowHost {
    let mut host = WindowHost::new(None, None).expect("host");
    assert!(
        host.view_preferences.is_none(),
        "owner settings are never opened in tests"
    );
    host.view_preferences = Some(Arc::new(
        ViewPreferences::open(path.to_owned(), |error| panic!("{error}")).expect("fixture store"),
    ));
    for app in host.windows.values_mut() {
        app.restore_view_preferences(host.view_preferences.clone());
    }
    host
}

#[test]
fn explicit_settings_survive_fresh_processes_and_old_windows_do_not_revert_them() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::preference_tests::explicit_settings_survive_fresh_processes_and_old_windows_do_not_revert_them",
    ) else {
        return;
    };
    for phase in ["write", "read"] {
        let result = crate::tests::hidden_command(
            std::env::current_exe().expect("owned preference fixture"),
        )
        .args([
            "--exact",
            "window_host::preference_tests::preference_restart_child",
            "--ignored",
            "--nocapture",
        ])
        .env("TOWAVUE_PREFERENCE_FIXTURE", root.join("viewing.conf"))
        .env("TOWAVUE_PREFERENCE_PHASE", phase)
        .output()
        .expect("owned preference fixture");
        assert!(
            result.status.success(),
            "{phase}: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
    }
}

#[test]
#[ignore = "owned subprocess of preference restart regression"]
fn preference_restart_child() {
    let root =
        crate::tests::isolated_test_root("window_host::preference_tests::preference_restart_child")
            .expect("isolated subprocess");
    let path =
        PathBuf::from(std::env::var_os("TOWAVUE_PREFERENCE_FIXTURE").expect("owned fixture"));
    // The isolation helper expands short TEMP names, as does the parent fixture.
    assert!(path.starts_with(&root));
    let mut host = host(&path);
    let first = *host
        .windows
        .keys()
        .next()
        .expect("owned preference fixture");
    if std::env::var("TOWAVUE_PREFERENCE_PHASE").expect("owned preference fixture") == "write" {
        assert!(!path.exists(), "startup does not write defaults");
        assert!(host.windows[&first].shows_tab_icons());
        let second = host
            .add_application(None)
            .expect("owned preference fixture");
        let app = host
            .windows
            .get_mut(&first)
            .expect("owned preference fixture");
        app.ui_context = Some(fonts::test_context());
        tab_transfer::tests::install(
            app,
            root.join("settings.bmp"),
            tab_transfer::tests::decoded(false),
        );
        for command in [
            CommandId::ResizeViewKeepArea,
            CommandId::ResizeViewKeepScale,
            CommandId::VolumeStepTen,
            CommandId::ToggleImageInterpolation,
            CommandId::ToggleImageMinification,
            CommandId::FolderNavigationStop,
            CommandId::ToggleReuseFolderTab,
            CommandId::ToggleTabIcons,
            CommandId::ToggleReadingMode,
            CommandId::IncreaseReadingPages,
            CommandId::DecreaseReadingFirstPage,
            CommandId::ToggleReadingAxis,
            CommandId::ReverseReadingOrder,
        ] {
            app.dispatch(command);
        }
        // Export choices require video command context.
        app.media_kind = Some(MediaKind::Video);
        app.dispatch(CommandId::ToggleHardwareEncode);
        app.dispatch(CommandId::ExportQualitySmaller);
        app.media_kind = Some(MediaKind::Image);
        let original = app.reading_settings;
        app.reading_mode = false;
        for cancel in [true, false] {
            let mut drag = reading_input::ReadingDrag::new(original, false, 1.0);
            drag.motion((60.0, 0.0));
            app.reading_drag = Some(drag);
            app.finish_reading_drag(cancel);
            assert_eq!(
                app.view_preferences
                    .as_ref()
                    .expect("store")
                    .value(ViewPreference::ReadingReversed),
                u16::from(cancel)
            );
        }
        app.dispatch(CommandId::ReverseReadingOrder);
        let app = host
            .windows
            .get_mut(&second)
            .expect("owned preference fixture");
        assert_eq!(app.view_resize_mode, view_resize::Mode::Fit);
        assert!(
            !app.shows_tab_icons(),
            "existing windows observe the shared choice"
        );
        app.dispatch(CommandId::CycleVolumeStep);
        assert_eq!(app.volume_step_percent, 5);
        assert_eq!(app.video_export_quality(), VideoExportQuality::Smaller);
        let third = host
            .add_application(None)
            .expect("owned preference fixture");
        verify(&host.windows[&third]);
        // Closing an older window does not write its stale defaults.
        host.windows.remove(&second);
    } else {
        assert!(
            !host.windows[&first].shows_tab_icons(),
            "restart restores tab icons"
        );
        verify(&host.windows[&first]);
        let second = host
            .add_application(None)
            .expect("owned preference fixture");
        verify(&host.windows[&second]);
        let app = host
            .windows
            .get_mut(&first)
            .expect("owned preference fixture");
        // Reading controls are disabled without an image/reading owner.
        app.dispatch(CommandId::IncreaseReadingPages);
        app.dispatch(CommandId::ToggleReadingAxis);
        assert_eq!(
            host.view_preferences
                .as_ref()
                .expect("owned preference fixture")
                .value(ViewPreference::ReadingPages),
            302
        );
        assert_eq!(
            host.view_preferences
                .as_ref()
                .expect("owned preference fixture")
                .value(ViewPreference::ReadingAxis),
            1
        );
    }
}

fn verify(app: &WindowApplication) {
    assert_eq!(app.view_resize_mode, view_resize::Mode::KeepScale);
    assert_eq!(app.volume_step_percent, 5);
    assert!(app.nearest_images);
    assert!(!app.high_quality_minification);
    assert!(!app.folder_navigation_loop);
    assert!(app.reuses_folder_tabs());
    assert_eq!(
        app.reading_settings,
        ReadingSettings {
            page_count: 3,
            first_page_count: 2,
            axis: ReadingAxis::Vertical,
            reversed: true
        }
    );
    assert_eq!(app.video_export_quality(), VideoExportQuality::Smaller);
    assert!(app.prefer_hardware_encode);
    assert!(
        !app.reading_mode,
        "per-document mode is not restored as a preference"
    );
}
