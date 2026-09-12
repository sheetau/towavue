use super::*;
use std::sync::mpsc;
use towavue_runtime_windows::DecodedImageFrame;

type App = Application<Box<dyn Fn(AppEvent) + Send + Sync>>;

fn app() -> (App, mpsc::Receiver<AppEvent>) {
    let (sent, received) = mpsc::channel();
    let notify: Box<dyn Fn(AppEvent) + Send + Sync> = Box::new(move |event| {
        let _ = sent.send(event);
    });
    let mut app = Application::new(None, notify).expect("app");
    app.ui_context = Some(fonts::test_context());
    (app, received)
}

pub(crate) fn decoded(animated: bool) -> Arc<DecodedImage> {
    Arc::new(DecodedImage {
        format: "test",
        frames: (0..if animated { 2 } else { 1 })
            .map(|index| DecodedImageFrame {
                width: 2,
                height: 2,
                rgba: [index * 127, 80, 200, 255].repeat(4),
                delay: Duration::from_secs(1),
            })
            .collect(),
    })
}

pub(crate) fn install<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    path: PathBuf,
    decoded: Arc<DecodedImage>,
) -> TabId {
    let id = app.tabs.open_new(path.clone(), MediaKind::Image);
    app.load_path(path.clone(), MediaKind::Image);
    app.image_generation = app.image_loader.request(Vec::new());
    app.image_loading = false;
    app.clear_image_previews();
    app.image = Some(
        ImagePresentation::from_decoded(app.ui_context.as_ref().expect("context"), &path, decoded)
            .expect("image"),
    );
    app.state = PlaybackState::Paused;
    id
}

fn transfer(source: &mut App, destination: &mut App, id: TabId) -> TabId {
    let request = DetachRequest {
        tab: id,
        path: source
            .tabs
            .tabs()
            .iter()
            .find(|tab| tab.id == id)
            .expect("tab")
            .target
            .current_path()
            .to_owned(),
        instance: if source.displayed_tab == Some(id) {
            source.media_generation
        } else {
            source.retained_images[&id].instance
        },
    };
    let stage = source
        .prepare_image_transfer(id, destination.ui_context.as_ref().expect("context"))
        .expect("stage");
    let packet = source.take_tab_transfer(&request, stage);
    destination.accept_tab_transfer(packet, 0)
}

fn finish(app: &mut App, events: &mpsc::Receiver<AppEvent>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.image_loading || app.image_edit_pending {
        let event = events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("image worker");
        // Keep the synthetic Shell snapshot; only exercise the real image workers here.
        if matches!(event, AppEvent::ImagesReady | AppEvent::ImageEdited(_, _)) {
            app.handle_app_event(event);
        }
    }
}

