use crate::audio_export::tests::frame;
use crate::video_rotation::tests::node;
use crate::*;

type App = Application<Box<dyn Fn(AppEvent) + Send + Sync>>;

fn focus(target: egui::accesskit::NodeId) -> egui::Event {
    egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
        action: egui::accesskit::Action::Focus,
        target_tree: egui::accesskit::TreeId::ROOT,
        target_node: target,
        data: None,
    })
}

fn setup() -> (App, std::sync::mpsc::Receiver<AppEvent>) {
    let (sent, events) = std::sync::mpsc::channel();
    let callback: Box<dyn Fn(AppEvent) + Send + Sync> = Box::new(move |event| {
        let _ = sent.send(event);
    });
    let mut app = Application::new(None, callback).expect("app");
    let context = fonts::test_context();
    context.enable_accesskit();
    context.global_style_mut(chrome::style);
    app.ui_context = Some(context);
    (app, events)
}

fn wait_image(app: &mut App, events: &std::sync::mpsc::Receiver<AppEvent>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.image_loading {
        app.handle_app_event(
            events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("image event"),
        );
    }
    assert!(app.image.is_some(), "{:?}", app.image_error);
}

fn tree(app: &mut App, events: Vec<egui::Event>) -> egui::accesskit::TreeUpdate {
    frame(app, egui::vec2(640.0, 480.0), events)
        .platform_output
        .accesskit_update
        .expect("tree")
}

fn settle(app: &mut App) -> egui::accesskit::TreeUpdate {
    tree(app, vec![]);
    tree(app, vec![]);
    tree(app, vec![])
}

#[test]
fn tab_focus_image_controls_restore_by_role_without_reloading_or_editing() {
    let Some(root) = tests::isolated_test_root(
        "tab_focus::tests::tab_focus_image_controls_restore_by_role_without_reloading_or_editing",
    ) else {
        return;
    };
    let (mut app, events) = setup();
    let mut tabs = Vec::new();
    let mut bitmap = vec![0_u8; 62];
    bitmap[..2].copy_from_slice(b"BM");
    bitmap[2..6].copy_from_slice(&62_u32.to_le_bytes());
    bitmap[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bitmap[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bitmap[18..22].copy_from_slice(&2_u32.to_le_bytes());
    bitmap[22..26].copy_from_slice(&1_u32.to_le_bytes());
    bitmap[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bitmap[28..30].copy_from_slice(&24_u16.to_le_bytes());
    bitmap[34..38].copy_from_slice(&8_u32.to_le_bytes());
    bitmap[54..].copy_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0]);
    for name in ["first.bmp", "second.bmp"] {
        let path = root.join(name);
        std::fs::write(&path, &bitmap).expect("fixture");
        app.open_external(path, true);
        wait_image(&mut app, &events);
        tabs.push(app.tabs.active().expect("tab").id);
        app.image_view.selection = Some(UnitRect {
            min: UnitPoint { x: 0.0, y: 0.0 },
            max: UnitPoint { x: 0.5, y: 1.0 },
        });
        let target = node(
            &settle(&mut app),
            if name == "first.bmp" {
                "Selection right (pixels)"
            } else {
                "Crop preview"
            },
        );
        tree(&mut app, vec![focus(target)]);
        settle(&mut app);
    }
    let histories = app.edits.clone();
    std::fs::remove_file(root.join("first.bmp"))
        .expect("remove owned source to prove retained return");
    std::fs::remove_file(root.join("second.bmp"))
        .expect("remove owned source to prove retained return");
    for _ in 0..3 {
        for (tab, label) in [
            (tabs[0], "Selection right (pixels)"),
            (tabs[1], "Crop preview"),
        ] {
            app.activate_tab(tab);
            assert!(!app.image_loading);
            let returned = settle(&mut app);
            assert_eq!(returned.focus, node(&returned, label), "{label}");
            assert_eq!(app.edits, histories);
        }
    }
    let context = app.ui_context.clone().expect("context");
    app.load_path(root.join("second.bmp"), MediaKind::Image);
    assert!(!context.data(|data| {
        data.get_temp::<super::State>(super::state_id())
            .expect("state")
            .saved
            .contains_key(&tabs[1])
    }));
    app.remove_tab(tabs[0], false);
    assert!(!context.data(|data| {
        data.get_temp::<super::State>(super::state_id())
            .expect("state")
            .saved
            .contains_key(&tabs[0])
    }));
}

#[test]
fn tab_focus_semantic_roles_ignore_widget_ids_and_wait_only_until_ready_or_new_input() {
    let context = fonts::test_context();
    let mut tabs = TabSet::default();
    let a = tabs.open_new("a.png".into(), MediaKind::Image);
    let b = tabs.open_new("b.wav".into(), MediaKind::Audio);
    let draw = |active, salt, enabled, loading, events: Vec<egui::Event>, request| {
        let mut ids = Vec::new();
        let _ = context.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                super::begin(&context, Some(active), enabled);
                ui.push_id(salt, |ui| {
                    for role in ["play", "seek"] {
                        let response = ui.button(role);
                        if request == Some(role) {
                            response.request_focus();
                        }
                        super::observe(&response, role);
                        ids.push(response.id);
                    }
                });
                super::finish(&context, loading, true);
            },
        );
        ids
    };
    draw(a, 1, true, false, vec![], Some("seek"));
    draw(b, 1, true, false, vec![], Some("play"));
    let ids = draw(a, 4, true, false, vec![], None);
    assert_eq!(context.memory(|m| m.focused()), Some(ids[1]));
    let ids = draw(b, 7, true, false, vec![], None);
    assert_eq!(context.memory(|m| m.focused()), Some(ids[0]));
    draw(a, 9, false, true, vec![], None);
    assert!(context.data(|d| {
        d.get_temp::<super::State>(super::state_id())
            .expect("state")
            .pending
            .is_some()
    }));
    let ids = draw(a, 9, true, false, vec![], None);
    assert_eq!(context.memory(|m| m.focused()), Some(ids[1]));
    draw(b, 1, true, false, vec![], None);
    draw(a, 2, false, true, vec![], None);
    let ids = draw(
        a,
        2,
        true,
        false,
        vec![egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        Some("play"),
    );
    assert_eq!(context.memory(|m| m.focused()), Some(ids[0]));
    super::forget(&context, a);
    assert!(!context.data(|d| {
        d.get_temp::<super::State>(super::state_id())
            .expect("state")
            .saved
            .contains_key(&a)
    }));
}

