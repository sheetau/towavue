use super::*;
use towavue_core::{TimeRange, TimelineEdit};

fn request(path: &Path, kind: MediaKind) -> ExportRequest {
    ExportRequest {
        source: path.into(),
        target: path.into(),
        kind,
        operations: vec![],
        hardware_encode: false,
    }
}

fn active(
    path: &Path,
    tab: TabId,
    kind: MediaKind,
    duration: Option<Duration>,
    normalized: bool,
) -> ActiveExport {
    let request = request(path, kind);
    let mut options = ExportOptions::default();
    options.audio.normalize_peak = normalized;
    ActiveExport {
        progress: ExportProgress::new(&request, &options, duration),
        // Rejected source alias provides an owned, non-writing worker for UI-only state tests.
        job: ExportJob::start(request.clone(), |_| {}).expect("fixture worker"),
        request,
        options,
        tab,
        encoded: Duration::ZERO,
        analyzing_audio: normalized,
        cancelling: false,
        continuation: None,
    }
}

fn paint<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    size: egui::Vec2,
    density: f32,
    time: f64,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let context = app.ui_context.clone().expect("context");
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        time: Some(time),
        events,
        ..Default::default()
    };
    input
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .expect("viewport")
        .native_pixels_per_point = Some(density);
    let mut actions = Vec::new();
    let output = context.run_ui(input, |ui| app.draw_ui(ui, &mut actions));
    for action in actions {
        app.handle_ui_action(action);
    }
    output
}

fn fill(output: &egui::FullOutput) -> Option<egui::Rect> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Rect(rect)
            if rect.fill == chrome::FOREGROUND
                && (rect.rect.bottom() - chrome::TITLE_HEIGHT).abs() < 0.001
                && rect.rect.height() <= 1.0 =>
        {
            Some(rect.rect)
        }
        _ => None,
    })
}

fn indicator(output: &egui::FullOutput) -> Option<&egui::accesskit::Node> {
    output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree")
        .nodes
        .iter()
        .find_map(|(_, node)| (node.label() == Some("Export progress")).then_some(node))
}

fn context<N: Fn(AppEvent) + Send + Sync + 'static>(app: &mut Application<N>) {
    let context = fonts::test_context();
    context.enable_accesskit();
    context.global_style_mut(chrome::style);
    app.ui_context = Some(context);
}

#[test]
fn export_progress_only_unknown_running_jobs_request_animation_repaints() {
    let Some(root) = crate::tests::isolated_test_root(
        "export_progress::tests::export_progress_only_unknown_running_jobs_request_animation_repaints",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    let path = root.join("source.wav");
    let tab = app.tabs.open_new(path.clone(), MediaKind::Audio);
    let context = fonts::test_context();
    let mut time = 0.0;
    for mode in 0..4 {
        let mut export = active(
            &path,
            tab,
            MediaKind::Audio,
            (mode == 1).then_some(Duration::from_secs(10)),
            false,
        );
        export.cancelling = mode == 3;
        let mut delay = Duration::ZERO;
        for _ in 0..6 {
            let output = context.run_ui(
                egui::RawInput {
                    time: Some(time),
                    ..Default::default()
                },
                |ui| {
                    draw(
                        ui,
                        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 32.0)),
                        (mode != 0).then_some(&mut export),
                    );
                },
            );
            delay = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
            time += 0.1;
        }
        if mode == 2 {
            assert!(
                delay <= Duration::from_millis(60),
                "running unknown duration"
            );
        } else {
            assert!(
                delay > Duration::from_secs(1),
                "mode {mode} should settle: {delay:?}"
            );
        }
    }
}