#[test]
fn image_animation_suspends_until_resampling_finishes_or_undo_recovers() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_transfer::tests::image_animation_suspends_until_resampling_finishes_or_undo_recovers",
    ) else {
        return;
    };
    for succeeds in [false, true] {
        let (mut app, _) = app();
        let original = decoded(true);
        install(
            &mut app,
            root.join("animation-edit.png"),
            Arc::clone(&original),
        );
        app.folder_watcher = None;
        app.nearest_images = true;
        app.update_image_sampling();
        let resize = EditOperation::Resize(
            towavue_core::ImageResize::new(3, 3, towavue_core::ResampleFilter::Nearest)
                .expect("resize"),
        );
        app.push_visual_edit(resize);
        app.image_edit_worker.clear();
        let start = Instant::now() - Duration::from_millis(3500);
        let deadline = start + Duration::from_secs(1);
        app.image.as_mut().expect("image").next_frame_at = Some(deadline);
        let context = app.ui_context.clone().expect("context");
        let _ = context.tex_manager().write().take_delta();
        let assert_suspended = |app: &mut App| {
            let now = Instant::now();
            app.status_message = Some(("notice".into(), now));
            app.schedule();
            let image = app.image.as_ref().expect("retained pixels");
            assert_eq!(
                image.next_frame_at,
                Some(deadline),
                "non-displayed animation must not advance"
            );
            assert_eq!(image.frame_index, 0);
            assert!(context.tex_manager().write().take_delta().set.is_empty());
            assert_eq!(
                app.idle_wakeup(now),
                Some(now + STATUS_MESSAGE_DURATION),
                "status expiry survives without the animation wakeup"
            );
        };
        assert_suspended(&mut app);
        if succeeds {
            let resized = towavue_runtime_windows::render_image_edits(
                &original,
                &[resize],
                &Default::default(),
            )
            .expect("resized animation");
            app.finish_image_edits(app.image_edit_generation, Ok(Arc::new(resized)));
        } else {
            app.finish_image_edits(app.image_edit_generation, Err("resampling failed".into()));
            assert_suspended(&mut app);
            app.undo_edit(false);
            assert!(Arc::ptr_eq(
                &app.image.as_ref().expect("original pixels").decoded,
                &original
            ));
        }
        app.update_image_sampling();
        let _ = context.tex_manager().write().take_delta();
        let now = Instant::now();
        app.schedule();
        let image = app.image.as_ref().expect("resumed animation");
        let next = image.next_frame_at.expect("animation deadline");
        assert!(next > now);
        assert_eq!(
            (next - start).as_millis() % 2000,
            if image.frame_index == 0 { 1000 } else { 0 }
        );
        assert_eq!(image.sampling.get(), TextureOptions::NEAREST);
        assert_eq!(
            app.image_copy_request().expect("resumed copy").size,
            if succeeds { (3, 3) } else { (2, 2) }
        );
        let updates = context.tex_manager().write().take_delta().set;
        assert_eq!(updates.len(), usize::from(image.frame_index != 0));
        for (_, delta) in updates {
            assert_eq!(delta.options, TextureOptions::NEAREST);
            assert!(
                delta.image
                    == egui::ImageData::Color(Arc::new(color_image(
                        &image.decoded.frames[image.frame_index]
                    )))
            );
        }
    }
    // A failed first reading page must not freeze other successfully decoded pages.
    let (mut app, _) = app();
    install(&mut app, root.join("reading.png"), decoded(true));
    let mut page = app.image.take().expect("reading page");
    let deadline = Instant::now() - Duration::from_millis(2500);
    page.next_frame_at = Some(deadline);
    app.reading_pages = vec![Ok(page)];
    app.reading_mode = true;
    app.image_error = Some("first page failed".into());
    app.schedule();
    assert!(
        app.reading_pages[0]
            .as_ref()
            .expect("valid page")
            .next_frame_at
            > Some(deadline)
    );
}

#[test]
fn failed_reedit_rerenders_after_tab_return_or_window_transfer() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_transfer::tests::failed_reedit_rerenders_after_tab_return_or_window_transfer",
    ) else {
        return;
    };
    for moved in [false, true] {
        for retry_succeeds in [false, true] {
            let (mut source, events) = app();
            let original = decoded(true);
            let id = install(&mut source, root.join("reedit.png"), Arc::clone(&original));
            let resize = |size| {
                EditOperation::Resize(
                    towavue_core::ImageResize::new(
                        size,
                        size,
                        towavue_core::ResampleFilter::Nearest,
                    )
                    .expect("resize"),
                )
            };
            source.push_visual_edit(resize(3));
            finish_comparison(&mut source, &events);
            let saved = source.edits[&id].operations().to_vec();
            source
                .edits
                .get_mut(&id)
                .expect("history")
                .mark_exported(&saved);
            let old_pixels = Arc::clone(&source.image.as_ref().expect("first resize").decoded);
            source.push_visual_edit(resize(4));
            source.image_edit_worker.clear();
            source.finish_image_edits(
                source.image_edit_generation,
                Err("second resize failed".into()),
            );
            assert!(source.image_error.is_some() && source.image_copy_request().is_none());
            assert!(Arc::ptr_eq(
                &source
                    .image
                    .as_ref()
                    .expect("last successful pixels")
                    .decoded,
                &old_pixels
            ));
            let operations = source.edits[&id].operations().to_vec();
            let (mut target, events, id) = if moved {
                let (mut destination, destination_events) = app();
                let moved_id = transfer(&mut source, &mut destination, id);
                (destination, destination_events, moved_id)
            } else {
                install(&mut source, root.join("neighbor.png"), decoded(false));
                source.activate_tab(id);
                (source, events, id)
            };
            assert!(
                target.image_edit_pending,
                "a failed re-edit must render, not compare the preceding successful pixels"
            );
            assert!(target.image_copy_request().is_none());
            assert!(target.edits[&id].is_dirty());
            assert_eq!(target.edits[&id].operations(), operations);
            assert_eq!(target.edits[&id].saved_operations(), saved);
            assert!(Arc::ptr_eq(
                target
                    .image_edit_source
                    .as_ref()
                    .expect("original retained"),
                &original
            ));
            if retry_succeeds {
                finish_comparison(&mut target, &events);
                let expected = towavue_runtime_windows::render_image_edits(
                    &original,
                    &operations,
                    &Default::default(),
                )
                .expect("expected edit");
                assert_eq!(
                    *target.image.as_ref().expect("retried pixels").decoded,
                    expected
                );
                assert_eq!(
                    target.image_copy_request().expect("retried copy").size,
                    (4, 4)
                );
                assert!(
                    target.edits[&id].is_dirty(),
                    "the old saved pixels cannot mark a different edit clean"
                );
            } else {
                target.image_edit_worker.clear();
                target.finish_image_edits(target.image_edit_generation, Err("retry failed".into()));
                assert!(target.image_error.is_some() && target.image_copy_request().is_none());
                assert!(target.edits[&id].is_dirty());
            }
            target.undo_edit(false);
            finish(&mut target, &events);
            assert!(target.image_error.is_none());
            assert_eq!(target.image_copy_request().expect("undo copy").size, (3, 3));
            assert_eq!(
                target.image.as_ref().expect("saved pixels").decoded,
                old_pixels
            );
            assert!(!target.edits[&id].is_dirty());
        }
    }
}

