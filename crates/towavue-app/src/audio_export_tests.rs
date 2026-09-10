use super::*;
use std::os::windows::process::CommandExt;
use std::sync::mpsc;

pub(super) fn fixture(path: &Path) {
    let output = std::process::Command::new(
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg")).join("bin/ffmpeg.exe"),
    )
    .creation_flags(0x08000000)
    .args([
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=size=64x48:rate=4:duration=1",
        "-f",
        "lavfi",
        "-i",
        "anullsrc=r=48000:cl=stereo:d=1",
        "-c:v",
        "ffv1",
        "-c:a",
        "pcm_s16le",
    ])
    .arg(path)
    .output()
    .expect("owned silent video fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(super) fn drain_export<F: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<F>,
    events: &mpsc::Receiver<AppEvent>,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.active_export.is_some() {
        if let AppEvent::Export(event) = events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("export event")
        {
            app.handle_export_event(event);
        }
    }
}

#[test]
fn audio_derivative_preserves_video_save_cursor_target_history_transport_and_leave_guard() {
    let Some(root) = crate::tests::isolated_test_root(
        "audio_export_tests::audio_derivative_preserves_video_save_cursor_target_history_transport_and_leave_guard",
    ) else {
        return;
    };
    let source = root.join("source 日本語 & video.mkv");
    fixture(&source);
    let original = std::fs::read(&source).expect("original");
    for dirty in [false, true] {
        let (notify, events) = mpsc::channel();
        let mut app = Application::new(None, move |event| {
            let _ = notify.send(event);
        })
        .expect("app");
        let tab = app.tabs.open_new(source.clone(), MediaKind::Video);
        app.path = Some(source.clone());
        app.media_kind = Some(MediaKind::Video);
        app.state = PlaybackState::Paused;
        let history = app.edits.entry(tab).or_default();
        history.push(EditOperation::SetVolume(0.5), MediaKind::Video);
        history.mark_exported(&[EditOperation::SetVolume(0.5)]);
        if dirty {
            history.push(EditOperation::RotateClockwise, MediaKind::Video);
        }
        let operations = history.operations().to_vec();
        let saved = root.join(format!("previous-{dirty}.mp4"));
        std::fs::write(&saved, b"previous video export").expect("prior output");
        app.export_paths.insert(tab, saved.clone());
        let target = root.join(format!("audio-{dirty}.wav"));
        app.pending_dialog = Some(DialogIntent::Export {
            tab,
            source: source.clone(),
            kind: MediaKind::Video,
            generation: app.media_generation,
            output: ExportOutput::AudioOnly,
            continuation: None,
        });
        app.finish_dialog(Ok(Some(target.clone())));
        let export = app.active_export.as_ref().expect("worker connected");
        assert_eq!(export.options.output, ExportOutput::AudioOnly);
        assert_eq!(export.request.kind, MediaKind::Video);
        assert_eq!(export.request.operations, operations);
        drain_export(&mut app, &events);
        assert!(app.export_error.is_none(), "{:?}", app.export_error);
        assert_eq!(&std::fs::read(&target).expect("audio output")[..4], b"RIFF");
        assert_eq!(app.export_paths.get(&tab), Some(&saved));
        assert_eq!(app.edits[&tab].operations(), operations);
        assert_eq!(app.edits[&tab].is_dirty(), dirty);
        assert_eq!(app.state, PlaybackState::Paused);
        assert_eq!(app.path.as_ref(), Some(&source));
        assert_eq!(app.tabs.active().expect("tab").id, tab);
        assert!(app.pending_guard.is_none());
        assert!(
            app.status_message
                .as_ref()
                .expect("status")
                .0
                .contains("video save state unchanged")
        );
        // Even a caller asking to reuse a target must choose a fresh audio path.
        assert!(!app.export_current_output(false, None, ExportOutput::AudioOnly));
        assert!(
            app.active_export.is_none(),
            "headless dialog cannot start, prior video target is not reused"
        );
        assert_eq!(
            std::fs::read(&saved).expect("prior output retained"),
            b"previous video export"
        );
        if dirty {
            app.request_guarded(GuardedAction::CloseTab(tab));
            assert!(matches!(app.pending_guard, Some(GuardedAction::CloseTab(id)) if id == tab));
            assert!(app.edits[&tab].is_dirty());
        }
    }
    assert_eq!(std::fs::read(source).expect("original retained"), original);
}

#[test]
fn audio_derivative_dialog_cancel_stale_source_failure_and_cancel_race_keep_edits() {
    let Some(root) = crate::tests::isolated_test_root(
        "audio_export_tests::audio_derivative_dialog_cancel_stale_source_failure_and_cancel_race_keep_edits",
    ) else {
        return;
    };
    let source = root.join("source.mkv");
    fixture(&source);
    let (notify, events) = mpsc::channel();
    let mut app = Application::new(None, move |event| {
        let _ = notify.send(event);
    })
    .expect("app");
    let tab = app.tabs.open_new(source.clone(), MediaKind::Video);
    app.path = Some(source.clone());
    app.media_kind = Some(MediaKind::Video);
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::RotateClockwise, MediaKind::Video);
    let generation = app.media_generation;
    let intent = || DialogIntent::Export {
        tab,
        source: source.clone(),
        kind: MediaKind::Video,
        generation,
        output: ExportOutput::AudioOnly,
        continuation: None,
    };
    for result in [Ok(None), Err(DialogError::OwnerUnavailable)] {
        app.pending_dialog = Some(intent());
        app.dispatch(CommandId::ExportAudio);
        app.finish_dialog(result);
        assert!(app.active_export.is_none());
        assert!(app.pending_guard.is_none());
    }
    let target = root.join("audio.wav");
    app.pending_dialog = Some(intent());
    let second = app.tabs.open_new(root.join("other.mkv"), MediaKind::Video);
    app.finish_dialog(Ok(Some(target.clone())));
    assert!(!target.exists());
    assert!(app.active_export.is_none());
    app.tabs.activate(tab);
    app.tabs.close(second);
    std::fs::write(&target, b"retain output").expect("target");
    for invalid in [true, false] {
        if invalid {
            app.edits.get_mut(&tab).expect("history").push(
                EditOperation::SetTrimStart(MediaTime::from_nanoseconds(2_000_000_000)),
                MediaKind::Video,
            );
        } else {
            app.edits.get_mut(&tab).expect("history").undo();
        }
        app.pending_dialog = Some(intent());
        app.finish_dialog(Ok(Some(target.clone())));
        if !invalid {
            // Wait until publication wins, then cancel before processing Finished.
            // Cancellation must not turn a derivative into a saved video or resume leaving.
            loop {
                if let AppEvent::Export(event) =
                    events.recv_timeout(Duration::from_secs(10)).expect("event")
                {
                    if matches!(event, ExportEvent::Finished(_)) {
                        app.handle_ui_action(UiAction::CancelExport);
                        app.handle_export_event(event);
                        break;
                    }
                    app.handle_export_event(event);
                }
            }
        } else {
            drain_export(&mut app, &events);
            assert!(app.export_error.take().is_some());
            assert_eq!(
                std::fs::read(&target).expect("retained target"),
                b"retain output"
            );
        }
        assert!(app.edits[&tab].is_dirty());
        assert!(app.export_paths.is_empty());
        assert!(app.pending_guard.is_none());
        assert!(!app.exit_requested);
    }
}
