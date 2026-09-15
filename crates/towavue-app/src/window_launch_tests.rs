use super::*;
use towavue_runtime_windows::{LaunchRole, LaunchServer};

#[test]
fn launch_arguments_resolve_in_the_sender_before_forwarding() {
    assert!(
        initial_path_from(std::iter::empty())
            .expect("Welcome")
            .is_none()
    );
    let relative = PathBuf::from("Cargo.toml");
    let canonical = canonical_shell_path(&relative).expect("local manifest");
    let extended = relative.canonicalize().expect("extended Windows path");
    for path in [relative, canonical.clone(), extended] {
        assert_eq!(
            initial_path_from([path.into_os_string()].into_iter()).expect("one path"),
            Some(canonical.clone())
        );
    }
    assert!(initial_path_from(["Cargo.toml".into(), "Cargo.toml".into()].into_iter()).is_err());
    assert!(
        initial_path_from([canonical.join("nonexistent.png").into_os_string()].into_iter())
            .is_err()
    );
}

#[test]
fn initial_image_prefetch_reuses_originals_and_revalidates_changed_sources() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::launch_tests::initial_image_prefetch_reuses_originals_and_revalidates_changed_sources",
    ) else {
        return;
    };
    for changed in [false, true] {
        let source = root.join(format!("initial-{changed}.bmp"));
        tab_transfer::tests::bitmap(&source);
        let mut app = Application::new(Some(source.clone()), |_| {}).expect("app");
        app.prefetch_initial_image();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !app.image_loader.is_idle() {
            assert!(Instant::now() < deadline, "initial prefetch deadline");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(app.window.is_none() && app.ui_context.is_none() && app.image.is_none());
        assert!(
            app.path.is_none(),
            "prefetch must not open a tab or publish UI state"
        );
        assert_eq!(
            app.image_loader.verification_metrics().prefetch.completed,
            1
        );
        if changed {
            let mut bytes = std::fs::read(&source).expect("owned bitmap");
            bytes[54] ^= 127;
            bytes.push(0); // Force a source-stamp change without relying on clock resolution.
            std::fs::write(&source, bytes).expect("changed owned bitmap");
        }
        let expected = towavue_runtime_windows::decode_image(&source).expect("expected original");
        let path = app.initial_path.take().expect("pending launch");
        app.ui_context = Some(fonts::test_context());
        app.open_external(path, false);
        while app.image_loading || app.pending_folder.is_some() || !app.image_loader.is_idle() {
            app.finish_image_load();
            app.finish_folder_load();
            assert!(Instant::now() < deadline, "initial Open deadline");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(app.image_error.is_none());
        let image = app.image.as_ref().expect("original presentation");
        assert!(
            *image.decoded == expected,
            "complete original pixels and geometry"
        );
        assert_eq!(app.path.as_ref(), Some(&source));
        assert_eq!(app.image_sequence.awaiting, Some(app.media_generation));
        let metrics = app.image_loader.verification_metrics();
        assert_eq!(metrics.foreground.calls, u64::from(changed));
        if !changed {
            assert_eq!(metrics.initial_cache_hits, 1, "startup original is reused");
        }
    }
}

#[test]
fn initial_image_prefetch_leaves_nonimages_idle_and_failed_images_recoverable() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::launch_tests::initial_image_prefetch_leaves_nonimages_idle_and_failed_images_recoverable",
    ) else {
        return;
    };
    for path in [None, Some(root.clone()), Some(root.join("audio.wav"))] {
        let app = Application::new(path, |_| {}).expect("app");
        app.prefetch_initial_image();
        assert!(app.image_loader.is_idle());
        assert_eq!(app.image_loader.verification_metrics().prefetch.calls, 0);
    }
    let source = root.join("broken.png");
    std::fs::write(&source, b"not an image").expect("owned invalid image");
    let mut app = Application::new(Some(source.clone()), |_| {}).expect("app");
    app.prefetch_initial_image();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.image_loader.is_idle() {
        assert!(Instant::now() < deadline, "failed prefetch deadline");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        app.image_error.is_none(),
        "speculation cannot publish an error"
    );
    app.ui_context = Some(fonts::test_context());
    let path = app.initial_path.take().expect("pending launch");
    app.open_external(path, false);
    while app.image_loading || app.pending_folder.is_some() || !app.image_loader.is_idle() {
        app.finish_image_load();
        app.finish_folder_load();
        assert!(Instant::now() < deadline, "failed Open deadline");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(app.image_error.is_some() && app.image.is_none());
    assert!(app.image_sequence.awaiting.is_none());
    assert_eq!(
        std::fs::read(source).expect("unchanged source"),
        b"not an image"
    );
}