#[test]
fn settled_image_comparison_survives_tab_switch_and_window_transfer() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_transfer::tests::settled_image_comparison_survives_tab_switch_and_window_transfer",
    ) else {
        return;
    };
    let (mut source, events) = app();
    let first = install(&mut source, root.join("edited.png"), decoded(true));
    source.push_visual_edit(EditOperation::Resize(
        towavue_core::ImageResize::new(3, 3, towavue_core::ResampleFilter::Nearest)
            .expect("resize"),
    ));
    finish_comparison(&mut source, &events);
    assert!(source.edits[&first].is_dirty());
    let pixels = Arc::clone(&source.image.as_ref().expect("edited").decoded);
    let texture = source.image.as_ref().expect("edited").texture.id();
    let second = install(&mut source, root.join("neighbor.png"), decoded(false));
    source.activate_tab(first);
    let generation = source.image_edit_generation;
    source.refresh_image_edits();
    assert_eq!(
        source.image_edit_generation, generation,
        "settled comparison remains reusable"
    );
    assert_eq!(
        source.image.as_ref().expect("restored").texture.id(),
        texture
    );
    assert!(Arc::ptr_eq(
        &source.image.as_ref().expect("restored").decoded,
        &pixels
    ));
    source.activate_tab(second);
    let generation = source.image_edit_generation;
    source.activate_tab(first);
    assert_eq!(
        source.image_edit_generation,
        generation.wrapping_add(1),
        "activation invalidates old worker generation but must not submit a fresh comparison"
    );
    let (mut destination, _) = app();
    let generation = destination.image_edit_generation;
    let moved = transfer(&mut source, &mut destination, first);
    assert_eq!(
        destination.image_edit_generation,
        generation.wrapping_add(1),
        "cross-window activation must not repeat a completed comparison"
    );
    assert!(destination.edits[&moved].is_dirty());
    assert!(Arc::ptr_eq(
        &destination.image.as_ref().expect("moved").decoded,
        &pixels
    ));
}

