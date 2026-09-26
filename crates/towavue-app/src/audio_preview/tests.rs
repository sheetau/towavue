use super::*;

fn app() -> Application<Box<dyn Fn(AppEvent) + Send + Sync>> {
    Application::new(
        None,
        Box::new(|_| {}) as Box<dyn Fn(AppEvent) + Send + Sync>,
    )
    .expect("app")
}

#[test]
fn waveform_completion_requires_current_track_and_request_even_with_same_source() {
    let Some(_root) = crate::tests::isolated_test_root(
        "audio_preview::tests::waveform_completion_requires_current_track_and_request_even_with_same_source",
    ) else {
        return;
    };
    let mut app = app();
    app.ui_context = Some(fonts::test_context());
    let path = PathBuf::from("source.mp4");
    let tab = app.tabs.open_new(path.clone(), MediaKind::Video);
    let track = AudioTrackId::from_index(2);
    app.audio_preview_choices.insert(
        tab,
        Choice {
            path: path.clone(),
            selection: AudioTrackSelection::Track(track),
        },
    );
    app.displayed_tab = Some(tab);
    app.path = Some(path.clone());
    app.media_kind = Some(MediaKind::Video);
    app.waveform_request = 7;
    app.waveform_loading = true;
    let preview = towavue_runtime_windows::PreviewImage {
        width: 1,
        height: 1,
        rgba: Arc::new(vec![255; 4]),
    };
    for request in [
        (6, Some(track)),
        (7, None),
        (7, Some(AudioTrackId::from_index(1))),
    ] {
        for result in [
            Ok(preview.clone()),
            Err(towavue_runtime_windows::PreviewError::Generate(
                "old failure".into(),
            )),
        ] {
            app.handle_app_event(AppEvent::Waveform(
                path.clone(),
                app.media_generation,
                request,
                result,
            ));
            assert!(app.waveform_loading);
            assert!(app.waveform.is_none());
            assert!(app.status_message.is_none());
        }
    }
    app.handle_app_event(AppEvent::Waveform(
        path,
        app.media_generation,
        (7, Some(track)),
        Ok(preview),
    ));
    assert!(app.waveform.is_some());
    assert!(!app.waveform_loading);
}

