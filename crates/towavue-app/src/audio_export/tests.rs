use super::*;
use crate::audio_export_tests::drain_export;
use std::os::windows::process::CommandExt;

fn setting() -> AudioExportOptions {
    AudioExportOptions {
        normalize_peak: true,
        channels: AudioChannels::Mono,
    }
}

fn apply<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    options: AudioExportOptions,
) {
    app.dispatch(CommandId::AudioExportOptions);
    let token = app
        .audio_export_dialog
        .as_ref()
        .expect("options modal")
        .token;
    app.handle_ui_action(UiAction::FinishAudioExportOptions(token, Some(options)));
}

#[test]
fn audio_export_modal_is_transactional_and_rejects_stale_tokens_sources_and_unrelated_actions() {
    let Some(root) = crate::tests::isolated_test_root(
        "audio_export::tests::audio_export_modal_is_transactional_and_rejects_stale_tokens_sources_and_unrelated_actions",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    let path = root.join("video.mkv");
    let tab = app.tabs.open_new(path.clone(), MediaKind::Video);
    let other = app.tabs.open_new(root.join("other.wav"), MediaKind::Audio);
    app.tabs.activate(tab);
    app.path = Some(path);
    app.media_kind = Some(MediaKind::Video);
    app.state = PlaybackState::Playing;
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::RotateClockwise, MediaKind::Video);
    let history = app.edits[&tab].clone();
    app.dispatch(CommandId::AudioExportOptions);
    let token = app
        .audio_export_dialog
        .as_ref()
        .expect("viewing-mode options")
        .token;
    app.dispatch(CommandId::Undo);
    app.request_guarded(GuardedAction::Exit);
    app.handle_ui_action(UiAction::ActivateTab(other));
    assert_eq!(app.tabs.active().expect("active tab").id, tab);
    assert!(!app.exit_requested);
    assert!(app.pending_guard.is_none());
    app.handle_ui_action(UiAction::FinishAudioExportOptions(
        token.wrapping_add(1),
        Some(setting()),
    ));
    assert!(app.audio_export_dialog.is_some());
    assert!(app.audio_export_settings.is_empty());
    app.handle_ui_action(UiAction::FinishAudioExportOptions(token, None));
    assert!(app.audio_export_dialog.is_none());
    apply(&mut app, setting());
    assert_eq!(app.audio_export_settings.get(&tab), Some(&setting()));
    assert_eq!(app.edits[&tab], history);
    assert_eq!(app.state, PlaybackState::Playing);
    for stale in 0..4 {
        app.open_audio_export_options();
        let dialog = app.audio_export_dialog.as_mut().expect("dialog");
        let token = dialog.token;
        match stale {
            0 => dialog.generation = dialog.generation.wrapping_add(1),
            1 => dialog.source = root.join("different.mkv"),
            2 => dialog.kind = MediaKind::Audio,
            _ => {
                dialog.tab = app.tabs.open_new(root.join("other.wav"), MediaKind::Audio);
                app.tabs.activate(tab);
            }
        }
        app.handle_ui_action(UiAction::FinishAudioExportOptions(
            token,
            Some(AudioExportOptions::default()),
        ));
        assert_eq!(app.audio_export_settings.get(&tab), Some(&setting()));
    }
    apply(&mut app, AudioExportOptions::default());
    assert!(app.audio_export_settings.is_empty());
    for kind in [None, Some(MediaKind::Image)] {
        app.media_kind = kind;
        app.dispatch(CommandId::AudioExportOptions);
        assert!(app.audio_export_dialog.is_none());
    }
}

fn frame<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let context = app.ui_context.clone().expect("context");
    let mut actions = Vec::new();
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ui| app.draw_ui(ui, &mut actions),
    );
    for action in actions {
        app.handle_ui_action(action);
    }
    let tree = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("accessibility");
    assert!(
        tree.nodes.iter().any(|(id, _)| *id == tree.focus),
        "focused node exists"
    );
    output
}