pub(super) fn exercise(host: &mut WindowHost, event_loop: &ActiveEventLoop) {
    let (sent, received) = std::sync::mpsc::channel();
    let server = LaunchServer::start_or_forward(None, move |request| {
        sent.send(request).expect("test request receiver");
    })
    .expect("receiver");
    assert!(matches!(server, LaunchRole::Primary(_)));
    let source = *host.windows.keys().next().expect("source");
    let app = host.windows.get_mut(&source).expect("source");
    opening_tests::finish_child(app);
    let original_tabs = app.tabs.clone();
    let original_edits = app.edits.clone();
    let generation = app.generation;
    let position = app.current_position();
    let path = app
        .path
        .as_ref()
        .expect("silent video")
        .canonicalize()
        .expect("sender canonical path");
    let root = path.parent().expect("fixture folder").to_owned();
    for requested in [Some(path), Some(root), None] {
        let client_path = requested.clone();
        let client = std::thread::spawn(move || {
            matches!(
                LaunchServer::start_or_forward(client_path.as_deref(), |_| {}),
                Ok(LaunchRole::Forwarded)
            )
        });
        let request = received
            .recv_timeout(Duration::from_secs(5))
            .expect("real local IPC");
        assert_eq!(request.path, requested);
        let keys: Vec<_> = host.windows.keys().copied().collect();
        host.route(Event::Launch(request));
        assert_eq!(host.pending_launches.len(), 1);
        host.open_pending_launches(event_loop, false);
        assert!(client.join().expect("client"), "startup was acknowledged");
        let child = *host
            .windows
            .keys()
            .find(|key| !keys.contains(key))
            .expect("new hosted window");
        let app = host.windows.get_mut(&child).expect("child");
        assert_eq!(app.window.as_ref().expect("HWND").is_visible(), Some(false));
        assert!(!app.command_context().has_unsaved_edits);
        if requested.as_ref().is_some_and(|path| path.is_file()) {
            opening_tests::finish_child(app);
            assert!(app.playback_error.is_none());
            assert_eq!(
                app.session
                    .as_ref()
                    .expect("session")
                    .metrics()
                    .cpu_transfer_count,
                0
            );
        } else if requested.is_none() {
            assert!(app.tabs.welcome().is_some());
        } else {
            let deadline = Instant::now() + Duration::from_secs(5);
            while app.pending_folder.is_some() {
                app.finish_folder_load();
                assert!(Instant::now() < deadline, "folder Open deadline");
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(
                app.folder_snapshot.is_some(),
                "directory resolves through ordinary Shell Open"
            );
        }
        let mut original = host.windows.remove(&source).expect("source");
        assert!(
            original
                .session
                .as_mut()
                .expect("source session")
                .draw_current(
                    host.windows
                        .get_mut(&child)
                        .expect("child")
                        .renderer
                        .as_mut()
                        .expect("renderer"),
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(160.0, 96.0)),
                    original.video_uv,
                )
                .expect("same device")
        );
        assert_eq!(original.tabs, original_tabs);
        assert_eq!(original.edits, original_edits);
        assert_eq!(original.generation, generation);
        assert_eq!(original.current_position(), position);
        host.windows.insert(source, original);
        host.windows.get_mut(&child).expect("child").exit_requested = true;
        host.remove_closed();
        assert_eq!(host.windows.len(), keys.len());
    }
    let count = host.windows.len();
    assert!(
        host.open_launched_window_with(None, false, |_, _| Err("injected startup failure".into()))
            .is_err()
    );
    assert!(
        host.open_launched_window_with(None, false, |app, device| {
            app.start_on_device(event_loop, device, false)
                .expect("hidden stage");
            Err("injected post-start failure".into())
        })
        .is_err()
    );
    assert_eq!(host.windows.len(), count);
    drop(server);
    eprintln!(
        "PASS hosted launch: real local receiver/ack opens file, folder and Welcome in hidden shared-device HWNDs; original tabs/edits/session/clock untouched, hardware cross-draw CPU transfers 0; pre/post-start failures leave no child; receiver joined on shutdown"
    );
}