pub(crate) fn hardware_round_trip<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
) {
    assert!(app.active_export.is_none());
    let source = app.path.clone().expect("hardware source");
    let tab = app.tabs.active().expect("tab").id;
    let history = app.edits.clone();
    let state = app.state;
    let generation = app.media_generation;
    app.active_export = Some(active(
        &source,
        tab,
        MediaKind::Video,
        Some(Duration::from_secs(8)),
        true,
    ));
    for (analyzing, time, expected) in [
        (true, 2, 12.5),
        (false, 2, 62.5),
        (false, 8, 99.0),
        (false, 0, 50.0),
    ] {
        app.handle_export_event(if analyzing {
            ExportEvent::AnalyzingAudio(Duration::from_secs(time))
        } else {
            ExportEvent::Progress(Duration::from_secs(time))
        });
        app.render_frame();
        let tree = crate::video_rotation::tests::frame(app, vec![]);
        let node = &tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Export progress"))
            .expect("hardware toolbar progress")
            .1;
        assert!((node.numeric_value().expect("fraction") - expected).abs() < 0.001);
        assert!(app.playback_error.is_none(), "{:?}", app.playback_error);
    }
    app.active_export.as_mut().expect("job").progress.duration = None;
    app.render_frame();
    app.handle_ui_action(UiAction::CancelExport);
    app.render_frame();
    let tree = crate::video_rotation::tests::frame(app, vec![]);
    assert!(
        tree.nodes
            .iter()
            .any(|(_, node)| node.label() == Some("Export progress")
                && node.value() == Some("Cancelling export")
                && node.numeric_value().is_none())
    );
    app.active_export.take();
    app.refresh_title();
    app.render_frame();
    let tree = crate::video_rotation::tests::frame(app, vec![]);
    assert!(
        !tree
            .nodes
            .iter()
            .any(|(_, node)| node.label() == Some("Export progress"))
    );
    assert_eq!(app.edits, history);
    assert_eq!(app.state, state);
    assert_eq!(app.media_generation, generation);
    assert_eq!(
        app.session
            .as_ref()
            .expect("session")
            .metrics()
            .cpu_transfer_count,
        0
    );
    eprintln!(
        "PASS hardware export progress: native app rendering, two-pass/fallback/indeterminate/cancel/clear, unchanged edits/transport and CPU transfers 0"
    );
}

#[test]
fn export_progress_uses_snapshot_trim_timeline_rate_and_two_pass_estimates() {
    let mut request = request(Path::new("source.wav"), MediaKind::Audio);
    let time = |seconds| media_time(Duration::from_secs(seconds));
    let range = |start, end| TimeRange::new(time(start), time(end)).expect("range");
    request.operations = vec![
        EditOperation::SetTrimStart(time(2)),
        EditOperation::SetTrimEnd(time(18)),
        EditOperation::Timeline(TimelineEdit::Delete(range(3, 7))),
        EditOperation::Timeline(TimelineEdit::Stretch(range(0, 4), time(8))),
        EditOperation::SetRate(2.0),
    ];
    for normalized in [false, true] {
        let mut options = ExportOptions::default();
        options.audio.normalize_peak = normalized;
        options.output = ExportOutput::AudioOnly;
        let progress = ExportProgress::new(&request, &options, Some(Duration::from_secs(20)));
        assert_eq!(progress.duration, Some(Duration::from_secs(8)));
        assert_eq!(
            progress.fraction(Duration::from_secs(2), true),
            Some(if normalized { 0.125 } else { 0.25 })
        );
        assert_eq!(
            progress.fraction(Duration::from_secs(2), false),
            Some(if normalized { 0.625 } else { 0.25 })
        );
        assert_eq!(
            progress.fraction(Duration::from_secs(80), false),
            Some(0.99)
        );
        assert_eq!(progress.fraction(Duration::ZERO, normalized), Some(0.0));
        assert!(
            ExportProgress::new(&request, &options, None)
                .duration
                .is_none()
        );
    }
    for kind in [MediaKind::Image, MediaKind::Audio, MediaKind::Video] {
        request.kind = kind;
        assert!(
            ExportProgress::new(&request, &ExportOptions::default(), Some(Duration::ZERO))
                .duration
                .is_none()
        );
        request.operations.clear();
        let progress = ExportProgress::new(
            &request,
            &ExportOptions::default(),
            Some(Duration::from_secs(20)),
        );
        assert_eq!(progress.duration.is_none(), kind == MediaKind::Image);
    }
}