#[test]
fn audio_export_controls_apply_cancel_restore_focus_and_fit_compact_windows() {
    let Some(root) = crate::tests::isolated_test_root(
        "audio_export::tests::audio_export_controls_apply_cancel_restore_focus_and_fit_compact_windows",
    ) else {
        return;
    };
    use crate::video_rotation::tests::{access, node};
    let context = fonts::test_context();
    context.enable_accesskit();
    let mut app = Application::new(None, |_| {}).expect("app");
    app.ui_context = Some(context.clone());
    let path = root.join("audio.wav");
    let tab = app.tabs.open_new(path.clone(), MediaKind::Audio);
    app.path = Some(path);
    app.media_kind = Some(MediaKind::Audio);
    app.state = PlaybackState::Paused;
    let size = egui::vec2(480.0, 360.0);
    for _ in 0..3 {
        frame(&mut app, size, vec![]);
    }
    // Use the actual menu widget's ID, not a fabricated focus target.
    let output = frame(&mut app, size, vec![]);
    let tree = output.platform_output.accesskit_update.expect("tree");
    let menu = node(&tree, "towavue menu");
    frame(
        &mut app,
        size,
        vec![egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Focus,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: menu,
                data: None,
            },
        )],
    );
    app.dispatch(CommandId::AudioExportOptions);
    for label in [
        "Normalize peak (-1 dBFS)",
        "Mono",
        "Stereo",
        "Keep source channels",
        "Mono",
    ] {
        let output = frame(&mut app, size, vec![]);
        let tree = output.platform_output.accesskit_update.expect("tree");
        frame(&mut app, size, vec![access(node(&tree, label), None)]);
    }
    assert_eq!(
        app.audio_export_dialog.as_ref().expect("inputs").options,
        setting()
    );
    assert!(
        app.audio_export_settings.is_empty(),
        "preview settings are not committed"
    );
    let output = frame(&mut app, size, vec![]);
    frame(
        &mut app,
        size,
        vec![access(
            node(
                &output.platform_output.accesskit_update.expect("tree"),
                "Apply options",
            ),
            None,
        )],
    );
    assert_eq!(app.audio_export_settings.get(&tab), Some(&setting()));
    for _ in 0..3 {
        frame(&mut app, size, vec![]);
    }
    assert_eq!(
        frame(&mut app, size, vec![])
            .platform_output
            .accesskit_update
            .expect("focus tree")
            .focus,
        menu
    );
    assert!(
        app.edits
            .get(&tab)
            .is_none_or(|history| !history.is_dirty())
    );
    for command in [CommandId::ToggleCommandPalette, CommandId::ToggleGridMenu] {
        app.dispatch(command);
        frame(&mut app, size, vec![]);
        app.dispatch(CommandId::AudioExportOptions);
        assert!(!app.palette_open && !app.grid_open);
        let token = app
            .audio_export_dialog
            .as_ref()
            .expect("overlay to options")
            .token;
        app.handle_ui_action(UiAction::FinishAudioExportOptions(token, None));
        for _ in 0..3 {
            frame(&mut app, size, vec![]);
        }
        assert_eq!(
            frame(&mut app, size, vec![])
                .platform_output
                .accesskit_update
                .expect("restored focus")
                .focus,
            menu
        );
    }
    for size in [
        egui::vec2(480.0, 360.0),
        egui::vec2(320.0, 200.0),
        egui::vec2(240.0, 150.0),
    ] {
        app.dispatch(CommandId::AudioExportOptions);
        for _ in 0..4 {
            frame(&mut app, size, vec![]);
        }
        let output = frame(&mut app, size, vec![]);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        for label in ["Apply options", "Cancel"] {
            let (shape, text) = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => Some((shape, text)),
                    _ => None,
                })
                .expect("footer action visible");
            let bounds = text.galley.rect.translate(text.pos.to_vec2());
            assert!(
                screen.contains_rect(bounds) && shape.clip_rect.contains_rect(bounds),
                "{label}: {bounds:?}"
            );
        }
        app.audio_export_dialog.as_mut().expect("draft").options = AudioExportOptions::default();
        frame(
            &mut app,
            size,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(app.audio_export_dialog.is_none());
        assert_eq!(app.audio_export_settings.get(&tab), Some(&setting()));
    }
    app.open_audio_export_options();
    app.audio_export_dialog.as_mut().expect("draft").options = AudioExportOptions::default();
    for _ in 0..3 {
        frame(&mut app, size, vec![]);
    }
    let output = frame(&mut app, size, vec![]);
    frame(
        &mut app,
        size,
        vec![access(
            node(
                &output.platform_output.accesskit_update.expect("tree"),
                "Cancel",
            ),
            None,
        )],
    );
    assert!(app.audio_export_dialog.is_none());
    assert_eq!(app.audio_export_settings.get(&tab), Some(&setting()));
    app.open_audio_export_options();
    app.audio_export_dialog
        .as_mut()
        .expect("stale dialog")
        .generation += 1;
    frame(&mut app, size, vec![]);
    assert!(app.audio_export_dialog.is_none());
    assert_eq!(app.audio_export_settings.get(&tab), Some(&setting()));
}