#[test]
fn tab_focus_registers_image_video_audio_values_and_path_rows_without_dispatch() {
    let Some(root) = tests::isolated_test_root(
        "tab_focus::tests::tab_focus_registers_image_video_audio_values_and_path_rows_without_dispatch",
    ) else {
        return;
    };
    for (kind, extension, labels) in [
        (
            MediaKind::Image,
            "png",
            vec![
                "Reading mode",
                "Crop preview",
                "Selection left (pixels)",
                "Selection right (pixels)",
                "Selection top (pixels)",
                "Selection bottom (pixels)",
                "Image position",
                "second.png",
            ],
        ),
        (
            MediaKind::Video,
            "mp4",
            vec![
                "Play / replay",
                "Playback position (seconds)",
                "Time selection start (seconds)",
                "Time selection end (seconds)",
                "Local volume (%)",
                "Selected duration (seconds)",
            ],
        ),
        (
            MediaKind::Audio,
            "wav",
            vec![
                "Play / replay",
                "Repeat off",
                "Shuffle off",
                "2. second.wav",
                "Time selection start (seconds)",
                "Time selection end (seconds)",
                "Local volume (%)",
                "Selected duration (seconds)",
            ],
        ),
    ] {
        let (mut app, _) = setup();
        let context = app.ui_context.clone().expect("context");
        let paths =
            ["first", "second", "third"].map(|name| root.join(format!("{name}.{extension}")));
        let active = app.tabs.open_new(paths[0].clone(), kind);
        let other = app
            .tabs
            .open_new(root.join(format!("other.{extension}")), kind);
        app.tabs.activate(active);
        app.path = Some(paths[0].clone());
        app.media_kind = Some(kind);
        app.state = PlaybackState::Paused;
        app.media_duration = Some(Duration::from_secs(10));
        app.timeline_open = kind != MediaKind::Image;
        app.time_selection = towavue_core::TimeRange::new(
            media_time(Duration::from_secs(2)),
            media_time(Duration::from_secs(8)),
        );
        app.image_view.selection = Some(UnitRect::FULL);
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: paths
                .iter()
                .map(|path| towavue_core::FolderMediaItem {
                    identity: towavue_core::ShellIdentity::new(vec![]),
                    path: path.clone(),
                    kind,
                })
                .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: std::time::SystemTime::now(),
        });
        if kind == MediaKind::Image {
            app.image = Some(
                ImagePresentation::from_decoded(
                    &context,
                    &paths[0],
                    Arc::new(DecodedImage {
                        format: "test",
                        frames: vec![towavue_runtime_windows::DecodedImageFrame {
                            width: 2,
                            height: 2,
                            rgba: vec![255; 16],
                            delay: Duration::ZERO,
                        }],
                    }),
                )
                .expect("in-memory image"),
            );
        }
        for label in labels {
            app.filmstrip_open = label == "second.png";
            let initial = settle(&mut app);
            let target = initial
                .nodes
                .iter()
                .find(|(_, n)| n.label().is_some_and(|l| l.starts_with(label)))
                .expect(label)
                .0;
            tree(&mut app, vec![focus(target)]);
            assert_eq!(settle(&mut app).focus, target, "initial {label}");
            // Keep the widget layout identical to expose accidental cross-tab ID reuse.
            app.tabs.activate(other);
            settle(&mut app);
            app.tabs.activate(active);
            let returned = settle(&mut app);
            assert_eq!(returned.focus, target, "restored {label}");
            assert!(app.edits.is_empty() && app.pending_guard.is_none());
            assert_eq!(app.state, PlaybackState::Paused);
            assert_eq!(app.path.as_ref(), Some(&paths[0]));
        }
    }
}

