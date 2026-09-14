use super::*;

type App = Application<fn(AppEvent)>;

fn fixture(root: &Path) -> (App, egui::Context, Vec<PathBuf>) {
    fixture_with_notify(root, (|_| {}) as fn(AppEvent))
}

fn fixture_with_notify<N: Fn(AppEvent) + Send + Sync + 'static>(
    root: &Path,
    notify: N,
) -> (Application<N>, egui::Context, Vec<PathBuf>) {
    let mut app = Application::new(None, notify).expect("app");
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
                animation_plays: 0,
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
                animation_plays: 0,
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
fn queued_destinations_precede_speculation_without_expanding_neighbors() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::sequence_tests::queued_destinations_precede_speculation_without_expanding_neighbors",
    ) else {
        return;
    };
    let (mut app, _, paths) = fixture(&root);
    app.verification_directional_prefetch = false;
    // The fixture's order is the supplied Shell snapshot, not a filename sort.
    app.folder_snapshot
        .as_mut()
        .expect("snapshot")
        .items
        .reverse();
    let paths: Vec<_> = paths.into_iter().rev().collect();
    app.path = Some(paths[0].clone());
    for (forward, steps, expected) in [
        (true, vec![], vec![1, 99, 2, 98, 3, 97, 4, 96, 5]),
        (true, vec![true; 2], vec![1, 2, 99, 98, 3, 97, 4, 96, 5]),
        (true, vec![true; 10], vec![1, 2, 3, 4, 5, 99, 98, 97, 96]),
        (false, vec![false; 3], vec![99, 98, 97, 1, 2, 3, 96, 4, 95]),
        (true, vec![false; 2], vec![99, 98, 1, 2, 3, 97, 4, 96, 5]),
        (
            true,
            vec![false, true, true, false, false, false],
            vec![99, 1, 98, 2, 3, 97, 4, 96, 5],
        ),
    ] {
        app.image_navigation_forward = forward;
        app.image_sequence.steps = steps.iter().copied().collect();
        assert_eq!(
            app.image_prefetch_paths(),
            Some(
                expected
                    .into_iter()
                    .map(|index| paths[index].clone())
                    .collect()
            ),
            "forward={forward}, steps={steps:?}"
        );
        assert_eq!(
            app.image_sequence.steps.iter().copied().collect::<Vec<_>>(),
            steps
        );
    }
    app.image_sequence.steps.clear();
    assert_eq!(
        app.image_prefetch_paths(),
        Some(
            [1, 99, 2, 98, 3, 97, 4, 96, 5]
                .map(|index| paths[index].clone())
                .to_vec()
        ),
        "clearing the queue restores balanced speculation"
    );
    app.reading_mode = true;
    let reading_plan = app.image_prefetch_paths();
    app.image_sequence.steps.extend([false; 256]);
    assert_eq!(app.image_prefetch_paths(), reading_plan);
}

#[test]
fn directional_prefetch_control_preserves_bounds_queue_priority_and_reading() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::sequence_tests::directional_prefetch_control_preserves_bounds_queue_priority_and_reading",
    ) else {
        return;
    };
    let (mut app, _, paths) = fixture(&root);
    assert!(app.verification_directional_prefetch);
    for (forward, expected) in [
        (true, vec![1, 2, 3, 4, 5, 6, 7, 8, 99]),
        (false, vec![99, 98, 97, 96, 95, 94, 93, 92, 1]),
    ] {
        app.image_navigation_forward = forward;
        assert_eq!(
            app.image_prefetch_paths(),
            Some(expected.into_iter().map(|i| paths[i].clone()).collect())
        );
    }
    app.image_navigation_forward = true;
    app.image_sequence.steps.extend([false, true, true]);
    assert_eq!(
        app.image_prefetch_paths(),
        Some(
            [99, 1, 2, 3, 4, 5, 6, 7, 8]
                .map(|i| paths[i].clone())
                .to_vec()
        )
    );
    app.image_sequence.steps.clear();
    app.reading_mode = true;
    let spread = app.image_prefetch_paths();
    app.verification_directional_prefetch = false;
    assert_eq!(app.image_prefetch_paths(), spread);
    app.reading_mode = false;
    app.verification_directional_prefetch = true;
    for size in [3, 2, 1] {
        app.folder_snapshot
            .as_mut()
            .expect("snapshot")
            .items
            .truncate(size);
        assert_eq!(
            app.image_prefetch_paths(),
            (size > 1).then(|| paths[1..size].to_vec())
        );
    }
}