#[test]
fn interrupted_or_changed_image_comparisons_restart_after_tab_return() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_transfer::tests::interrupted_or_changed_image_comparisons_restart_after_tab_return",
    ) else {
        return;
    };
    let (mut app, events) = app();
    let first = install(&mut app, root.join("pending.png"), decoded(true));
    app.push_visual_edit(EditOperation::Resize(
        towavue_core::ImageResize::new(3, 3, towavue_core::ResampleFilter::Nearest)
            .expect("resize"),
    ));
    let stale_generation = app.image_edit_generation;
    let instance = app.media_generation;
    let deadline = Instant::now() + Duration::from_secs(5);
    // Receive the materialized pixels, but deliberately leave the comparison unaccepted.
    while app.image_edit_pending {
        let event = events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("render");
        if matches!(event, AppEvent::ImageEdited(..)) {
            app.handle_app_event(event);
        }
    }
    assert_eq!(app.image_edit_compared, None);
    let pixels = Arc::clone(&app.image.as_ref().expect("rendered").decoded);
    let operations = app.edits[&first].operations().to_vec();
    let second = install(&mut app, root.join("other.png"), decoded(false));
    assert!(app.retained_images[&first].comparison.is_none());
    let generation = app.image_edit_generation;
    app.activate_tab(first);
    assert_eq!(
        app.image_edit_generation,
        generation.wrapping_add(2),
        "incomplete comparison must restart"
    );
    assert!(
        !app.image_edit_pending,
        "already materialized pixels do not need rendering again"
    );
    app.handle_app_event(AppEvent::ImageContentCompared(
        stale_generation,
        instance,
        first,
        operations.clone(),
        vec![],
        true,
    ));
    assert_eq!(app.image_edit_compared, None);
    assert!(app.edits[&first].is_dirty());
    finish_comparison(&mut app, &events);
    assert_eq!(app.image_edit_compared, Some(false));
    app.activate_tab(second);
    app.edits
        .get_mut(&first)
        .expect("history")
        .mark_exported(&[EditOperation::FlipHorizontal]);
    let generation = app.image_edit_generation;
    app.activate_tab(first);
    assert_eq!(
        app.image_edit_generation,
        generation.wrapping_add(2),
        "changed saved snapshot needs new evidence"
    );
    assert_eq!(app.image_edit_compared, None);
    app.handle_app_event(AppEvent::ImageContentCompared(
        app.image_edit_generation,
        app.media_generation,
        first,
        operations,
        vec![],
        true,
    ));
    assert_eq!(
        app.image_edit_compared, None,
        "wrong baseline must not become reusable evidence"
    );
    assert!(app.edits[&first].is_dirty());
    finish_comparison(&mut app, &events);
    assert!(Arc::ptr_eq(
        &app.image.as_ref().expect("same render").decoded,
        &pixels
    ));
}

#[test]
fn retained_pixel_equivalence_restores_dirty_state_for_the_same_snapshots() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_transfer::tests::retained_pixel_equivalence_restores_dirty_state_for_the_same_snapshots",
    ) else {
        return;
    };
    let (mut app, events) = app();
    let first = install(&mut app, root.join("uniform.png"), decoded(true));
    app.push_visual_edit(EditOperation::FlipHorizontal);
    finish_comparison(&mut app, &events);
    assert_eq!(app.image_edit_compared, Some(true));
    assert!(!app.edits[&first].is_dirty());
    install(&mut app, root.join("neighbor.png"), decoded(false));
    app.edits
        .get_mut(&first)
        .expect("history")
        .mark_exported(&[]);
    assert!(
        app.edits[&first].is_dirty(),
        "history conservatively invalidates pixel evidence"
    );
    let generation = app.image_edit_generation;
    app.activate_tab(first);
    assert_eq!(app.image_edit_generation, generation.wrapping_add(1));
    assert!(
        !app.edits[&first].is_dirty(),
        "identical snapshots reuse the accepted match"
    );
    assert_eq!(
        app.edits[&first].operations(),
        &[EditOperation::FlipHorizontal]
    );
}

fn finish_comparison(app: &mut App, events: &mpsc::Receiver<AppEvent>) {
    let generation = app.image_edit_generation;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let event = events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("comparison");
        let finished =
            matches!(&event, AppEvent::ImageContentCompared(found, ..) if *found == generation);
        if matches!(
            event,
            AppEvent::ImageEdited(..) | AppEvent::ImageContentCompared(..)
        ) {
            app.handle_app_event(event);
        }
        if finished {
            break;
        }
    }
}