fn tone(path: &Path, video: bool) {
    let mut command = std::process::Command::new(
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg")).join("bin/ffmpeg.exe"),
    );
    command.creation_flags(0x08000000).args([
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=431:sample_rate=48000:duration=1",
    ]);
    if video {
        command.args([
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x48:rate=4:duration=1",
            "-c:v",
            "ffv1",
        ]);
    }
    let output = command
        .args(["-c:a", "pcm_s16le"])
        .arg(path)
        .output()
        .expect("owned fixture; no playback");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn decoded_samples(path: &Path) -> Vec<f32> {
    let output = std::process::Command::new(
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg")).join("bin/ffmpeg.exe"),
    )
    .creation_flags(0x08000000)
    .args(["-v", "error", "-i"])
    .arg(path)
    .args([
        "-map",
        "0:a:0",
        "-c:a",
        "pcm_f32le",
        "-f",
        "f32le",
        "pipe:1",
    ])
    .output()
    .expect("decode only");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
        .stdout
        .as_chunks::<4>()
        .0
        .iter()
        .map(|sample| f32::from_le_bytes(*sample))
        .collect()
}

#[test]
fn audio_export_settings_drive_save_resave_derivative_cancel_stale_dialog_and_source_lifecycle() {
    let Some(root) = crate::tests::isolated_test_root(
        "audio_export::tests::audio_export_settings_drive_save_resave_derivative_cancel_stale_dialog_and_source_lifecycle",
    ) else {
        return;
    };
    for kind in [MediaKind::Audio, MediaKind::Video] {
        let source = root.join(if kind == MediaKind::Video {
            "source.mkv"
        } else {
            "source.wav"
        });
        tone(&source, kind == MediaKind::Video);
        let original = std::fs::read(&source).expect("original");
        let (sender, events) = std::sync::mpsc::channel();
        let mut app = Application::new(None, move |event| {
            let _ = sender.send(event);
        })
        .expect("app");
        let tab = app.tabs.open_new(source.clone(), kind);
        app.path = Some(source.clone());
        app.media_kind = Some(kind);
        app.state = PlaybackState::Paused;
        app.edits
            .entry(tab)
            .or_default()
            .push(EditOperation::SetVolume(0.5), kind);
        let options = AudioExportOptions {
            normalize_peak: true,
            channels: AudioChannels::Stereo,
        };
        apply(&mut app, options);
        let generation = app.media_generation;
        let intent = |output| DialogIntent::Export {
            tab,
            source: source.clone(),
            kind,
            generation,
            output,
            continuation: None,
        };
        let target = root.join(if kind == MediaKind::Video {
            "video.avi"
        } else {
            "audio.wav"
        });
        app.pending_dialog = Some(intent(ExportOutput::Media));
        app.finish_dialog(Ok(Some(target.clone())));
        assert_eq!(
            app.active_export.as_ref().expect("job").options.audio,
            options
        );
        app.open_audio_export_options();
        assert!(
            app.audio_export_dialog.is_none(),
            "do not reconfigure a running export"
        );
        drain_export(&mut app, &events);
        assert!(app.export_error.is_none(), "{:?}", app.export_error);
        assert!(!app.edits[&tab].is_dirty());
        let output = decoded_samples(&target);
        assert_eq!(output.len(), 48000 * 2);
        assert!(
            output
                .as_chunks::<2>()
                .0
                .iter()
                .all(|samples| samples[0] == samples[1])
        );
        let peak = output
            .iter()
            .map(|value| value.abs())
            .fold(0.0_f32, f32::max);
        assert!((peak - 10_f32.powf(-0.05)).abs() < 0.00004);
        assert!(app.export_current(false, None));
        assert_eq!(
            app.active_export
                .as_ref()
                .expect("reuse target")
                .request
                .target,
            target
        );
        drain_export(&mut app, &events);
        assert_eq!(decoded_samples(&target), output);
        if kind == MediaKind::Video {
            app.edits
                .get_mut(&tab)
                .expect("history")
                .push(EditOperation::SetVolume(0.25), kind);
            let derivative = root.join("derivative.wav");
            app.pending_dialog = Some(intent(ExportOutput::AudioOnly));
            app.finish_dialog(Ok(Some(derivative.clone())));
            drain_export(&mut app, &events);
            assert!(app.edits[&tab].is_dirty());
            assert_eq!(app.export_paths.get(&tab), Some(&target));
            assert_eq!(app.audio_export_settings.get(&tab), Some(&options));
            assert_eq!(decoded_samples(&derivative).len(), 48000 * 2);
            let before = std::fs::read(&target).expect("existing video target");
            app.pending_dialog = Some(intent(ExportOutput::AudioOnly));
            app.finish_dialog(Ok(Some(target.clone())));
            drain_export(&mut app, &events);
            assert!(app.export_error.is_some(), "audio-only AVI is refused");
            app.handle_ui_action(UiAction::DismissExportError);
            assert_eq!(
                std::fs::read(&target).expect("retained video target"),
                before
            );
            assert_eq!(app.audio_export_settings.get(&tab), Some(&options));
        }
        app.pending_dialog = Some(intent(ExportOutput::Media));
        app.finish_dialog(Ok(None));
        assert_eq!(app.audio_export_settings.get(&tab), Some(&options));
        let before = std::fs::read(&target).expect("before stale dialog");
        app.pending_dialog = Some(intent(ExportOutput::Media));
        app.next_media_instance();
        app.finish_dialog(Ok(Some(target.clone())));
        assert!(app.active_export.is_none());
        assert_eq!(
            std::fs::read(&target).expect("stale target retained"),
            before
        );
        let other = app.tabs.open_new(source.clone(), kind);
        app.activate_tab(tab);
        assert_eq!(app.audio_export_settings.get(&tab), Some(&options));
        assert!(!app.audio_export_settings.contains_key(&other));
        app.navigate_to_unchecked(source.clone());
        assert!(
            !app.audio_export_settings.contains_key(&tab),
            "reload initializes settings"
        );
        assert!(!app.export_paths.contains_key(&tab));
        apply(&mut app, options);
        app.close_tab_unchecked(tab);
        assert!(!app.audio_export_settings.contains_key(&tab));
        assert_eq!(
            std::fs::read(&source).expect("unchanged original"),
            original
        );
    }
}

#[test]
fn leaving_guard_saves_with_current_audio_options_before_allowing_exit() {
    let Some(root) = crate::tests::isolated_test_root(
        "audio_export::tests::leaving_guard_saves_with_current_audio_options_before_allowing_exit",
    ) else {
        return;
    };
    let source = root.join("source.wav");
    let target = root.join("saved.wav");
    tone(&source, false);
    let (sender, events) = std::sync::mpsc::channel();
    let mut app = Application::new(None, move |event| {
        let _ = sender.send(event);
    })
    .expect("app");
    let tab = app.tabs.open_new(source.clone(), MediaKind::Audio);
    app.path = Some(source);
    app.media_kind = Some(MediaKind::Audio);
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::SetVolume(0.25), MediaKind::Audio);
    app.export_paths.insert(tab, target.clone());
    apply(&mut app, setting());
    app.request_guarded(GuardedAction::Exit);
    assert!(app.pending_guard.is_some());
    app.handle_ui_action(UiAction::ResolveGuard(GuardDecision::Save));
    assert_eq!(
        app.active_export
            .as_ref()
            .expect("guard export")
            .options
            .audio,
        setting()
    );
    assert!(!app.exit_requested);
    drain_export(&mut app, &events);
    assert!(app.exit_requested);
    assert!(app.export_error.is_none());
    assert!(!app.edits[&tab].is_dirty());
    let output = decoded_samples(&target);
    assert_eq!(output.len(), 48000);
    let peak = output
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0_f32, f32::max);
    assert!((peak - 10_f32.powf(-0.05)).abs() < 0.00004);
}

