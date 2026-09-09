use super::*;
use std::os::windows::process::CommandExt;
use towavue_core::{TimeRange, TimelineEdit};
use winit::platform::windows::EventLoopBuilderExtWindows;

fn time(ms: i64) -> MediaTime {
    MediaTime::from_nanoseconds(ms * 1_000_000)
}
fn range(a: i64, b: i64) -> TimeRange {
    TimeRange::new(time(a), time(b)).expect("range")
}

fn render<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    context: &egui::Context,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Vec<UiAction>) {
    let mut actions = Vec::new();
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(960.0, 576.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.draw_ui(ui, &mut actions),
    );
    (output, actions)
}

#[test]
fn source_identity_remaps_deleted_positions_stretches_and_empty_undo() {
    let mut plan = EditTimeline::new(time(4000), PlaybackRange::default()).expect("plan");
    assert!(plan.apply(TimelineEdit::Delete(range(1000, 2000))));
    assert!(plan.apply(TimelineEdit::Stretch(range(1000, 2000), time(2000))));
    for (source, edited) in [
        (0, 0),
        (500, 500),
        (1500, 1000),
        (2000, 1000),
        (2500, 2000),
        (3000, 3000),
        (4000, 4000),
    ] {
        assert_eq!(
            remap_position(None, Some(&plan), time(source)),
            time(edited)
        );
    }
    assert_eq!(remap_position(Some(&plan), None, time(2000)), time(2500));
    let mut keep = plan.clone();
    assert!(keep.apply(TimelineEdit::Keep(range(1000, 3000))));
    assert_eq!(remap_position(Some(&plan), Some(&keep), time(0)), time(0));
    assert_eq!(
        remap_position(Some(&plan), Some(&keep), time(3500)),
        time(2000)
    );
    let mut empty = plan.clone();
    assert!(empty.apply(TimelineEdit::Delete(range(0, 4000))));
    assert_eq!(
        remap_position(Some(&plan), Some(&empty), time(2000)),
        time(0)
    );
    assert_eq!(remap_position(Some(&empty), Some(&plan), time(0)), time(0));
}

#[test]
fn waveform_regions_follow_source_uv_edited_width_gain_and_mute() {
    let mut plan = EditTimeline::new(time(4000), PlaybackRange::default()).expect("plan");
    assert!(plan.apply(TimelineEdit::Delete(range(1000, 2000))));
    assert!(plan.apply(TimelineEdit::Stretch(range(1000, 2000), time(2000))));
    assert!(plan.apply(TimelineEdit::SetVolume(range(1000, 3000), 0.5)));
    let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(400.0, 100.0));
    let regions = waveform_regions(rect, Duration::from_secs(4), &plan, 2.0);
    assert_eq!(regions.len(), 3);
    for ((destination, uv), (left, width, height, u0, u1)) in regions.iter().zip([
        (10.0, 100.0, 200.0, 0.0, 0.25),
        (110.0, 200.0, 100.0, 0.5, 0.75),
        (310.0, 100.0, 200.0, 0.75, 1.0),
    ]) {
        assert_eq!(
            (
                destination.left(),
                destination.width(),
                destination.height(),
                destination.center().y
            ),
            (left, width, height, 70.0)
        );
        assert_eq!((uv.left(), uv.right()), (u0, u1));
    }
    assert!(plan.apply(TimelineEdit::SetVolume(range(1000, 3000), 0.0)));
    assert_eq!(
        waveform_regions(rect, Duration::from_secs(4), &plan, 1.0).len(),
        2
    );
    assert!(waveform_regions(rect, Duration::from_secs(4), &plan, 0.0).is_empty());
    assert!(plan.apply(TimelineEdit::Delete(range(0, 4000))));
    assert!(waveform_regions(rect, Duration::from_secs(4), &plan, 1.0).is_empty());
}

#[test]
fn app_timeline_history_seek_background_duration_and_empty_round_trip() {
    let Some(root) = crate::tests::isolated_test_root(
        "timeline_edit::tests::app_timeline_history_seek_background_duration_and_empty_round_trip",
    ) else {
        return;
    };
    run_app_trial(root, false);
}

