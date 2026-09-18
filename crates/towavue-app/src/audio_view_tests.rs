use crate::audio_export::tests::frame;
use crate::*;
use std::os::windows::process::CommandExt;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
fn audio_viewing_seek_and_editing_modes_preserve_live_state_without_hover_cards() {
    let Some(root) = tests::isolated_test_root(
        "audio_view_tests::audio_viewing_seek_and_editing_modes_preserve_live_state_without_hover_cards",
    ) else {
        return;
    };
    let source = root.join("silence.wav");
    assert!(
        std::process::Command::new(
            PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe")
        )
        .creation_flags(0x0800_0000)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=48000:cl=stereo",
            "-t",
            "8",
            "-c:a",
            "pcm_s16le"
        ])
        .arg(&source)
        .status()
        .expect("silent fixture")
        .success()
    );
    struct Trial(PathBuf);
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("hidden window");
            let renderer = FrameRenderer::new(&window).expect("D3D11");
            let (sender, events) = std::sync::mpsc::channel();
            let mut app = Application::new(None, move |event| {
                let _ = sender.send(event);
            })
            .expect("app");
            app.renderer = Some(renderer);
            app.open_external(self.0.clone(), true);
            assert_eq!(app.state, PlaybackState::Playing, "real audio session");
            app.dispatch(CommandId::TogglePause);
            let deadline = Instant::now() + Duration::from_secs(8);
            while app.media_duration.is_none() {
                app.handle_app_event(
                    events
                        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                        .expect("duration"),
                );
            }
            assert!(!app.timeline_open && !app.waveform_loading);
            let tab = app.tabs.active_id().expect("audio tab");
            let media_generation = app.media_generation;
            let history = app.edits[&tab].clone();
            for density in [1.0, 1.25, 2.0] {
                let context = fonts::test_context();
                context.enable_accesskit();
                context.set_pixels_per_point(density);
                app.ui_context = Some(context);
                let size = egui::vec2(960.0, 400.0);
                app.seek_to(media_time(Duration::from_millis(1234)));
                for _ in 0..4 {
                    frame(&mut app, size, vec![]);
                }
                let output = frame(&mut app, size, vec![]);
                let (id, bounds) = slider(&output);
                assert!(bounds.height() <= 14.1, "compact audio seek");
                assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "00:01 / 00:08")));
                let point = egui::pos2(
                    ((bounds.x0 + bounds.x1) / 2.0) as f32,
                    ((bounds.y0 + bounds.y1) / 2.0) as f32,
                );
                for _ in 0..45 {
                    frame(&mut app, size, vec![egui::Event::PointerMoved(point)]);
                }
                assert!(app.hover_thumbnail.is_none() && app.thumbnail_loading.is_none());
                assert!(!app.video_scrub_seen && app.video_scrub.is_none());
                frame(
                    &mut app,
                    size,
                    vec![egui::Event::AccessKitActionRequest(
                        egui::accesskit::ActionRequest {
                            action: egui::accesskit::Action::SetValue,
                            target_tree: egui::accesskit::TreeId::ROOT,
                            target_node: id,
                            data: Some(egui::accesskit::ActionData::NumericValue(3.25)),
                        },
                    )],
                );
                assert!((app.current_position().as_seconds_f64() - 3.25).abs() < 0.001);
                frame(&mut app, size, vec![pointer(point, true)]);
                let output = frame(&mut app, size, vec![pointer(point, false)]);
                assert!((app.current_position().as_seconds_f64() - 4.0).abs() < 0.03);
                assert!(!app.timeline_open);
                assert_eq!(slider(&output).1.height(), bounds.height());
                // The same vertical lift used by video enters editing without a seek.
                let before = app.current_position();
                frame(&mut app, size, vec![pointer(point, true)]);
                frame(
                    &mut app,
                    size,
                    vec![egui::Event::PointerMoved(point - egui::vec2(0.0, 40.0))],
                );
                frame(
                    &mut app,
                    size,
                    vec![pointer(point - egui::vec2(0.0, 40.0), false)],
                );
                assert!(app.timeline_open && app.timeline_is_visible());
                assert_eq!(app.current_position(), before);
                for _ in 0..4 {
                    frame(&mut app, size, vec![]);
                }
                let output = frame(&mut app, size, vec![]);
                assert!(slider(&output).1.height() > 30.0);
                assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().starts_with("00:00:04:") && text.galley.text().ends_with(" / 00:00:08:000"))));
                app.dispatch(CommandId::ToggleTimeline);
                assert!(!app.timeline_is_visible());
                app.set_fullscreen(true);
                frame(&mut app, size, vec![egui::Event::PointerGone]);
                assert!(!app.fullscreen_controls_visible && !app.timeline_is_visible());
                for _ in 0..4 {
                    frame(
                        &mut app,
                        size,
                        vec![egui::Event::PointerMoved(egui::pos2(500.0, 399.0))],
                    );
                }
                let output = frame(&mut app, size, vec![]);
                assert!(slider(&output).1.height() <= 14.1);
                app.dispatch(CommandId::ToggleTimeline);
                assert!(!app.fullscreen && app.timeline_is_visible());
                app.dispatch(CommandId::ToggleTimeline);
                assert_eq!(app.media_generation, media_generation);
                assert_eq!(app.edits[&tab], history);
                assert_eq!(app.state, PlaybackState::Paused);
            }
            for mode in [false, true] {
                app.timeline_open = mode;
                app.open_external(self.0.clone(), true);
                let other = app.tabs.active_id().expect("new audio tab");
                assert_ne!(other, tab);
                assert!(!app.timeline_open, "fresh audio uses viewing mode");
                app.dispatch(CommandId::TogglePause);
                app.activate_tab(tab);
                assert_eq!(app.timeline_open, mode, "retain each audio tab's mode");
                app.remove_tab(other, false);
            }
            drop(app);
            drop(window);
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("event loop")
        .run_app(&mut Trial(source))
        .expect("audio mode trial");
}

fn slider(output: &egui::FullOutput) -> (egui::accesskit::NodeId, egui::accesskit::Rect) {
    let nodes: Vec<_> = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree")
        .nodes
        .iter()
        .filter(|(_, node)| node.label() == Some("Playback position (seconds)"))
        .collect();
    assert_eq!(nodes.len(), 1, "one playback control");
    (nodes[0].0, nodes[0].1.bounds().expect("slider bounds"))
}

fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}