#[test]
fn image_navigation_retargets_pending_folder_refresh_without_restarting_it() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::sequence_tests::image_navigation_retargets_pending_folder_refresh_without_restarting_it",
    ) else {
        return;
    };
    let (mut app, context, paths) = fixture(&root);
    // Model a pending completion without depending on native enumeration timing.
    let generation = app.folder_order.request(None);
    app.pending_folder = Some((generation, FolderIntent::Refresh(paths[0].clone())));
    for (key, index) in [("Right", 1), ("Right", 2), ("Left", 1), ("Right", 2)] {
        app.process_shortcut(key.parse().expect("navigation key"));
        app.image_loader.request(Vec::new());
        assert_eq!(app.path.as_ref(), Some(&paths[index]));
        assert!(
            matches!(&app.pending_folder,
            Some((current, FolderIntent::Refresh(path))) if *current == generation && path == &paths[index]),
            "same-folder navigation reuses the request and retargets its completion"
        );
        complete(&mut app);
        let token = draw(&mut app, &context);
        app.finish_image_sequence_frame(token);
    }
    app.refresh_folder_snapshot();
    assert!(
        matches!(&app.pending_folder,
        Some((current, FolderIntent::Refresh(path))) if *current != generation && path == &paths[2]),
        "explicit/watcher refresh still invalidates earlier work"
    );
    for destination in [
        root.join("new.png"),
        root.join("elsewhere").join("next.png"),
    ] {
        let previous = app.pending_folder.as_ref().expect("pending refresh").0;
        app.navigate_to_unchecked(destination.clone());
        app.image_loader.request(Vec::new());
        assert!(
            matches!(&app.pending_folder,
            Some((current, FolderIntent::Refresh(path))) if *current != previous && path == &destination),
            "unknown items and different folders require a fresh request"
        );
    }
    assert!(
        app.folder_snapshot.is_none(),
        "different folder loses old order"
    );
    app.folder_order.request(None);
}

#[test]
fn initial_image_burst_waits_for_original_presentation_and_folder_order() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::sequence_tests::initial_image_burst_waits_for_original_presentation_and_folder_order",
    ) else {
        return;
    };
    for folder_ready in [true, false] {
        let (mut app, context, paths) = fixture(&root);
        let snapshot = app.folder_snapshot.take().expect("order");
        if folder_ready {
            app.folder_snapshot = Some(snapshot.clone());
        }
        app.displayed_tab = None;
        app.image = None;
        app.load_path(paths[0].clone(), MediaKind::Image);
        // Control both asynchronous completions without decoding missing fixture paths.
        app.image_loader.request(Vec::new());
        app.folder_order.request(None);
        let generation = app.image_generation;
        for forward in [true, true, false, true] {
            app.dispatch(if forward {
                CommandId::NextSameKind
            } else {
                CommandId::PreviousSameKind
            });
            app.image_loader.request(Vec::new());
        }
        assert_eq!(
            app.path.as_ref(),
            Some(&paths[0]),
            "first original must not be skipped"
        );
        assert_eq!(
            app.image_generation, generation,
            "burst must not restart initial loading"
        );
        assert_eq!(
            app.image_sequence.steps.iter().copied().collect::<Vec<_>>(),
            [true, true, false, true]
        );
        assert_eq!(draw(&mut app, &context), None);
        let order = [0, 1, 2, 1, 2];
        for (step, index) in order.into_iter().enumerate() {
            complete(&mut app);
            assert_eq!(app.path.as_ref(), Some(&paths[index]));
            let token = draw(&mut app, &context).expect("original is drawn");
            if step == 0 && !folder_ready {
                app.finish_image_sequence_frame(Some(token));
                assert_eq!(app.path.as_ref(), Some(&paths[0]));
                assert_eq!(
                    app.image_sequence.steps.len(),
                    4,
                    "late order must not discard input"
                );
                app.pending_folder = None;
                app.apply_folder_snapshot(snapshot.clone());
            }
            app.finish_image_sequence_frame(Some(token));
            app.image_loader.request(Vec::new());
            app.folder_order.request(None);
            app.finish_image_sequence_frame(Some(token));
            assert_eq!(app.path.as_ref(), Some(&paths[order[(step + 1).min(4)]]));
        }
        assert!(app.image_sequence.awaiting.is_none() && app.image_sequence.steps.is_empty());
    }
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
                assert!(
                    app.image_sequence.steps.is_empty(),
                    "direct open drops old directions"
                );
                complete(&mut app);
                let token = draw(&mut app, &context);
                app.finish_image_sequence_frame(token);
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
    run_native_sequence(
        "image_navigation::sequence_tests::native_render_frame_advances_the_sequence_only_after_the_new_original",
        false,
        false,
    );
}