#[test]
#[ignore = "requires a live Windows shared-mode audio endpoint; generated media plays muted"]
fn edited_audio_video_history_keeps_clock_and_background_eof_in_edited_time() {
    let Some(root) = crate::tests::isolated_test_root(
        "timeline_edit::tests::edited_audio_video_history_keeps_clock_and_background_eof_in_edited_time",
    ) else {
        return;
    };
    if let Err(error) =
        towavue_runtime_windows::AudioOutput::start(towavue_runtime_windows::AudioFormat {
            sample_rate: 48000,
            channels: 2,
        })
    {
        eprintln!("SKIP edited audio application: shared-mode endpoint unavailable: {error}");
        return;
    }
    run_app_trial(root, true);
}

fn run_app_trial(root: PathBuf, audio: bool) {
    let path = root.join("timeline.mp4");
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
    let ffmpeg =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
    let mut command = std::process::Command::new(ffmpeg);
    command
        .creation_flags(0x0800_0000)
        .args(["-v", "error", "-i"])
        .arg(source);
    if !audio {
        command.arg("-an");
    }
    let generated = command
        .args(["-c", "copy"])
        .arg(&path)
        .output()
        .expect("owned video");
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    struct Trial {
        path: PathBuf,
        audio: bool,
        completed: bool,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = Arc::new(
                event_loop
                    .create_window(Window::default_attributes().with_visible(false))
                    .expect("owned hidden window"),
            );
            let renderer = match FrameRenderer::new(&window) {
                Ok(renderer) => renderer,
                Err(error) => {
                    eprintln!("SKIP timeline application: D3D11 unavailable: {error}");
                    self.completed = true;
                    event_loop.exit();
                    return;
                }
            };
            let mut app = Application::new(None, |_| {}).expect("app");
            app.window = Some(window);
            app.renderer = Some(renderer);
            let tab = app.tabs.open_new(self.path.clone(), MediaKind::Video);
            let mut baseline = EditHistory::default();
            baseline.push(
                EditOperation::SetVolume(if self.audio { 0.0 } else { 1.0 }),
                MediaKind::Video,
            );
            baseline.mark_saved();
            app.edits.insert(tab, baseline);
            app.load_path(self.path.clone(), MediaKind::Video);
            app.media_duration = Some(Duration::from_secs(2));
            app.session
                .as_mut()
                .expect("session")
                .set_paused(true)
                .expect("pause");
            app.state = PlaybackState::Paused;
            app.seek_to(time(1200));
            app.timeline_open = true;
            app.handle_ui_action(UiAction::TimeSelection(
                tab,
                app.generation,
                Some(range(500, 1000)),
            ));
            assert!(!app.edits[&tab].is_dirty(), "selection is not an edit");
            app.process_shortcut("Delete".parse().expect("Delete key"));
            assert!(app.time_selection.is_none());
            assert_eq!(app.current_position(), time(700));
            assert_eq!(app.playback_duration(), Some(Duration::from_millis(1500)));
            assert_eq!(app.media_duration, Some(Duration::from_secs(2)));
            app.handle_ui_action(UiAction::TimeSelection(
                tab,
                app.generation,
                Some(range(250, 1250)),
            ));
            app.process_shortcut("Ctrl+Y".parse().expect("keep shortcut"));
            assert_eq!(app.playback_duration(), Some(Duration::from_secs(1)));
            if !self.audio {
                let plan = app.history_timeline().expect("history").expect("plan");
                let mut expected_frames = 0;
                towavue_runtime_windows::decode_file(&self.path, |output| {
                    if let towavue_runtime_windows::DecodeOutput::Video(frame) = output
                        && plan.edited_time(frame.presentation_time).is_some()
                    {
                        expected_frames += 1;
                    }
                    true
                })
                .expect("decode original reference");
                let target = self.path.with_file_name("selected.mp4");
                towavue_runtime_windows::export_media(&towavue_runtime_windows::ExportRequest {
                    source: self.path.clone(),
                    target: target.clone(),
                    kind: MediaKind::Video,
                    operations: app.edits[&tab].operations().to_vec(),
                    hardware_encode: false,
                })
                .expect("export UI-created history");
                let mut actual_frames = 0;
                towavue_runtime_windows::decode_file(&target, |output| {
                    if matches!(output, towavue_runtime_windows::DecodeOutput::Video(_)) {
                        actual_frames += 1;
                    }
                    true
                })
                .expect("reopen selected export");
                assert_eq!(actual_frames, expected_frames);
                assert!(actual_frames > 0);
            }
            app.undo_edit(false);
            assert_eq!(app.current_position(), time(700));
            app.time_selection = Some(range(500, 1000));
            app.handle_ui_action(UiAction::TimeAdjustment(
                tab,
                app.generation,
                app.time_selection,
                TimelineEdit::SetVolume(range(500, 1000), 0.5),
            ));
            assert_eq!(app.current_position(), time(700));
            assert_eq!(app.time_selection, Some(range(500, 1000)));
            assert_eq!(
                app.session
                    .as_ref()
                    .expect("session")
                    .timeline()
                    .expect("gain plan")
                    .spans()[1]
                    .volume(),
                0.5
            );
            app.undo_edit(false);
            app.time_selection = Some(range(500, 1000));
            app.handle_ui_action(UiAction::TimeAdjustment(
                tab,
                app.generation,
                app.time_selection,
                TimelineEdit::Stretch(range(500, 1000), time(1000)),
            ));
            assert_eq!(app.time_selection, Some(range(500, 1500)));
            assert_eq!(app.current_position(), time(900));
            app.push_edit(EditOperation::Timeline(TimelineEdit::Stretch(
                range(0, 2000),
                time(4000),
            )));
            assert_eq!(app.current_position(), time(1800));
            app.seek_to(time(3500));
            assert_eq!(
                app.current_position(),
                time(3500),
                "edited position must exceed source duration"
            );
            app.undo_edit(false);
            assert_eq!(app.current_position(), time(1750));
            app.undo_edit(false);
            assert_eq!(app.current_position(), time(1250));
            app.undo_edit(false);
            assert_eq!(app.current_position(), time(1750));
            assert!(app.session.as_ref().expect("session").timeline().is_none());
            let history = app.edits[&tab].clone();
            let generation = app.generation;
            app.push_edit(EditOperation::Timeline(TimelineEdit::Delete(range(
                0, 9000,
            ))));
            assert_eq!(
                app.edits[&tab], history,
                "invalid edit must preserve the redo branch"
            );
            assert_eq!(app.generation, generation);
            for _ in 0..3 {
                app.undo_edit(true);
            }
            assert_eq!(app.current_position(), time(3500));
            assert_eq!(app.playback_duration(), Some(Duration::from_secs(4)));
            let context = fonts::test_context();
            context.enable_accesskit();
            app.ui_context = Some(context.clone());
            app.timeline_open = true;
            let waveform = context.load_texture(
                "owned waveform",
                egui::ColorImage::new([8, 8], vec![Color32::WHITE; 64]),
                TextureOptions::LINEAR,
            );
            let texture = waveform.id();
            app.waveform = Some(waveform);
            render(&mut app, &context, vec![]);
            let (output, _) = render(&mut app, &context, vec![]);
            let tree = output
                .platform_output
                .accesskit_update
                .expect("edited timeline accessibility");
            let (slider, node) = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some("Playback position (seconds)"))
                .expect("edited seek slider");
            assert_eq!(node.max_numeric_value(), Some(4.0));
            assert_eq!(node.numeric_value(), Some(3.5));
            assert!(
                !tree
                    .nodes
                    .iter()
                    .any(|(_, node)| node.label().is_some_and(|label| label.starts_with("Trim ")))
            );
            let meshes: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.texture_id == texture => {
                        Some((shape.clip_rect, mesh))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                meshes.len(),
                if self.audio { 0 } else { 3 },
                "only audible retained waveform source regions"
            );
            let uv: Vec<_> = meshes
                .iter()
                .map(|(_, mesh)| {
                    (
                        mesh.vertices
                            .iter()
                            .map(|vertex| vertex.uv.x)
                            .fold(f32::INFINITY, f32::min),
                        mesh.vertices
                            .iter()
                            .map(|vertex| vertex.uv.x)
                            .fold(f32::NEG_INFINITY, f32::max),
                    )
                })
                .collect();
            assert_eq!(
                uv,
                if self.audio {
                    vec![]
                } else {
                    vec![(0.0, 0.25), (0.5, 0.75), (0.75, 1.0)]
                }
            );
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().contains("00:03 / 00:04"))));
            let (_, actions) = render(
                &mut app,
                &context,
                vec![egui::Event::AccessKitActionRequest(
                    egui::accesskit::ActionRequest {
                        action: egui::accesskit::Action::SetValue,
                        target_tree: egui::accesskit::TreeId::ROOT,
                        target_node: *slider,
                        data: Some(egui::accesskit::ActionData::NumericValue(3.25)),
                    },
                )],
            );
            assert!(
                matches!(actions.as_slice(), [UiAction::Seek(position)] if *position == time(3250))
            );
            for action in actions {
                app.handle_ui_action(action);
            }
            assert_eq!(app.current_position(), time(3250));
            app.seek_to(time(3500));
            let generation = app.generation;
            app.push_edit(EditOperation::SetVolume(if self.audio { 0.0 } else { 0.5 }));
            assert_eq!(
                app.generation, generation,
                "master volume does not restart audio"
            );
            app.push_edit(EditOperation::SetRate(4.0));
            assert_eq!(app.current_position(), time(3500));
            assert_eq!(app.playback_duration(), Some(Duration::from_secs(4)));
            let history = app.edits[&tab].clone();
            app.push_edit(EditOperation::SetTrimStart(time(500)));
            assert_eq!(
                app.edits[&tab], history,
                "source trim cannot mutate an edited axis"
            );
            app.toggle_pause();
            app.time_selection = Some(range(1000, 2000));
            let other = app
                .tabs
                .open_new(self.path.with_file_name("other.png"), MediaKind::Image);
            app.load_path(self.path.with_file_name("other.png"), MediaKind::Image);
            let saved = app
                .retained_playback
                .get_mut(&tab)
                .expect("retained timeline");
            assert_eq!(saved.state, PlaybackState::Playing);
            assert!(saved.position() >= time(3500));
            let deadline = Instant::now() + Duration::from_secs(3);
            while saved.state == PlaybackState::Playing {
                saved.poll();
                assert!(Instant::now() < deadline, "edited background EOF");
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(saved.state, PlaybackState::Ended);
            assert_eq!(saved.position(), time(4000));
            app.activate_tab(tab);
            assert_eq!(app.current_position(), time(4000));
            assert_eq!(app.time_selection, Some(range(1000, 2000)));
            app.push_edit(EditOperation::Timeline(TimelineEdit::Delete(range(
                0, 4000,
            ))));
            assert_eq!(app.current_position(), time(0));
            assert_eq!(app.playback_duration(), Some(Duration::ZERO));
            let (output, _) = render(&mut app, &context, vec![]);
            assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == texture)));
            assert!(!app.session.as_ref().expect("empty").has_audio());
            let generation = app.generation;
            app.toggle_pause();
            assert_eq!(
                app.generation, generation,
                "empty play must not start a pipeline"
            );
            app.activate_tab(other);
            app.activate_tab(tab);
            assert_eq!(app.playback_duration(), Some(Duration::ZERO));
            app.undo_edit(false);
            assert_eq!(app.current_position(), time(0));
            assert_eq!(app.playback_duration(), Some(Duration::from_secs(4)));
            app.seek_to(time(9000));
            assert_eq!(app.current_position(), time(4000));
            assert!(app.playback_error.is_none());
            drop(app);
            self.completed = true;
            eprintln!(
                "PASS timeline app: atomic history, source-preserving Undo, edited Seek/duration, background EOF and empty restoration"
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let event_loop = EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("test event loop");
    let mut trial = Trial {
        path,
        audio,
        completed: false,
    };
    event_loop.run_app(&mut trial).expect("timeline app trial");
    assert!(trial.completed);
}
