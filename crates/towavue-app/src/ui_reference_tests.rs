//! Whole-client readbacks for reviewing the concept's media layouts.
//! Media and Shell state are generated; this does not exercise native input or captions.

use crate::*;
use std::os::windows::process::CommandExt;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "requires hidden hardware D3D11; writes generated full-client UI readbacks"]
fn media_reference_layouts_reach_the_gpu() {
    let Some(root) =
        tests::isolated_test_root("ui_reference_tests::media_reference_layouts_reach_the_gpu")
    else {
        return;
    };
    // PCM silence has the same declared duration as the seeded paused UI clock.
    // It is opened muted and paused; no user media or audible output is involved.
    let samples = 8_000_u32 * 161;
    let mut wav = Vec::with_capacity(44 + samples as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + samples).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
    wav.extend_from_slice(&8_000_u32.to_le_bytes());
    wav.extend_from_slice(&8_000_u32.to_le_bytes());
    wav.extend_from_slice(b"\x01\0\x08\0data");
    wav.extend_from_slice(&samples.to_le_bytes());
    wav.resize(44 + samples as usize, 128);
    std::fs::write(root.join("Track 2.wav"), wav).expect("generated silent audio");
    assert!(
        std::process::Command::new(
            PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg fixture tools"))
                .join("bin/ffmpeg.exe"),
        )
        .creation_flags(0x0800_0000)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=0x2060a0:s=320x180:r=1:d=161",
            "-c:v",
            "mpeg4",
            "-pix_fmt",
            "yuv420p",
            "-an"
        ])
        .arg(root.join("Clip 2.mp4"))
        .status()
        .expect("generate video")
        .success()
    );
    struct Trial {
        root: PathBuf,
        complete: bool,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("hidden reference window");
            let mut renderer = FrameRenderer::new(&window).expect("hardware D3D11");
            let output = std::env::var_os("TOWAVUE_REFERENCE_RENDER_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| self.root.clone());
            std::fs::create_dir_all(&output).expect("reference output directory");
            for density in [1.0, 1.25, 2.0] {
                for scene in [
                    "image",
                    "image-reading",
                    "image-languages",
                    "image-export",
                    "image-export-wait",
                    "audio",
                    "audio-timeline",
                    "video",
                    "video-timeline",
                ] {
                    let context = fonts::test_context();
                    context.global_style_mut(chrome::style);
                    if matches!(scene, "image-reading" | "image-languages") {
                        fonts::install(&context);
                    }
                    let mut app = fixture(&self.root, &context, scene);
                    if scene == "image-reading" {
                        // Seed only the model; no native cursor capture or physical input.
                        app.reading_drag = Some(reading_input::ReadingDrag::new(
                            app.reading_settings,
                            true,
                            f64::from(density),
                        ));
                    }
                    if scene.starts_with("image-export") {
                        let path = app.path.clone().expect("export source");
                        let tab = app.tabs.active().expect("export tab").id;
                        let request = ExportRequest {
                            source: path.clone(),
                            target: path,
                            kind: MediaKind::Image,
                            operations: vec![],
                            hardware_encode: false,
                        };
                        let options = ExportOptions::default();
                        app.active_export = Some(ActiveExport {
                            progress: export_progress::ExportProgress::new(
                                &request,
                                &options,
                                Some(Duration::from_secs(10)),
                            ),
                            // Same-source validation rejects the fixture without file mutation.
                            job: ExportJob::start(request.clone(), |_| {})
                                .expect("export fixture")
                                .into(),
                            tab,
                            request,
                            options,
                            encoded: Duration::from_secs(4),
                            analyzing_audio: false,
                            cancelling: false,
                            continuation: (scene == "image-export-wait")
                                .then_some(GuardedAction::CloseTab(tab)),
                        });
                    }
                    if !scene.starts_with("image") {
                        app.session = Some(
                            PlaybackSession::open_paused(
                                app.path.as_deref().expect("media path"),
                                renderer.graphics_device(),
                                0.0,
                                1.0,
                                Default::default(),
                                |_| {},
                            )
                            .expect("generated paused native session"),
                        );
                        if scene.starts_with("video") {
                            let session = app.session.as_mut().expect("video session");
                            let deadline = Instant::now() + Duration::from_secs(10);
                            while session.pending_video_time().is_none() {
                                assert!(Instant::now() < deadline, "decoded video deadline");
                                std::thread::sleep(Duration::from_millis(1));
                            }
                            assert!(session.advance_pending());
                        }
                    }
                    let width = (1228.0 * density) as u32;
                    let height = (708.0 * density) as u32;
                    renderer
                        .resize_surface(width, height)
                        .expect("surface size");
                    let mut pixels = Vec::new();
                    for frame in 0..4 {
                        let mut input = egui::RawInput {
                            time: Some(frame as f64 * 0.25),
                            max_texture_side: Some(renderer.max_texture_side()),
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(1228.0, 708.0),
                            )),
                            ..Default::default()
                        };
                        input
                            .viewports
                            .entry(egui::ViewportId::ROOT)
                            .or_default()
                            .native_pixels_per_point = Some(density);
                        let mut actions = Vec::new();
                        let ui = context.run_ui(input, |ui| app.draw_ui(ui, &mut actions));
                        assert!(actions.is_empty(), "passive reference rendering");
                        if frame >= 2 && scene.starts_with("image-export") {
                            let text = ui
                                .shapes
                                .iter()
                                .find_map(|shape| match &shape.shape {
                                    egui::Shape::Text(text)
                                        if text.galley.text().starts_with("Exporting") =>
                                    {
                                        Some(text)
                                    }
                                    _ => None,
                                })
                                .expect("export status text");
                            let bounds = text.galley.rect.translate(text.pos.to_vec2());
                            assert!(
                                bounds.top() >= 678.0 && bounds.bottom() <= 708.0,
                                "export text fits the GPU status viewport: {scene}, {density}: {bounds:?}"
                            );
                        }
                        if !scene.starts_with("image") {
                            // Populate only requested visible durations through the production
                            // delivery interface, independently of asynchronous disk inspection.
                            while let Some(request) = app.playlist.duration_request() {
                                app.playlist
                                    .finish_duration(request, Some(Duration::from_secs(161)));
                            }
                        }
                        renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
                        if scene.starts_with("video") {
                            let rect = app.video_rect.expect("video layout");
                            assert!(
                                app.session
                                    .as_mut()
                                    .expect("video session")
                                    .draw_current(&mut renderer, rect * density, app.video_uv)
                                    .expect("native video before UI")
                            );
                        }
                        renderer.render_ui(&context, ui).expect("UI submission");
                        pixels = renderer
                            .verification_surface_rgba()
                            .expect("readback before Present");
                        renderer.present_surface().expect("Present");
                    }
                    assert_eq!(pixels.len(), (width * height * 4) as usize);
                    let at = |x: f32, y: f32| {
                        let offset = (((y * density) as usize * width as usize)
                            + (x * density) as usize)
                            * 4;
                        &pixels[offset..offset + 4]
                    };
                    if scene.starts_with("image") {
                        assert_eq!(
                            at(614.0, 354.0),
                            [32, 96, 160, 255],
                            "full image reaches media center"
                        );
                        assert_ne!(
                            at(614.0, 10.0),
                            [32, 96, 160, 255],
                            "image cannot cover toolbar"
                        );
                        assert_ne!(
                            at(614.0, 698.0),
                            [32, 96, 160, 255],
                            "image cannot cover status"
                        );
                    } else if scene.starts_with("video") {
                        let center = at(614.0, 354.0);
                        assert!(
                            center[2] > center[0].saturating_add(60),
                            "decoded blue video survives UI composition: {scene}, {density}: {center:?}"
                        );
                        assert_eq!(app.timeline_open, scene == "video-timeline");
                        let rect = app.video_rect.expect("video layout");
                        assert!(
                            rect.top() >= 32.0 && rect.bottom() <= 679.0,
                            "video fits between chrome: {scene}: {rect:?}"
                        );
                        assert!(
                            at(614.0, 10.0)[2] < 100 && at(614.0, 698.0)[2] < 100,
                            "video must not cover toolbar/status"
                        );
                        if scene == "video" {
                            assert_eq!(
                                at(50.0, 678.0),
                                [255, 255, 255, 255],
                                "video seek progress"
                            );
                        } else {
                            assert!(rect.bottom() <= 590.0, "editing reserves timeline height");
                        }
                    } else {
                        let list = app.playlist.scroll_rect.expect("audio list viewport");
                        assert!(list.top() >= 32.0 && list.bottom() < 708.0);
                        assert!(
                            list.height() > 200.0,
                            "audio list keeps usable space: {scene}: {list:?}"
                        );
                        let row_pixels = (40..200)
                            .flat_map(|y| (8..300).map(move |x| (x, y)))
                            .filter(|(x, y)| at(*x as f32, *y as f32)[0] > 100)
                            .count();
                        assert!(
                            row_pixels > 100,
                            "audio rows must remain visible above the timeline: {scene}, {density}: {list:?}, {row_pixels} bright pixels"
                        );
                        assert_eq!(app.timeline_open, scene == "audio-timeline");
                        if scene == "audio" {
                            assert_eq!(
                                at(50.0, 678.0),
                                [255, 255, 255, 255],
                                "paused audio position reaches the compact seek bar"
                            );
                            assert_ne!(
                                at(700.0, 678.0),
                                [255, 255, 255, 255],
                                "remaining audio is not painted as played"
                            );
                        }
                        assert!(
                            pixels
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .filter(|p| p[0] > 100)
                                .count()
                                > 1000,
                            "list and transport text reach the GPU"
                        );
                    }
                    std::fs::write(
                        output.join(format!("{scene}-{width}x{height}.rgba")),
                        &pixels,
                    )
                    .expect("write generated readback");
                }
            }
            self.complete = true;
            eprintln!(
                "PASS reference layouts: image/languages/reading/export status, compact/editing audio and compact/editing video at 100/125/200%; twenty-seven full-client GPU readbacks. Generated state with native paused decoding; no native-caption or physical-input evidence."
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut trial = Trial {
        root,
        complete: false,
    };
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut trial)
        .expect("reference trial");
    assert!(trial.complete, "GPU reference must not silently skip");
}