#[test]
fn hidden_session_switches_tracks_without_changing_edits_and_moves_choice_with_tab() {
    let Some(root) = crate::tests::isolated_test_root(
        "audio_preview::tests::hidden_session_switches_tracks_without_changing_edits_and_moves_choice_with_tab",
    ) else {
        return;
    };
    let path = root.join("tracks.mkv");
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
            "color=size=160x96:rate=25:duration=4",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=44100:cl=stereo:d=4",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=48000:cl=stereo:d=4",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-c:v",
            "mpeg4",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(&path)
        .output()
        .expect("silent fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    struct Trial(PathBuf);
    impl winit::application::ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = Arc::new(
                event_loop
                    .create_window(Window::default_attributes().with_visible(false))
                    .expect("hidden window"),
            );
            let mut app = app();
            app.renderer = Some(FrameRenderer::new(&window).expect("D3D11"));
            app.window = Some(window.clone());
            app.ui_context = Some(fonts::test_context());
            let tab = app.tabs.open_new(self.0.clone(), MediaKind::Video);
            app.load_path(self.0.clone(), MediaKind::Video);
            assert_eq!(
                app.state,
                PlaybackState::Playing,
                "{:?}",
                app.playback_error
            );
            app.media_duration = Some(Duration::from_secs(4));
            app.push_edit(EditOperation::SetVolume(0.0));
            app.set_preview_rate(1.25);
            app.toggle_pause();
            let target = MediaTime::from_nanoseconds(700_000_000);
            app.seek_to(target);
            let history = app.edits.clone();
            let plan = app.session.as_ref().expect("session").timeline().cloned();
            let bounds = app.session.as_ref().expect("session").range();
            let tracks: Vec<_> = app
                .session
                .as_ref()
                .expect("session")
                .audio_tracks()
                .tracks
                .iter()
                .map(|track| track.id)
                .collect();
            assert_eq!(tracks.len(), 2);
            let retention = towavue_core::AudioTrackRetention::Selected(vec![tracks[1]]);
            crate::audio_export::track_tests::choose(&mut app, retention.clone());
            let mut last_generation = app.generation;
            for selection in [
                AudioTrackSelection::Track(tracks[1]),
                AudioTrackSelection::All,
                AudioTrackSelection::Default,
            ] {
                app.select_audio_track(selection);
                assert_ne!(app.generation, last_generation);
                last_generation = app.generation;
                assert_eq!(app.state, PlaybackState::Paused);
                assert_eq!(app.audio_selection(), selection);
                let session = app.session.as_ref().expect("session");
                assert_eq!(session.audio_selection(), selection);
                assert_eq!(session.target(), target);
                assert_eq!(session.rate(), 1.25);
                assert_eq!(session.range(), bounds);
                assert_eq!(session.timeline(), plan.as_ref());
                assert_eq!(app.edits, history);
                assert_eq!(
                    app.waveform_audio_track(),
                    match selection {
                        AudioTrackSelection::All => Some(tracks[0]),
                        AudioTrackSelection::Track(id) => Some(id),
                        _ => None,
                    }
                );
            }
            app.select_audio_track(AudioTrackSelection::Track(tracks[1]));
            app.cycle_audio_track();
            assert_eq!(app.audio_selection(), AudioTrackSelection::Track(tracks[0]));
            let generation = app.generation;
            app.select_audio_track(AudioTrackSelection::Track(AudioTrackId::from_index(99)));
            assert_eq!(app.generation, generation);
            app.about_open = true;
            app.select_audio_track(AudioTrackSelection::All);
            assert_eq!(app.generation, generation);
            app.about_open = false;
            let request = app.tab_detach_request(tab).expect("transfer request");
            let transfer = app.take_tab_transfer(&request, None);
            assert!(!app.audio_export_tracks.contains_key(&tab));
            assert!(!app.audio_preview_choices.contains_key(&tab));
            let mut destination = self::app();
            destination.ui_context = Some(fonts::test_context());
            let destination_window = Arc::new(
                event_loop
                    .create_window(Window::default_attributes().with_visible(false))
                    .expect("hidden destination"),
            );
            destination.renderer = Some(
                FrameRenderer::with_graphics_device(
                    &destination_window,
                    app.renderer.as_ref().expect("renderer").graphics_device(),
                )
                .expect("destination renderer on shared device"),
            );
            destination.window = Some(destination_window);
            let moved = destination.accept_tab_transfer(transfer, 0);
            assert_eq!(destination.audio_retention_for(moved, &self.0), retention);
            assert_eq!(
                destination.audio_selection_for(Some(moved), &self.0),
                AudioTrackSelection::Track(tracks[0])
            );
            assert_eq!(
                destination.audio_selection(),
                AudioTrackSelection::Track(tracks[0])
            );
            assert_eq!(
                destination
                    .session
                    .as_ref()
                    .expect("moved session")
                    .audio_selection(),
                AudioTrackSelection::Track(tracks[0])
            );
            assert_eq!(destination.edits.get(&moved), history.get(&tab));
            destination.quiesce_file_relocation(&self.0);
            let renamed = self.0.with_file_name("renamed.mkv");
            std::fs::rename(&self.0, &renamed).expect("rename owned fixture");
            destination.finish_file_relocation(
                &self.0,
                Some(&file_operations::Completed {
                    versions: None,
                    outcome: towavue_runtime_windows::FileOperationOutcome::Moved(renamed.clone()),
                    resume: None,
                    recycle: None,
                    preference_warning: None,
                }),
            );
            assert_eq!(destination.path.as_ref(), Some(&renamed));
            assert_eq!(destination.audio_retention_for(moved, &renamed), retention);
            assert_eq!(
                destination.audio_retention_for(moved, &self.0),
                towavue_core::AudioTrackRetention::All
            );
            assert_eq!(
                destination.audio_selection(),
                AudioTrackSelection::Track(tracks[0])
            );
            assert_eq!(
                destination
                    .session
                    .as_ref()
                    .expect("renamed session")
                    .audio_selection(),
                AudioTrackSelection::Track(tracks[0])
            );
            destination.select_audio_track(AudioTrackSelection::All);
            assert_eq!(destination.state, PlaybackState::Paused);
            assert_eq!(destination.audio_selection(), AudioTrackSelection::All);
            destination.remove_tab(moved, false);
            assert!(!destination.audio_export_tracks.contains_key(&moved));
            assert!(!destination.audio_preview_choices.contains_key(&moved));
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
        .run_app(&mut Trial(path))
        .expect("hidden trial");
}

#[test]
fn choices_are_owned_by_tab_and_source_and_do_not_modify_edits() {
    let Some(_root) = crate::tests::isolated_test_root(
        "audio_preview::tests::choices_are_owned_by_tab_and_source_and_do_not_modify_edits",
    ) else {
        return;
    };
    let mut app = app();
    let path = PathBuf::from("source.mp4");
    let first = app.tabs.open_new(path.clone(), MediaKind::Video);
    let second = app.tabs.open_new(path.clone(), MediaKind::Video);
    let selection = AudioTrackSelection::Track(AudioTrackId::from_index(2));
    app.audio_preview_choices.insert(
        first,
        Choice {
            path: path.clone(),
            selection,
        },
    );
    let edits = app.edits.clone();
    assert_eq!(app.audio_selection_for(Some(first), &path), selection);
    assert_eq!(
        app.audio_selection_for(Some(second), &path),
        AudioTrackSelection::Default
    );
    assert_eq!(
        app.audio_selection_for(Some(first), Path::new("replacement.mp4")),
        AudioTrackSelection::Default
    );
    assert_eq!(
        app.audio_selection_for(None, &path),
        AudioTrackSelection::Default
    );
    app.path = Some(path);
    app.displayed_tab = Some(first);
    app.media_kind = Some(MediaKind::Video);
    assert_eq!(
        app.waveform_audio_track(),
        Some(AudioTrackId::from_index(2))
    );
    app.media_kind = Some(MediaKind::Audio);
    assert_eq!(app.waveform_audio_track(), None);
    assert_eq!(app.edits, edits);
}

#[test]
fn track_labels_use_source_order_and_collapse_metadata_lines() {
    let track = AudioTrack {
        id: AudioTrackId::from_index(7),
        title: Some("Commentary\n  Main".into()),
        language: Some("eng".into()),
    };
    assert_eq!(
        track_label(localization::Language::English, 1, &track),
        "Track 2 · Commentary Main (eng)"
    );
    assert_eq!(
        track_label(localization::Language::Japanese, 1, &track),
        "トラック 2 · Commentary Main (eng)"
    );
    let track = AudioTrack {
        title: None,
        language: Some("und".into()),
        ..track
    };
    assert_eq!(
        track_label(localization::Language::English, 0, &track),
        "Track 1"
    );
}
