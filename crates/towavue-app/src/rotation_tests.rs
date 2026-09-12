use crate::*;
use towavue_core::ImageRotation;

#[test]
fn free_rotation_worker_keeps_animation_original_pixels_and_undo_redo_tab_generations() {
    let Some(root) = crate::tests::isolated_test_root(
        "rotation_tests::free_rotation_worker_keeps_animation_original_pixels_and_undo_redo_tab_generations",
    ) else {
        return;
    };
    let (sender, events) = std::sync::mpsc::channel();
    let mut app = Application::new(None, move |event| {
        let _ = sender.send(event);
    })
    .expect("app");
    let context = fonts::test_context();
    app.ui_context = Some(context.clone());
    let path = root.join("rotation.png");
    let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
    app.path = Some(path.clone());
    app.media_kind = Some(MediaKind::Image);
    app.displayed_tab = Some(tab);
    let source = Arc::new(DecodedImage {
        format: "test",
        frames: [31, 71]
            .into_iter()
            .map(|value| towavue_runtime_windows::DecodedImageFrame {
                width: 9,
                height: 7,
                rgba: (0..63)
                    .flat_map(|index| {
                        [
                            value,
                            index as u8 * 3,
                            255 - index as u8 * 3,
                            if index % 3 == 0 { 0 } else { 255 },
                        ]
                    })
                    .collect(),
                delay: Duration::from_millis(value as u64),
            })
            .collect(),
    });
    let original = source.clone();
    let original_pixels = (*source).clone();
    app.image =
        Some(ImagePresentation::from_decoded(&context, &path, source.clone()).expect("source"));
    app.image.as_mut().expect("image").frame_index = 1;
    let settle = |app: &mut Application<_>| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.image_edit_pending {
            app.handle_app_event(
                events
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .expect("worker result"),
            );
        }
        assert!(app.image_error.is_none(), "{:?}", app.image_error);
    };
    let rotation = ImageRotation::new(317, (9, 7)).expect("rotation");
    app.push_visual_edit(EditOperation::RotateImage(rotation));
    assert!(app.image_edit_pending);
    assert!(app.image_copy_request().is_none());
    let expected = towavue_runtime_windows::render_image_edits(
        &source,
        app.edits[&tab].operations(),
        &towavue_runtime_windows::Cancellation::default(),
    )
    .expect("reference pixels");
    settle(&mut app);
    assert!(app.image_materialized);
    assert_eq!(*app.image.as_ref().expect("rotated").decoded, expected);
    for (rotated, original) in app
        .image
        .as_ref()
        .expect("rotated")
        .decoded
        .frames
        .iter()
        .zip(&source.frames)
    {
        assert_eq!(rotated.delay, original.delay);
    }
    assert_eq!(app.image.as_ref().expect("rotated").frame_index, 1);
    assert_eq!(
        app.visual_transform(rotation.size()).size,
        (rotation.size().0 as f32, rotation.size().1 as f32),
        "no second rotation"
    );
    let copy = app.image_copy_request().expect("copy snapshot");
    assert_eq!(copy.size, rotation.size());
    assert_eq!(copy.source_uv, ImageTransform::new(rotation.size(), &[]).uv);
    assert_eq!(*copy.image, expected);
    app.undo_edit(false);
    assert!(!app.image_materialized && !app.image_edit_pending);
    assert!(Arc::ptr_eq(
        &app.image.as_ref().expect("original").decoded,
        &source
    ));
    app.undo_edit(true);
    settle(&mut app);
    assert_eq!(*app.image.as_ref().expect("redo").decoded, expected);
    let (width, height) = rotation.size();
    app.push_visual_edit(EditOperation::Crop(PixelCrop {
        x: 1,
        y: 1,
        width: width - 2,
        height: height - 2,
    }));
    settle(&mut app);
    let next = ImageRotation::new(-219, (width - 2, height - 2)).expect("second rotation");
    app.push_visual_edit(EditOperation::RotateImage(next));
    let stale = app.image_edit_generation;
    let other_path = root.join("other.png");
    let other = app.tabs.open_new(other_path.clone(), MediaKind::Image);
    app.load_path(other_path.clone(), MediaKind::Image);
    app.image_generation = app.image_loader.request(Vec::new());
    app.image_loading = false;
    app.image = Some(
        ImagePresentation::from_decoded(&context, &other_path, source.clone())
            .expect("other image"),
    );
    app.finish_image_edits(stale, Err("old tab".into()));
    assert!(app.image_error.is_none());
    app.activate_tab(tab);
    assert!(app.image_edit_pending);
    settle(&mut app);
    let expected = towavue_runtime_windows::render_image_edits(
        &source,
        app.edits[&tab].operations(),
        &towavue_runtime_windows::Cancellation::default(),
    )
    .expect("composed reference");
    assert_eq!(
        *app.image.as_ref().expect("restored composition").decoded,
        expected
    );
    assert_eq!(
        app.image
            .as_ref()
            .expect("restored composition")
            .dimensions(),
        next.size()
    );
    assert!(Arc::ptr_eq(
        app.image_edit_source.as_ref().expect("retained original"),
        &original
    ));
    app.undo_edit(false);
    let stale = app.image_edit_generation;
    app.finish_image_edits(stale, Err("injected worker failure".into()));
    assert!(app.image_error.is_some() && app.image_copy_request().is_none());
    app.undo_edit(false);
    app.undo_edit(false);
    assert!(!app.image_materialized && !app.image_edit_pending && app.image_error.is_none());
    assert!(Arc::ptr_eq(
        &app.image.as_ref().expect("source after failure").decoded,
        &source
    ));
    app.finish_image_edits(stale, Ok(Arc::new(expected)));
    assert!(!app.image_materialized && app.image_error.is_none());
    app.remove_tab(tab, false);
    app.remove_tab(other, false);
    app.finish_image_edits(stale, Err("closed tab".into()));
    assert!(app.image.is_none() && app.image_error.is_none());
    assert_eq!(*source, original_pixels);
}
