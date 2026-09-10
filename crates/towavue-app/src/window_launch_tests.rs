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
    let canonical = relative.canonicalize().expect("local manifest");
    for path in [relative, canonical.clone()] {
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