#[test]
fn tab_focus_fullscreen_reveals_the_saved_transport_and_missing_controls_fall_back() {
    let Some(root) = tests::isolated_test_root(
        "tab_focus::tests::tab_focus_fullscreen_reveals_the_saved_transport_and_missing_controls_fall_back",
    ) else {
        return;
    };
    let (mut app, _) = setup();
    let context = app.ui_context.clone().expect("context");
    let active = app.tabs.open_new(root.join("video.mp4"), MediaKind::Video);
    let other = app.tabs.open_new(root.join("other.mp4"), MediaKind::Video);
    app.tabs.activate(active);
    app.path = Some(root.join("video.mp4"));
    app.media_kind = Some(MediaKind::Video);
    app.state = PlaybackState::Playing;
    let initial = settle(&mut app);
    let pause = initial
        .nodes
        .iter()
        .find(|(_, n)| n.label().is_some_and(|l| l.starts_with("Pause")))
        .expect("pause")
        .0;
    tree(&mut app, vec![focus(pause)]);
    settle(&mut app);
    app.tabs.activate(other);
    settle(&mut app);
    app.fullscreen = true;
    app.fullscreen_controls_keyboard = false;
    app.fullscreen_controls_visible = false;
    app.tabs.activate(active);
    app.state = PlaybackState::Paused;
    let returned = settle(&mut app);
    assert!(app.fullscreen_controls_visible);
    let play = returned
        .nodes
        .iter()
        .find(|(_, n)| n.label().is_some_and(|l| l.starts_with("Play / replay")))
        .expect("play")
        .0;
    assert_eq!(returned.focus, play);
    app.tabs.activate(other);
    settle(&mut app);
    app.tabs.activate(active);
    app.fullscreen = false;
    app.media_kind = Some(MediaKind::Image);
    let returned = settle(&mut app);
    assert_eq!(returned.focus, node(&returned, "video.mp4"));
    super::forget(&context, active);
    assert!(!context.data(|data| {
        data.get_temp::<super::State>(super::state_id())
            .expect("state")
            .saved
            .contains_key(&active)
    }));
}

pub(crate) fn hardware_focus<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    label: &str,
    set: bool,
) {
    let context = app.ui_context.clone().expect("context");
    context.enable_accesskit();
    let mut target = None;
    for pass in 0..4 {
        let mut actions = Vec::new();
        let events = if set && pass == 2 {
            vec![focus(target.expect("target"))]
        } else {
            vec![]
        };
        // Hosted probes share the live redraw clock; renderer-only fixtures keep
        // egui's simulated clock because they have no native input adapter.
        let time = app.ui_state.as_mut().and_then(|state| {
            state
                .take_egui_input(app.window.as_ref().expect("window"))
                .time
        });
        let mut output = context.run_ui(
            egui::RawInput {
                time,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                events,
                ..Default::default()
            },
            |ui| app.draw_ui(ui, &mut actions),
        );
        assert!(actions.is_empty(), "focus must not dispatch media actions");
        let tree = output
            .platform_output
            .accesskit_update
            .take()
            .expect("tree");
        target = Some(
            tree.nodes
                .iter()
                .find(|(_, node)| node.label().is_some_and(|value| value.starts_with(label)))
                .expect("media control")
                .0,
        );
        if pass == 3 {
            assert_eq!(tree.focus, target.expect("live target"), "{label}");
        }
        let renderer = app.renderer.as_mut().expect("renderer");
        renderer.resize_surface(640, 480).expect("UI surface");
        renderer
            .render_ui(&context, output)
            .expect("render retained focus");
        renderer.present_surface().expect("present focus");
        if let Some(state) = app.ui_state.as_mut() {
            let native_time = state
                .take_egui_input(app.window.as_ref().expect("window"))
                .time
                .expect("native clock");
            assert!(
                context.time() <= native_time,
                "Focus probe advanced beyond the native clock: {} > {native_time}",
                context.time()
            );
        }
    }
    eprintln!("PASS retained media focus: {label}, set={set}, native UI rendered without commands");
}
