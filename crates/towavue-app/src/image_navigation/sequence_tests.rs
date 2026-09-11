use super::*;

type App = Application<fn(AppEvent)>;

fn fixture(root: &Path) -> (App, egui::Context, Vec<PathBuf>) {
    let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
    let context = fonts::test_context();
    let paths: Vec<_> = (0..100)
        .map(|index| root.join(format!("{index:03}.png")))
        .collect();
    let tab = app.tabs.open_new(paths[0].clone(), MediaKind::Image);
    app.path = Some(paths[0].clone());
    app.displayed_tab = Some(tab);
    app.media_kind = Some(MediaKind::Image);
    app.state = PlaybackState::Paused;
    app.ui_context = Some(context.clone());
    app.image = Some(
        ImagePresentation::from_decoded(
            &context,
            &paths[0],
            Arc::new(DecodedImage {
                format: "test",
                frames: vec![towavue_runtime_windows::DecodedImageFrame {
                    width: 1,
                    height: 1,
                    rgba: vec![0, 255, 0, 255],
                    delay: Duration::ZERO,
                }],
            }),
        )
        .expect("initial original"),
    );
    app.folder_snapshot = Some(FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![]),
        folder_path: root.to_owned(),
        items: paths
            .iter()
            .enumerate()
            .map(|(index, path)| towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(vec![index as u8]),
                path: path.clone(),
                kind: MediaKind::Image,
            })
            .collect(),
        sort_columns: vec![],
        source: FolderSnapshotSource::LiveExplorerView,
        generation: 1,
        captured_at: std::time::SystemTime::UNIX_EPOCH,
    });
    (app, context, paths)
}

fn complete(app: &mut App) {
    app.apply_loaded_images(towavue_runtime_windows::LoadedImages {
        generation: app.image_generation,
        first_index: 0,
        total: 1,
        images: vec![(
            app.path.clone().expect("target"),
            Ok(Arc::new(DecodedImage {
                format: "test",
                frames: vec![towavue_runtime_windows::DecodedImageFrame {
                    width: 1,
                    height: 1,
                    rgba: vec![255, 0, 0, 255],
                    delay: Duration::ZERO,
                }],
            })),
        )],
    });
}

fn draw(app: &mut App, context: &egui::Context) -> Option<u64> {
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 480.0),
            )),
            ..Default::default()
        },
        |ui| app.draw_ui(ui, &mut Vec::new()),
    );
    let token = app.image_sequence_token(&output);
    for shape in &mut output.shapes {
        shape.clip_rect = egui::Rect::NOTHING;
    }
    assert_eq!(
        app.image_sequence_token(&output),
        None,
        "clipped-out meshes are not presentations"
    );
    token
}

#[test]
fn next_image_burst_keeps_the_first_unpresented_original() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::sequence_tests::next_image_burst_keeps_the_first_unpresented_original",
    ) else {
        return;
    };
    let (mut app, context, paths) = fixture(&root);
    for _ in 0..100 {
        app.dispatch(CommandId::NextSameKind);
        // Do not depend on missing-file worker timing; completion is controlled by the test.
        app.image_loader.request(Vec::new());
    }
    assert_eq!(
        app.path.as_ref(),
        Some(&paths[1]),
        "do not supersede an unseen original"
    );
    assert_eq!(app.image_sequence.steps.len(), 99);
    assert_eq!(
        draw(&mut app, &context),
        None,
        "held image is not target presentation"
    );
    for index in 1..=100 {
        complete(&mut app);
        assert_eq!(app.path.as_ref(), Some(&paths[index % 100]));
        let token = draw(&mut app, &context).expect("new original mesh");
        app.finish_image_sequence_frame(Some(token + 1000));
        assert_eq!(
            app.path.as_ref(),
            Some(&paths[index % 100]),
            "stale ack is ignored"
        );
        assert_eq!(
            draw(&mut app, &context),
            Some(token),
            "drawing alone does not advance"
        );
        app.finish_image_sequence_frame(Some(token));
        app.image_loader.request(Vec::new());
        app.finish_image_sequence_frame(Some(token));
        assert_eq!(
            app.path.as_ref(),
            Some(&paths[if index == 100 { 0 } else { (index + 1) % 100 }]),
            "each acknowledged original advances exactly once"
        );
    }
    assert!(app.image_sequence.awaiting.is_none() && app.image_sequence.steps.is_empty());
}

#[test]
fn sequence_preserves_direction_order_and_has_a_notified_capacity() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::sequence_tests::sequence_preserves_direction_order_and_has_a_notified_capacity",
    ) else {
        return;
    };
    let (mut app, context, paths) = fixture(&root);
    for (index, forward) in [true, true, false, true, false, false]
        .into_iter()
        .enumerate()
    {
        app.navigate(forward, true);
        app.image_loader.request(Vec::new());
        if index == 0 {
            complete(&mut app);
        }
    }
    for index in [1, 2, 1, 2, 1, 0] {
        complete(&mut app);
        assert_eq!(app.path.as_ref(), Some(&paths[index]));
        let token = draw(&mut app, &context);
        app.finish_image_sequence_frame(token);
        app.image_loader.request(Vec::new());
    }
    app.navigate(true, true);
    for _ in 0..300 {
        app.navigate(true, true);
    }
    app.image_loader.request(Vec::new());
    assert_eq!(app.image_sequence.steps.len(), 256);
    assert_eq!(app.path.as_ref(), Some(&paths[1]));
    assert!(
        app.status_message
            .as_ref()
            .is_some_and(|(message, _)| message.contains("queue is full"))
    );
}