fn fixture(root: &Path, context: &egui::Context, scene: &str) -> Application<fn(AppEvent)> {
    let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("reference app");
    let kind = if scene.starts_with("image") {
        MediaKind::Image
    } else if scene.starts_with("video") {
        MediaKind::Video
    } else {
        MediaKind::Audio
    };
    let names = if scene == "image-languages" {
        [
            "日本語.png",
            "한국어.png",
            "العربية.png",
            "हिन्दी.png",
            "ภาษาไทย.png",
        ]
    } else if kind == MediaKind::Image {
        [
            "Landscape.png",
            "Study.png",
            "Portrait.png",
            "Texture.png",
            "Diagram.png",
        ]
    } else if kind == MediaKind::Video {
        [
            "Clip 1.mp4",
            "Clip 2.mp4",
            "Clip 3.mp4",
            "Clip 4.mp4",
            "Clip 5.mp4",
        ]
    } else {
        [
            "Track 1.wav",
            "Track 2.wav",
            "Track 3.wav",
            "Track 4.wav",
            "Track 5.wav",
        ]
    };
    let path = root.join(names[1]);
    let tab = app.tabs.open_new(path.clone(), kind);
    app.path = Some(path.clone());
    app.displayed_tab = Some(tab);
    app.media_kind = Some(kind);
    app.state = PlaybackState::Paused;
    app.ui_context = Some(context.clone());
    app.folder_snapshot = Some(FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![0]),
        folder_path: root.to_path_buf(),
        items: names
            .iter()
            .enumerate()
            .map(|(index, name)| towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(vec![index as u8]),
                path: root.join(name),
                kind,
            })
            .collect(),
        sort_columns: vec![],
        source: FolderSnapshotSource::NaturalNameFallback,
        generation: 1,
        captured_at: std::time::SystemTime::UNIX_EPOCH,
    });
    if kind == MediaKind::Image {
        app.image = Some(
            ImagePresentation::from_decoded(
                context,
                &path,
                Arc::new(DecodedImage {
                    animation_plays: 0,
                    format: "png",
                    frames: vec![towavue_runtime_windows::DecodedImageFrame {
                        width: 960,
                        height: 640,
                        rgba: [32, 96, 160, 255].repeat(960 * 640),
                        delay: Duration::ZERO,
                    }],
                }),
            )
            .expect("generated image"),
        );
        for name in &names[2..] {
            app.tabs.open_new(root.join(name), kind);
        }
        assert!(app.tabs.activate(tab));
    } else {
        app.media_duration = Some(Duration::from_secs(161));
        app.clock = Some(PlaybackClock::paused(
            media_time(Duration::from_secs(50)),
            1.0,
        ));
        app.timeline_open = scene.ends_with("-timeline");
        if app.timeline_open {
            let mut waveform = egui::ColorImage::filled([512, 64], Color32::TRANSPARENT);
            for x in 0..512 {
                let amplitude = 4 + (x * 17 % 25);
                for y in (32 - amplitude)..(32 + amplitude) {
                    waveform[(x, y)] = Color32::WHITE;
                }
            }
            app.waveform =
                Some(context.load_texture("generated waveform", waveform, TextureOptions::LINEAR));
        }
    }
    app
}
