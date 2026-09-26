use super::*;
use towavue_core::{TimeRange, TimelineEdit};

fn ms(value: i64) -> MediaTime {
    MediaTime::from_nanoseconds(value * 1_000_000)
}

fn fixture(root: &Path) -> PathBuf {
    let first = root.join("first.srt");
    let second = root.join("second.srt");
    for (path, text) in [(&first, "Main subtitle"), (&second, "Alternate subtitle")] {
        std::fs::write(path, format!("1\n00:00:02,000 --> 00:00:04,000\n{text}\n"))
            .expect("subtitle fixture");
    }
    let path = root.join("subtitled.mkv");
    let ffmpeg =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
    let output = crate::tests::hidden_command(ffmpeg)
        .args([
            "-v",
            "error",
            "-n",
            "-f",
            "lavfi",
            "-i",
            "color=size=160x96:rate=25:duration=8",
        ])
        .arg("-i")
        .arg(first)
        .arg("-i")
        .arg(second)
        .args([
            "-map", "0:v", "-map", "1:s", "-map", "2:s", "-c:v", "mpeg4", "-c:s", "srt",
        ])
        .arg(&path)
        .output()
        .expect("generate captioned video");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    path
}

fn frame(app: &mut TestApp, size: egui::Vec2, pointer: egui::Pos2) -> egui::FullOutput {
    let context = app.ui_context.clone().expect("context");
    context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events: vec![egui::Event::PointerMoved(pointer)],
            focused: true,
            ..Default::default()
        },
        |root| {
            let mut actions = Vec::new();
            app.draw_ui(root, &mut actions);
            assert!(actions.is_empty(), "passive caption layout");
        },
    )
}

fn caption_bounds(output: &egui::FullOutput, text: &str) -> Option<egui::Rect> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(shape)
            if shape.galley.text() == text && shape.override_text_color.is_none() =>
        {
            Some(shape.galley.rect.translate(shape.pos.to_vec2()))
        }
        _ => None,
    })
}

fn assert_caption(app: &mut TestApp, position: i64, expected: Option<&str>) {
    app.seek_to(ms(position));
    assert_eq!(app.current_position(), ms(position));
    let size = egui::vec2(800.0, 500.0);
    frame(app, size, egui::pos2(400.0, 100.0));
    let output = frame(app, size, egui::pos2(400.0, 100.0));
    for text in ["Main subtitle", "Alternate subtitle"] {
        assert_eq!(
            caption_bounds(&output, text).is_some(),
            expected == Some(text),
            "caption {text} at edited {position} ms"
        );
    }
}