#[test]
fn final_image_transfer_clears_source_cache_without_dropping_destination_pixels() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_transfer::tests::final_image_transfer_clears_source_cache_without_dropping_destination_pixels",
    ) else {
        return;
    };
    let (mut source, _) = app();
    let (mut destination, _) = app();
    let audio_path = root.join("remaining.wav");
    let audio = source.tabs.open_new(audio_path.clone(), MediaKind::Audio);
    source.load_path(audio_path, MediaKind::Audio);
    let audio_instance = source.media_generation;
    let id = install(&mut source, root.join("moving.png"), decoded(false));
    let image = source.image.as_ref().expect("original").clone();
    let weak = Arc::downgrade(&image.decoded);
    let texture = image.texture.id();
    source.image_texture_cache.entries.push_back(image.into());
    let moved = transfer(&mut source, &mut destination, id);
    assert_eq!(source.tabs.active().expect("remaining audio").id, audio);
    assert_eq!(source.media_generation, audio_instance);
    assert!(source.image_texture_cache.entries.is_empty());
    assert!(
        source
            .ui_context
            .as_ref()
            .expect("source context")
            .tex_manager()
            .read()
            .meta(texture)
            .is_none()
    );
    assert!(Arc::ptr_eq(
        &weak.upgrade().expect("destination owns pixels"),
        &destination.image.as_ref().expect("moved original").decoded
    ));
    assert_eq!(
        destination
            .image
            .as_ref()
            .expect("moved original")
            .decoded
            .frames[0]
            .rgba,
        [0, 80, 200, 255].repeat(4)
    );
    destination.close_tab_unchecked(moved);
    assert!(weak.upgrade().is_none());
}

#[test]
fn image_transfer_rebinds_current_pixels_and_preserves_animation_edits_and_view() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_transfer::tests::image_transfer_rebinds_current_pixels_and_preserves_animation_edits_and_view",
    ) else {
        return;
    };
    for animated in [false, true] {
        for state in [PlaybackState::Playing, PlaybackState::Paused] {
            let (mut source, _) = app();
            let (mut destination, _) = app();
            let pixels_source = decoded(animated);
            let id = install(
                &mut source,
                root.join("not-on-disk.png"),
                Arc::clone(&pixels_source),
            );
            let neighbor = install(
                &mut destination,
                root.join("other-not-on-disk.png"),
                decoded(false),
            );
            source.state = state;
            source.image_view.zoom = ZoomMode::Custom(2.5);
            source.image_view.selection = Some(UnitRect {
                min: UnitPoint { x: 0.1, y: 0.2 },
                max: UnitPoint { x: 0.8, y: 0.9 },
            });
            source
                .edits
                .entry(id)
                .or_default()
                .push(EditOperation::FlipHorizontal, MediaKind::Image);
            let history = source.edits[&id].clone();
            let view = source.image_view;
            let image = source.image.as_mut().expect("image");
            image.frame_index = usize::from(animated);
            image.next_frame_at = animated.then(|| Instant::now() + Duration::from_secs(60));
            image.sampling.set(TextureOptions::NEAREST);
            let deadline = image.next_frame_at;
            let pixels = color_image(&pixels_source.frames[image.frame_index]);
            image.texture.set(pixels.clone(), TextureOptions::NEAREST);
            let sampler = image.sampling.clone();
            let source_id = image.texture.id();
            let destination_context = destination.ui_context.clone().expect("context");
            let _ = destination_context.tex_manager().write().take_delta();
            let moved = transfer(&mut source, &mut destination, id);
            assert!(source.tabs.tabs().is_empty() && source.image.is_none());
            assert!(source.edits.is_empty() && source.pending_guard.is_none());
            assert!(destination.retained_images.contains_key(&neighbor));
            assert_eq!(destination.edits[&moved], history);
            assert_eq!(destination.image_view, view);
            assert_eq!(destination.state, state);
            assert!(!destination.image_loading);
            let image = destination.image.as_ref().expect("moved image");
            assert!(Arc::ptr_eq(&image.decoded, &pixels_source));
            assert_eq!(image.frame_index, usize::from(animated));
            assert_eq!(image.next_frame_at, deadline);
            assert_eq!(image.sampling.get(), TextureOptions::NEAREST);
            assert!(!std::rc::Rc::ptr_eq(&image.sampling, &sampler));
            let delta = destination_context.tex_manager().write().take_delta();
            let upload = delta
                .set
                .iter()
                .find(|(id, _)| *id == image.texture.id())
                .expect("destination upload");
            assert!(
                upload.1.image == egui::ImageData::Color(Arc::new(pixels)),
                "current frame pixels uploaded to destination context"
            );
            assert_eq!(upload.1.options, TextureOptions::NEAREST);
            assert!(
                source
                    .ui_context
                    .as_ref()
                    .expect("source context")
                    .tex_manager()
                    .write()
                    .take_delta()
                    .free
                    .contains(&source_id)
            );
            let returned = transfer(&mut destination, &mut source, moved);
            assert_eq!(source.edits[&returned], history);
            assert!(Arc::ptr_eq(
                &source.image.as_ref().expect("returned").decoded,
                &pixels_source
            ));
        }
    }
}