#[test]
fn sequence_is_cancelled_by_source_order_failure_departure_and_modal_changes() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::sequence_tests::sequence_is_cancelled_by_source_order_failure_departure_and_modal_changes",
    ) else {
        return;
    };
    for ending in 0..8 {
        let (mut app, context, paths) = fixture(&root);
        app.navigate(true, true);
        app.navigate(true, true);
        app.image_loader.request(Vec::new());
        let mut snapshot = app.folder_snapshot.clone().expect("snapshot");
        snapshot.generation += 1;
        snapshot.items.push(towavue_core::FolderMediaItem {
            identity: towavue_core::ShellIdentity::new(vec![200]),
            path: root.join("unrelated.mp4"),
            kind: MediaKind::Video,
        });
        app.apply_folder_snapshot(snapshot.clone());
        assert_eq!(
            app.image_sequence.steps.len(),
            1,
            "unchanged ordering keeps pending steps"
        );
        match ending {
            0 => {
                let stale = app.image_sequence.awaiting;
                app.jump_images(7);
                app.finish_image_sequence_frame(stale);
                assert_eq!(app.path.as_ref(), Some(&paths[8]));
                assert!(app.image_sequence.steps.is_empty());
                complete(&mut app);
                let token = draw(&mut app, &context);
                app.finish_image_sequence_frame(token);
            }
            1 => {
                app.load_path(paths[5].clone(), MediaKind::Image);
            }
            2 => {
                app.take_image_tab_state();
            }
            3 => {
                app.close_tab_unchecked(app.tabs.active().expect("tab").id);
            }
            4 => {
                snapshot.items.reverse();
                app.apply_folder_snapshot(snapshot);
            }
            5 => app.apply_loaded_images(towavue_runtime_windows::LoadedImages {
                generation: app.image_generation,
                first_index: 0,
                total: 1,
                images: vec![(
                    paths[1].clone(),
                    Err(towavue_runtime_windows::ImageDecodeError::UnknownFormat),
                )],
            }),
            _ => {
                complete(&mut app);
                let token = draw(&mut app, &context);
                if ending == 6 {
                    app.palette_open = true;
                } else {
                    app.push_edit(EditOperation::RotateClockwise);
                }
                app.finish_image_sequence_frame(token);
                assert_eq!(app.path.as_ref(), Some(&paths[1]));
            }
        }
        app.image_loader.request(Vec::new());
        assert!(
            app.image_sequence.awaiting.is_none() && app.image_sequence.steps.is_empty(),
            "ending {ending}"
        );
    }
}

#[test]
fn approved_guard_keeps_the_first_unpresented_target() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::sequence_tests::approved_guard_keeps_the_first_unpresented_target",
    ) else {
        return;
    };
    let (mut app, context, paths) = fixture(&root);
    app.push_edit(EditOperation::RotateClockwise);
    app.navigate(true, true);
    assert!(app.pending_guard.is_some());
    assert_eq!(app.path.as_ref(), Some(&paths[0]));
    app.resolve_guard(GuardDecision::Discard);
    app.image_loader.request(Vec::new());
    for _ in 0..4 {
        app.navigate(true, true);
    }
    assert_eq!(app.path.as_ref(), Some(&paths[1]));
    assert_eq!(app.image_sequence.steps.len(), 4);
    for path in &paths[1..=5] {
        complete(&mut app);
        assert_eq!(app.path.as_ref(), Some(path));
        let token = draw(&mut app, &context);
        app.finish_image_sequence_frame(token);
        app.image_loader.request(Vec::new());
    }
    assert!(app.image_sequence.awaiting.is_none());
}

#[test]
fn native_render_frame_advances_the_sequence_only_after_the_new_original() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::sequence_tests::native_render_frame_advances_the_sequence_only_after_the_new_original",
    ) else {
        return;
    };
    struct Trial {
        root: PathBuf,
    }
    use winit::platform::windows::EventLoopBuilderExtWindows;
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = Arc::new(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_visible(false)
                            .with_inner_size(LogicalSize::new(640, 480)),
                    )
                    .expect("owned window"),
            );
            let mut renderer = match FrameRenderer::new(&window) {
                Ok(renderer) => renderer,
                Err(error) => {
                    eprintln!("SKIP native image sequence: hardware D3D11 unavailable: {error}");
                    event_loop.exit();
                    return;
                }
            };
            let size = window.inner_size();
            renderer
                .resize_surface(size.width, size.height)
                .expect("surface");
            let (mut app, context, paths) = fixture(&self.root);
            app.ui_state = Some(egui_winit::State::new(
                context,
                egui::ViewportId::ROOT,
                &window,
                Some(window.scale_factor() as f32),
                None,
                Some(renderer.max_texture_side()),
            ));
            app.window = Some(window);
            app.renderer = Some(renderer);
            app.fullscreen = true;
            for _ in 0..3 {
                app.render_frame();
            }
            for _ in 0..100 {
                app.dispatch(CommandId::NextSameKind);
            }
            app.image_loader.request(Vec::new());
            app.render_frame();
            assert_eq!(
                app.path.as_ref(),
                Some(&paths[1]),
                "held frame cannot advance"
            );
            for index in 1..=100 {
                assert_eq!(app.path.as_ref(), Some(&paths[index % 100]));
                complete(&mut app);
                app.render_frame();
                app.image_loader.request(Vec::new());
                assert_eq!(
                    app.path.as_ref(),
                    Some(&paths[if index == 100 { 0 } else { (index + 1) % 100 }]),
                    "the production render_frame must acknowledge and advance exactly once"
                );
            }
            assert!(app.image_sequence.awaiting.is_none() && app.image_sequence.steps.is_empty());
            assert!(app.playback_error.is_none());
            eprintln!(
                "PASS native image sequence: 100 accepted steps, render_frame GPU/Present acknowledgement, held frames excluded; generated tiny originals and hidden window, not physical keys"
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut Trial { root })
        .expect("native sequence");
}