#[test]
fn hidden_embedded_captions_follow_edits_rate_transfer_and_rename() {
    let Some(root) = crate::tests::isolated_test_root(
        "subtitles::tests::lifecycle::hidden_embedded_captions_follow_edits_rate_transfer_and_rename",
    ) else {
        return;
    };
    struct Trial(PathBuf);
    impl winit::application::ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let (mut app, events) = app(&self.0);
            let window = Arc::new(
                event_loop
                    .create_window(Window::default_attributes().with_visible(false))
                    .expect("hidden window"),
            );
            app.renderer = Some(FrameRenderer::new(&window).expect("D3D11"));
            app.window = Some(window);
            app.load_path(self.0.clone(), MediaKind::Video);
            assert_eq!(
                app.state,
                PlaybackState::Playing,
                "{:?}",
                app.playback_error
            );
            app.toggle_pause();
            app.media_duration = Some(Duration::from_secs(8));
            let tab = app.displayed_tab.expect("tab");
            let tracks = app
                .session
                .as_ref()
                .expect("session")
                .subtitle_tracks()
                .to_vec();
            assert_eq!(tracks.len(), 2);
            let original_history = app.edits.clone();
            for (track, text) in tracks.iter().zip(["Main subtitle", "Alternate subtitle"]) {
                app.apply_subtitle_action(Action::Select(Selection::Embedded(track.id)));
                complete(&mut app, &events);
                assert_caption(&mut app, 2500, Some(text));
                assert_eq!(app.edits, original_history);
                assert_eq!(app.state, PlaybackState::Paused);
            }
            let range = |a, b| TimeRange::new(ms(a), ms(b)).expect("range");
            app.push_edit(EditOperation::SetTrimStart(ms(1000)));
            app.push_edit(EditOperation::Timeline(TimelineEdit::Delete(range(
                1000, 2000,
            ))));
            app.push_edit(EditOperation::Timeline(TimelineEdit::Stretch(
                range(0, 1000),
                ms(2000),
            )));
            let history = app.edits.clone();
            assert_eq!(
                app.session
                    .as_ref()
                    .expect("edited session")
                    .timeline()
                    .expect("edited source mapping")
                    .source_time(ms(2250)),
                Some(ms(3250))
            );
            app.set_preview_rate(1.75);
            assert_caption(&mut app, 2250, Some("Alternate subtitle"));
            assert_caption(&mut app, 3100, None);
            app.apply_subtitle_action(Action::Delay(SubtitleDelay::from_tenths(2)));
            assert_caption(&mut app, 3100, Some("Alternate subtitle"));
            app.apply_subtitle_action(Action::Delay(SubtitleDelay::from_tenths(-2)));
            assert_caption(&mut app, 2900, None);
            app.apply_subtitle_action(Action::Delay(SubtitleDelay::default()));
            assert_caption(&mut app, 2250, Some("Alternate subtitle"));
            app.apply_subtitle_action(Action::Show(false));
            assert_caption(&mut app, 2250, None);
            app.apply_subtitle_action(Action::Show(true));
            assert_caption(&mut app, 2250, Some("Alternate subtitle"));
            app.toggle_pause();
            assert_eq!(app.state, PlaybackState::Playing);
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut seen = [false; 2];
            while app.current_position() < ms(3250) {
                assert!(Instant::now() < deadline, "advancing native video clock");
                while let Ok(event) = events.try_recv() {
                    app.handle_app_event(event);
                }
                app.load_next_frame();
                app.advance_media();
                let before = app.current_position();
                let output = frame(&mut app, egui::vec2(800.0, 500.0), egui::pos2(400.0, 100.0));
                let after = app.current_position();
                assert!(after >= before, "forward playback");
                // Avoid classifying a frame whose layout spans the cue boundary.
                // The edited 3s boundary corresponds to source 4s after these edits.
                if after < ms(2950) {
                    assert!(caption_bounds(&output, "Alternate subtitle").is_some());
                    seen[0] = true;
                } else if before > ms(3050) {
                    assert!(caption_bounds(&output, "Alternate subtitle").is_none());
                    seen[1] = true;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(
                seen,
                [true, true],
                "caption enters and leaves on the running clock"
            );
            app.toggle_pause();
            assert_eq!(app.state, PlaybackState::Paused);
            assert_caption(&mut app, 2250, Some("Alternate subtitle"));
            assert_eq!(app.edits, history);
            assert_eq!(app.session.as_ref().expect("session").rate(), 1.75);
            let settings = app.subtitle_settings();
            let document = app
                .subtitle_choice()
                .expect("choice")
                .document
                .clone()
                .expect("document");
            let request = app.tab_detach_request(tab).expect("transfer request");
            let transfer = app.take_tab_transfer(&request, None);
            assert!(!app.subtitles.choices.contains_key(&tab));
            let (mut destination, destination_events) =
                super::app(&self.0.with_file_name("placeholder.mp4"));
            let empty = destination.displayed_tab.expect("placeholder");
            destination.remove_tab(empty, false);
            let window = Arc::new(
                event_loop
                    .create_window(Window::default_attributes().with_visible(false))
                    .expect("hidden destination"),
            );
            destination.renderer = Some(
                FrameRenderer::with_graphics_device(
                    &window,
                    app.renderer.as_ref().expect("renderer").graphics_device(),
                )
                .expect("shared device"),
            );
            destination.window = Some(window);
            let moved = destination.accept_tab_transfer(transfer, 0);
            assert_eq!(destination.subtitle_settings(), settings);
            assert_eq!(destination.edits.get(&moved), history.get(&tab));
            assert!(Arc::ptr_eq(
                &document,
                destination
                    .subtitle_choice()
                    .expect("choice")
                    .document
                    .as_ref()
                    .expect("moved document")
            ));
            assert_caption(&mut destination, 2250, Some("Alternate subtitle"));
            destination.quiesce_file_relocation(&self.0);
            let version = towavue_runtime_windows::FileOperationSource::capture(&self.0)
                .expect("loaded file identity");
            assert_eq!(destination.source_versions[&moved].as_ref(), Some(&version));
            let renamed = self.0.with_file_name("renamed.mkv");
            std::fs::rename(&self.0, &renamed).expect("rename generated media");
            let current = version.after_move(&renamed).expect("moved file identity");
            destination.finish_file_relocation(
                &self.0,
                Some(&file_operations::Completed {
                    versions: Some(Box::new(file_operations::RelocatedVersions {
                        original: version,
                        current: Some(current),
                    })),
                    outcome: towavue_runtime_windows::FileOperationOutcome::Moved(renamed.clone()),
                    resume: None,
                    recycle: None,
                    preference_warning: None,
                }),
            );
            assert_eq!(destination.path.as_ref(), Some(&renamed));
            assert_eq!(destination.subtitle_settings(), settings);
            assert_caption(&mut destination, 2250, Some("Alternate subtitle"));
            let original = std::fs::read(&renamed).expect("original video");
            let target = renamed.with_file_name("saved.mkv");
            destination.start_test_save_as(target.clone(), None);
            crate::source_save::tests::finish(&mut destination, &destination_events);
            assert!(
                destination.export_error.is_none(),
                "{:?}",
                destination.export_error
            );
            assert_eq!(destination.path.as_ref(), Some(&target));
            assert_eq!(destination.subtitle_settings(), settings);
            assert!(Arc::ptr_eq(
                &document,
                destination
                    .subtitle_choice()
                    .expect("saved choice")
                    .document
                    .as_ref()
                    .expect("saved document")
            ));
            // Force a fresh embedded read after Save As; the stream identity and
            // cue times must still come from the retained original, not output.
            destination.apply_subtitle_action(Action::Select(Selection::Embedded(tracks[0].id)));
            complete(&mut destination, &destination_events);
            assert_caption(&mut destination, 2250, Some("Main subtitle"));
            destination.push_edit(EditOperation::SetVolume(0.5));
            assert!(destination.save_source(None));
            crate::source_save::tests::finish(&mut destination, &destination_events);
            assert!(
                destination.export_error.is_none(),
                "{:?}",
                destination.export_error
            );
            destination.apply_subtitle_action(Action::Select(Selection::Embedded(tracks[1].id)));
            complete(&mut destination, &destination_events);
            assert_caption(&mut destination, 2250, Some("Alternate subtitle"));
            assert_eq!(
                std::fs::read(&renamed).expect("untouched original"),
                original
            );
            destination.remove_tab(moved, false);
            assert!(!destination.subtitles.choices.contains_key(&moved));
            event_loop.exit();
        }
        fn window_event(
            &mut self,
            _: &ActiveEventLoop,
            _: winit::window::WindowId,
            _: WindowEvent,
        ) {
        }
    }
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut Trial(fixture(&root)))
        .expect("hidden trial");
}