#[test]
fn export_progress_toolbar_geometry_uia_hover_and_lightweight_loading() {
    let Some(root) = crate::tests::isolated_test_root(
        "export_progress::tests::export_progress_toolbar_geometry_uia_hover_and_lightweight_loading",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for width in [240.0, 480.0, 960.0] {
            let mut app = Application::new(None, |_| {}).expect("app");
            context(&mut app);
            let path = root.join("source.png");
            let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
            app.path = Some(path.clone());
            app.media_kind = Some(MediaKind::Image);
            app.image_loading = true;
            let size = egui::vec2(width, 400.0);
            for i in 0..3 {
                paint(&mut app, size, density, i as f64, vec![]);
            }
            let idle = paint(&mut app, size, density, 3.0, vec![]);
            assert!(
                fill(&idle).is_none() && indicator(&idle).is_none(),
                "image load is not export progress"
            );
            app.image_loading = false;
            app.active_export = Some(active(
                &path,
                tab,
                MediaKind::Video,
                Some(Duration::from_secs(10)),
                false,
            ));
            app.handle_export_event(ExportEvent::Progress(Duration::from_secs(4)));
            let before = app.edits.clone();
            for (i, events) in [
                vec![],
                vec![egui::Event::PointerMoved(egui::pos2(width * 0.4, 31.75))],
                vec![egui::Event::PointerButton {
                    pos: egui::pos2(width * 0.4, 31.75),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                vec![egui::Event::PointerButton {
                    pos: egui::pos2(width * 0.4, 31.75),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            ]
            .into_iter()
            .enumerate()
            {
                let output = paint(&mut app, size, density, 4.0 + i as f64, events);
                let rect = fill(&output).expect("toolbar fill");
                assert!((rect.height() * density - 1.0).abs() < 0.001);
                assert!((rect.width() - width * 0.4).abs() < 0.001);
                assert_eq!(rect.left(), 0.0);
                let node = indicator(&output).expect("accessible progress");
                assert_eq!(node.role(), egui::accesskit::Role::ProgressIndicator);
                assert!((node.numeric_value().expect("fraction") - 40.0).abs() < 0.001);
                assert!(!node.supports_action(egui::accesskit::Action::Focus));
                assert!(!node.supports_action(egui::accesskit::Action::SetValue));
            }
            assert_eq!(app.edits, before);
            assert_eq!(app.tabs.active().expect("tab").id, tab);
            assert_eq!(
                app.active_export.as_ref().expect("job").encoded,
                Duration::from_secs(4)
            );
            app.fullscreen = true;
            let full = paint(&mut app, size, density, 8.0, vec![]);
            assert!(fill(&full).is_none() && indicator(&full).is_none());
            assert!(
                full.platform_output
                    .accesskit_update
                    .expect("tree")
                    .nodes
                    .iter()
                    .any(|(_, node)| node.label() == Some("Cancel export"))
            );
            app.fullscreen = false;
            let restored = paint(&mut app, size, density, 9.0, vec![]);
            assert!(fill(&restored).is_some() && indicator(&restored).is_some());
        }
    }
}

#[test]
fn export_progress_unknown_length_freezes_on_cancel_and_all_terminal_events_clear() {
    let Some(root) = crate::tests::isolated_test_root(
        "export_progress::tests::export_progress_unknown_length_freezes_on_cancel_and_all_terminal_events_clear",
    ) else {
        return;
    };
    for terminal in 0..3 {
        let mut app = Application::new(None, |_| {}).expect("app");
        context(&mut app);
        let path = root.join("source.jpg");
        let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
        app.path = Some(path.clone());
        app.media_kind = Some(MediaKind::Image);
        app.active_export = Some(active(&path, tab, MediaKind::Image, None, false));
        let size = egui::vec2(640.0, 480.0);
        let first = paint(&mut app, size, 1.25, 1.0, vec![]);
        let first = fill(&first).expect("indeterminate");
        let moved = paint(&mut app, size, 1.25, 1.8, vec![]);
        assert_ne!(fill(&moved).expect("moving"), first);
        assert!(
            indicator(&moved)
                .expect("unknown progress")
                .numeric_value()
                .is_none()
        );
        app.handle_ui_action(UiAction::CancelExport);
        let frozen = paint(&mut app, size, 1.25, 2.0, vec![]);
        assert_eq!(
            indicator(&frozen).expect("cancelling").value(),
            Some("Cancelling export")
        );
        app.handle_export_event(ExportEvent::Progress(Duration::from_secs(10)));
        app.handle_export_event(ExportEvent::AnalyzingAudio(Duration::from_secs(20)));
        assert_eq!(
            app.active_export.as_ref().expect("cancelled").encoded,
            Duration::ZERO
        );
        let later = paint(&mut app, size, 1.25, 8.0, vec![]);
        assert_eq!(fill(&later), fill(&frozen));
        app.handle_export_event(ExportEvent::Finished(match terminal {
            0 => Ok(towavue_runtime_windows::ExportOutcome {
                used_hardware_encoder: false,
            }),
            1 => Err(ExportError::Cancelled),
            _ => Err(ExportError::Failed("fixture failure".into())),
        }));
        let finished = paint(&mut app, size, 1.25, 9.0, vec![]);
        assert!(fill(&finished).is_none() && indicator(&finished).is_none());
        assert!(app.active_export.is_none());
        assert_eq!(app.export_error.is_some(), terminal == 2);
        app.handle_export_event(ExportEvent::Progress(Duration::from_secs(30)));
        assert!(
            app.active_export.is_none(),
            "late progress cannot recreate the bar"
        );
    }
}

#[test]
fn export_progress_real_normalized_save_tracks_job_after_tab_switch_and_finishes_cleanly() {
    let Some(root) = crate::tests::isolated_test_root(
        "export_progress::tests::export_progress_real_normalized_save_tracks_job_after_tab_switch_and_finishes_cleanly",
    ) else {
        return;
    };
    let source = root.join("source.mkv");
    crate::audio_export_tests::fixture(&source);
    for output in [ExportOutput::Media, ExportOutput::AudioOnly] {
        normalized_save(&root, &source, output);
    }
}

fn normalized_save(root: &Path, source: &Path, output: ExportOutput) {
    let original = std::fs::read(source).expect("source bytes");
    let (send, events) = std::sync::mpsc::channel();
    let mut app = Application::new(None, move |event| {
        let _ = send.send(event);
    })
    .expect("app");
    context(&mut app);
    let tab = app.tabs.open_new(source.into(), MediaKind::Video);
    app.path = Some(source.into());
    app.media_kind = Some(MediaKind::Video);
    app.media_duration = Some(Duration::from_secs(1));
    app.audio_export_settings.insert(
        tab,
        AudioExportOptions {
            normalize_peak: true,
            ..Default::default()
        },
    );
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::SetRate(2.0), MediaKind::Video);
    let target = root.join(if output == ExportOutput::Media {
        "saved.avi"
    } else {
        "saved.wav"
    });
    assert!(app.start_export(
        tab,
        source.into(),
        MediaKind::Video,
        target.clone(),
        None,
        output
    ));
    assert_eq!(
        app.active_export.as_ref().expect("job").progress.duration,
        Some(Duration::from_millis(500))
    );
    assert!(app.active_export.as_ref().expect("job").analyzing_audio);
    let other = app.tabs.open_new(root.join("other.jpg"), MediaKind::Image);
    app.path = Some(root.join("other.jpg"));
    app.media_kind = Some(MediaKind::Image);
    app.media_duration = Some(Duration::from_secs(300));
    let size = egui::vec2(640.0, 480.0);
    let mut analyzed = false;
    let mut encoded = false;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut time = 0.0;
    while app.active_export.is_some() {
        if let AppEvent::Export(event) = events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("worker event")
        {
            analyzed |= matches!(event, ExportEvent::AnalyzingAudio(_));
            encoded |= matches!(event, ExportEvent::Progress(_));
            app.handle_export_event(event);
            time += 0.1;
            let output = paint(&mut app, size, 1.0, time, vec![]);
            if let Some(export) = &app.active_export {
                assert_eq!(export.tab, tab);
                assert_eq!(export.progress.duration, Some(Duration::from_millis(500)));
                assert!(
                    indicator(&output)
                        .expect("job progress")
                        .numeric_value()
                        .expect("estimate")
                        < 100.0
                );
            } else {
                assert!(indicator(&output).is_none() && fill(&output).is_none());
            }
        }
    }
    assert!(analyzed && encoded);
    assert!(app.export_error.is_none(), "{:?}", app.export_error);
    assert_eq!(
        app.edits[&tab].is_dirty(),
        output == ExportOutput::AudioOnly,
        "only ordinary media save marks the original history saved"
    );
    assert_eq!(app.tabs.active().expect("other tab").id, other);
    assert!(target.is_file());
    assert_eq!(std::fs::read(source).expect("source unchanged"), original);
}