#[test]
fn image_transfer_stages_all_textures_before_removing_source_state() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_transfer::tests::image_transfer_stages_all_textures_before_removing_source_state",
    ) else {
        return;
    };
    let (mut source, _) = app();
    let (destination, _) = app();
    let id = install(&mut source, root.join("source.png"), decoded(false));
    let page = Arc::new(DecodedImage {
        format: "test",
        frames: vec![DecodedImageFrame {
            width: 4,
            height: 4,
            rgba: vec![255; 64],
            delay: Duration::ZERO,
        }],
    });
    source
        .reading_pages
        .push(Ok(ImagePresentation::from_decoded(
            source.ui_context.as_ref().expect("context"),
            Path::new("page.png"),
            page,
        )
        .expect("page")));
    source
        .edits
        .entry(id)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    let context = destination.ui_context.as_ref().expect("context");
    context.input_mut(|input| input.max_texture_side = 2);
    let image = source.image.as_ref().expect("source").texture.id();
    assert!(source.prepare_image_transfer(id, context).is_err());
    assert_eq!(source.tabs.active().expect("source tab").id, id);
    assert_eq!(source.image.as_ref().expect("source").texture.id(), image);
    assert_eq!(source.reading_pages.len(), 1);
    assert!(source.edits[&id].is_dirty());
    assert!(destination.tabs.tabs().is_empty());
}

#[test]
fn transferred_pending_image_edit_restarts_from_shared_source_and_undo_restores_it() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_transfer::tests::transferred_pending_image_edit_restarts_from_shared_source_and_undo_restores_it",
    ) else {
        return;
    };
    let (mut source, _) = app();
    let (mut destination, events) = app();
    let original = decoded(false);
    let id = install(
        &mut source,
        root.join("not-on-disk.png"),
        Arc::clone(&original),
    );
    source.push_visual_edit(EditOperation::Resize(
        towavue_core::ImageResize::new(4, 4, towavue_core::ResampleFilter::Nearest)
            .expect("resize"),
    ));
    let stale = source.image_edit_generation;
    assert!(source.image_edit_pending);
    let history = source.edits[&id].clone();
    let moved = transfer(&mut source, &mut destination, id);
    assert!(destination.image_edit_pending);
    assert!(Arc::ptr_eq(
        destination.image_edit_source.as_ref().expect("original"),
        &original
    ));
    source.finish_image_edits(stale, Err("old window result".into()));
    assert!(source.image_error.is_none());
    finish(&mut destination, &events);
    assert!(destination.image_materialized && destination.image_error.is_none());
    assert_eq!(destination.edits[&moved], history);
    assert_eq!(
        destination.image.as_ref().expect("resized").dimensions(),
        (4, 4)
    );
    let returned = transfer(&mut destination, &mut source, moved);
    assert!(!source.image_edit_pending && source.image_materialized);
    assert_eq!(source.edits[&returned], history);
    source.undo_edit(false);
    assert!(Arc::ptr_eq(
        &source.image.as_ref().expect("undo").decoded,
        &original
    ));
}

pub(crate) fn bitmap(path: &Path) {
    let mut bytes = vec![0_u8; 62];
    bytes[..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&62_u32.to_le_bytes());
    bytes[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&2_u32.to_le_bytes());
    bytes[22..26].copy_from_slice(&1_u32.to_le_bytes());
    bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24_u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&8_u32.to_le_bytes());
    bytes[54..].copy_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0]);
    std::fs::write(path, bytes).expect("generated bitmap");
}