#[test]
#[ignore = "generates 100 large JPEGs; requires FFMPEG_DIR and hardware D3D11; correctness, not a latency benchmark"]
fn native_initial_large_images_preserve_every_accepted_step() {
    run_native_sequence(
        "image_navigation::sequence_tests::native_initial_large_images_preserve_every_accepted_step",
        true,
        false,
    );
}

#[test]
fn native_folder_notifications_preserve_initial_navigation_burst() {
    run_native_sequence(
        "image_navigation::sequence_tests::native_folder_notifications_preserve_initial_navigation_burst",
        false,
        true,
    );
}

#[test]
#[ignore = "generates 100 large JPEGs; requires FFMPEG_DIR, Shell and hardware D3D11; correctness, not latency"]
fn native_folder_notifications_preserve_initial_large_image_burst() {
    run_native_sequence(
        "image_navigation::sequence_tests::native_folder_notifications_preserve_initial_large_image_burst",
        true,
        true,
    );
}

fn run_native_sequence(test_name: &str, large: bool, native_order: bool) {
    let Some(root) = crate::tests::isolated_test_root(test_name) else {
        return;
    };
    struct Trial {
        root: PathBuf,
        large: bool,
        native_order: bool,
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
            let (notify, events) = std::sync::mpsc::channel();
            let (mut app, context, mut paths) = fixture_with_notify(&self.root, move |event| {
                let _ = notify.send(event);
            });
            if self.large {
                paths = super::performance_tests::large_jpeg_fixture(&self.root);
            } else {
                for path in &mut paths {
                    path.set_extension("bmp");
                    super::performance_tests::bitmap_fixture(path);
                }
            }
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
            let mut snapshot = app.folder_snapshot.take().expect("order");
            for (item, path) in snapshot.items.iter_mut().zip(&paths) {
                item.path = path.clone();
            }
            app.tabs
                .active_mut()
                .expect("tab")
                .target
                .set_current_path(paths[0].clone(), MediaKind::Image);
            app.displayed_tab = None;
            app.image = None;
            app.load_path(paths[0].clone(), MediaKind::Image);
            if self.native_order {
                for _ in 0..100 {
                    app.process_shortcut("Right".parse().expect("navigation key"));
                }
                assert_eq!(app.image_sequence.steps.len(), 100);
                let mut ordered = Vec::new();
                let mut presented = 0;
                let mut completions = 0;
                let mut order_source = None;
                let deadline = Instant::now() + Duration::from_secs(60);
                while presented <= 100 {
                    assert!(
                        Instant::now() < deadline,
                        "native sequence timeout at {presented}"
                    );
                    while let Ok(event) = events.try_recv() {
                        let previous = app
                            .folder_snapshot
                            .as_ref()
                            .map(|snapshot| snapshot.generation);
                        app.handle_app_event(event);
                        if let Some(snapshot) = &app.folder_snapshot
                            && Some(snapshot.generation) != previous
                        {
                            completions += 1;
                            let mut current: Vec<_> = snapshot
                                .items_of_kind(MediaKind::Image)
                                .map(|item| item.path.clone())
                                .collect();
                            assert_eq!(current.len(), paths.len());
                            assert!(paths.iter().all(|path| current.contains(path)));
                            let first = current
                                .iter()
                                .position(|path| path == &paths[0])
                                .expect("initial source");
                            current.rotate_left(first);
                            if ordered.is_empty() {
                                ordered = current;
                                order_source = Some(snapshot.source);
                            } else {
                                assert_eq!(
                                    current, ordered,
                                    "unchanged fixture retains Shell order"
                                );
                            }
                        }
                    }
                    let path = app.path.clone().expect("current source");
                    let generation = app.media_generation;
                    assert_eq!(app.image_sequence.awaiting, Some(generation));
                    app.render_frame();
                    assert!(app.image_error.is_none());
                    if app.image_sequence.awaiting != Some(generation) {
                        assert!(!ordered.is_empty(), "order must arrive before advancement");
                        assert_eq!(path, ordered[presented % ordered.len()]);
                        presented += 1;
                        assert_eq!(
                            app.image_sequence.steps.len(),
                            100_usize.saturating_sub(presented)
                        );
                    } else {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
                assert!(completions > 0);
                assert!(
                    app.image_sequence.steps.is_empty() && app.image_sequence.awaiting.is_none()
                );
                assert!(app.playback_error.is_none());
                eprintln!(
                    "PASS native folder navigation: 101 original presentations, 100 initial shortcut commands, {completions} real folder completions, source={order_source:?}, large={}; real decode/prefetch and app-event handlers, hidden GPU/Present, no physical input or latency claim",
                    self.large
                );
                event_loop.exit();
                return;
            }
            // Delay only Shell order; original decoding and prefetch use real workers.
            app.folder_order.request(None);
            for _ in 0..100 {
                app.dispatch(CommandId::NextSameKind);
            }
            app.render_frame();
            assert_eq!(
                app.path.as_ref(),
                Some(&paths[0]),
                "initial load cannot be superseded"
            );
            assert_eq!(app.image_sequence.steps.len(), 100);
            let deadline = Instant::now() + Duration::from_secs(10);
            while app.image_loading {
                assert!(Instant::now() < deadline, "initial original decode timeout");
                app.render_frame();
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(app.image_error.is_none());
            assert_eq!(
                app.image.as_ref().expect("decoded original").dimensions(),
                if self.large { (4096, 2304) } else { (2, 1) }
            );
            assert_eq!(
                app.path.as_ref(),
                Some(&paths[0]),
                "presented original waits for folder order"
            );
            assert_eq!(app.image_sequence.steps.len(), 100);
            app.pending_folder = None;
            app.apply_folder_snapshot(snapshot);
            app.render_frame();
            assert_eq!(
                app.path.as_ref(),
                Some(&paths[1]),
                "ready order releases the first queued direction"
            );
            for index in 1..=100 {
                assert_eq!(app.path.as_ref(), Some(&paths[index % 100]));
                let generation = app.media_generation;
                let deadline = Instant::now() + Duration::from_secs(10);
                while app.image_sequence.awaiting == Some(generation) {
                    assert!(
                        Instant::now() < deadline,
                        "original presentation timeout at step {index}"
                    );
                    app.render_frame();
                    assert!(
                        app.image_error.is_none(),
                        "original decode failed at step {index}"
                    );
                    if app.image_sequence.awaiting == Some(generation) {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
                assert_eq!(
                    app.path.as_ref(),
                    Some(&paths[if index == 100 { 0 } else { (index + 1) % 100 }]),
                    "the production render_frame must acknowledge and advance exactly once"
                );
            }
            assert!(app.image_sequence.awaiting.is_none() && app.image_sequence.steps.is_empty());
            assert!(app.playback_error.is_none());
            eprintln!(
                "PASS native image sequence: {}, 100 steps queued during initial loading, real decode/prefetch workers, late scripted order retained, render_frame GPU/Present acknowledgement; hidden window, not physical keys or a latency benchmark",
                if self.large {
                    "100 generated 4096x2304 JPEGs"
                } else {
                    "100 generated 2x1 BMPs"
                }
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
        .run_app(&mut Trial {
            root,
            large,
            native_order,
        })
        .expect("native sequence");
}