#[test]
fn fullscreen_captions_stay_above_controls_without_moving_on_hover() {
    let Some(root) = crate::tests::isolated_test_root(
        "subtitles::tests::lifecycle::fullscreen_captions_stay_above_controls_without_moving_on_hover",
    ) else {
        return;
    };
    let (mut app, _) = app(&root.join("layout.mp4"));
    let owner = app.subtitle_owner().expect("owner");
    let choice = app.ensure_subtitle_choice(&owner);
    choice.settings.visible = true;
    choice.settings.selection = Selection::External;
    choice.document = Some(Arc::new(SubtitleTimeline::new(vec![
        SubtitleCue::new(
            ms(0),
            ms(10000),
            SubtitleContent::Text("Main subtitle".into()),
        )
        .expect("cue"),
    ])));
    app.fullscreen = true;
    for density in [1.0, 1.25, 2.0] {
        app.ui_context = Some(crate::localization::test_ui::japanese_context(density));
        for size in [egui::vec2(800.0, 500.0), egui::vec2(360.0, 200.0)] {
            let mut previous = None;
            for hover in [false, true, false] {
                let pointer = egui::pos2(size.x * 0.5, if hover { size.y - 5.0 } else { 50.0 });
                frame(&mut app, size, pointer);
                let output = frame(&mut app, size, pointer);
                assert_eq!(app.fullscreen_controls_visible, hover);
                let bounds = caption_bounds(&output, "Main subtitle").expect("caption");
                assert!(
                    bounds.bottom() < size.y - 48.0,
                    "caption must clear fullscreen controls: {bounds:?}, {size:?}"
                );
                if let Some(previous) = previous {
                    assert_eq!(bounds, previous, "stable caption baseline");
                }
                previous = Some(bounds);
            }
        }
    }
}