#[test]
fn image_transfer_resumes_only_missing_pages_and_keeps_loading_preview() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_transfer::tests::image_transfer_resumes_only_missing_pages_and_keeps_loading_preview",
    ) else {
        return;
    };
    for mode in 0..3 {
        let partial = mode != 0;
        let primary_failed = mode == 2;
        let (mut source, _) = app();
        let (mut destination, events) = app();
        let last = root.join("remaining.bmp");
        bitmap(&last);
        let paths = if partial {
            vec![
                root.join("not-on-disk.png"),
                root.join("second-not-on-disk.png"),
                root.join("failed-not-on-disk.png"),
                last.clone(),
            ]
        } else {
            vec![last.clone()]
        };
        let original = decoded(false);
        let neighbor = decoded(true);
        let id = install(&mut source, paths[0].clone(), Arc::clone(&original));
        let snapshot = FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![0]),
            folder_path: root.clone(),
            items: paths
                .iter()
                .enumerate()
                .map(|(index, path)| towavue_core::FolderMediaItem {
                    identity: towavue_core::ShellIdentity::new(vec![index as u8]),
                    path: path.clone(),
                    kind: MediaKind::Image,
                })
                .collect(),
            sort_columns: Vec::new(),
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 19,
            captured_at: std::time::SystemTime::UNIX_EPOCH,
        };
        if partial {
            source.reading_mode = true;
            source.reading_settings.page_count = 4;
            source.reading_settings.first_page_count = 4;
            source.reading_settings.axis = towavue_core::ReadingAxis::Vertical;
            source.reading_settings.reversed = true;
            source.folder_snapshot = Some(snapshot.clone());
            source
                .reading_pages
                .push(Ok(ImagePresentation::from_decoded(
                    source.ui_context.as_ref().expect("context"),
                    &paths[1],
                    Arc::clone(&neighbor),
                )
                .expect("loaded neighbor")));
            source
                .reading_pages
                .push(Err("known failed neighbor".into()));
        } else {
            source.image = None;
        }
        if primary_failed {
            source.image = None;
            source.image_error = Some("known primary failure".into());
        }
        source.image_loading = true;
        source.pending_image_previews.insert(last.clone());
        source.finish_image_preview(
            last.clone(),
            source.image_preview_generation,
            towavue_runtime_windows::CachedImagePreview {
                image: towavue_runtime_windows::PreviewImage {
                    width: 1,
                    height: 1,
                    rgba: vec![0, 0, 255, 255],
                },
                source_size: (2, 1),
            },
        );
        let pixels = Arc::clone(&source.image_previews[&last].pixels);
        let stale_generation = source.image_generation;
        transfer(&mut source, &mut destination, id);
        assert!(destination.image_loading);
        assert_eq!(
            destination.image_request_offset,
            if partial { 3 } else { 0 }
        );
        assert!(Arc::ptr_eq(
            &destination.image_previews[&last].pixels,
            &pixels
        ));
        let context = destination.ui_context.clone().expect("destination context");
        let _ = context.run_ui(Default::default(), |ui| {
            ui.label("preview recovery fixture");
        });
        assert!(
            destination
                .restored_ui_textures(&context)
                .iter()
                .any(|(id, delta)| {
                    *id == destination.image_previews[&last].texture.id()
                        && delta.image == egui::ImageData::Color(Arc::clone(&pixels))
                }),
            "retained loading previews must be uploaded after graphics recovery"
        );
        if partial {
            assert_eq!(destination.reading_pages.len(), 2);
            let generation = destination.image_generation;
            destination.apply_folder_snapshot(snapshot);
            assert_eq!(
                destination.image_generation, generation,
                "unchanged Shell order must not restart a resumed request"
            );
        }
        source.apply_loaded_images(towavue_runtime_windows::LoadedImages {
            generation: stale_generation,
            first_index: 0,
            total: 1,
            images: vec![(paths[0].clone(), Ok(decoded(false)))],
        });
        assert!(
            source.image.is_none(),
            "old owner rejects its queued completion"
        );
        finish(&mut destination, &events);
        assert!(!destination.image_loading && destination.image_previews.is_empty());
        if partial {
            assert_eq!(destination.reading_pages.len(), 3);
            assert!(Arc::ptr_eq(
                &destination.reading_pages[0]
                    .as_ref()
                    .expect("retained page")
                    .decoded,
                &neighbor
            ));
            assert_eq!(
                destination.reading_pages[1]
                    .as_ref()
                    .err()
                    .map(String::as_str),
                Some("known failed neighbor")
            );
            assert_eq!(
                destination.reading_pages[2]
                    .as_ref()
                    .expect("new page")
                    .dimensions(),
                (2, 1)
            );
            if primary_failed {
                assert_eq!(
                    destination.image_error.as_deref(),
                    Some("known primary failure")
                );
            } else {
                assert!(Arc::ptr_eq(
                    &destination
                        .image
                        .as_ref()
                        .expect("retained primary")
                        .decoded,
                    &original
                ));
                assert!(destination.image_error.is_none());
            }
        } else {
            assert_eq!(
                destination
                    .image
                    .as_ref()
                    .expect("first decoded image")
                    .dimensions(),
                (2, 1)
            );
        }
    }
}