pub(crate) fn hardware_round_trip<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
) {
    use crate::video_rotation::tests::{access, frame, node};
    let tab = app.tabs.active().expect("native tab").id;
    let before = app
        .audio_export_settings
        .get(&tab)
        .copied()
        .unwrap_or_default();
    let history = app.edits.clone();
    let state = app.state;
    let generation = app.generation;
    app.dispatch(CommandId::AudioExportOptions);
    app.render_frame();
    let tree = frame(app, vec![]);
    frame(
        app,
        vec![access(node(&tree, "Normalize peak (-1 dBFS)"), None)],
    );
    let tree = frame(app, vec![]);
    frame(app, vec![access(node(&tree, "Mono"), None)]);
    let tree = frame(app, vec![]);
    frame(app, vec![access(node(&tree, "Apply options"), None)]);
    assert_eq!(app.audio_export_settings.get(&tab), Some(&setting()));
    app.dispatch(CommandId::AudioExportOptions);
    app.render_frame();
    let token = app
        .audio_export_dialog
        .as_ref()
        .expect("restore dialog")
        .token;
    app.handle_ui_action(UiAction::FinishAudioExportOptions(token, Some(before)));
    assert_eq!(app.edits, history);
    assert_eq!(app.state, state);
    assert_eq!(app.generation, generation);
    assert_eq!(
        app.session
            .as_ref()
            .expect("native session")
            .metrics()
            .cpu_transfer_count,
        0
    );
    eprintln!(
        "PASS hardware audio export options: UIA Apply/restore, unchanged history/transport and CPU transfers 0"
    );
}
