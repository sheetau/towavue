use super::*;

fn image_meshes(
    app: &mut App,
    context: &egui::Context,
    density: f32,
) -> Vec<(egui::Rect, egui::Mesh)> {
    let texture = app.image.as_ref().expect("image").texture.id();
    context
        .run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                viewports: [(
                    egui::ViewportId::ROOT,
                    egui::ViewportInfo {
                        native_pixels_per_point: Some(density),
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
                ..Default::default()
            },
            |ui| app.draw_image(ui),
        )
        .shapes
        .into_iter()
        .filter_map(|shape| match shape.shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id == texture => {
                Some((shape.clip_rect, (*mesh).clone()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn pending_raster_edits_keep_previous_pixels_geometry_and_resume_after_undo() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_handoff::tests::edit::pending_raster_edits_keep_previous_pixels_geometry_and_resume_after_undo",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for succeeds in [false, true] {
            let (mut app, context, tab) = fixture(&root);
            let original = app.image.as_ref().expect("image").decoded.clone();
            app.push_visual_edit(EditOperation::Crop(PixelCrop {
                x: 11,
                y: 7,
                width: 100,
                height: 60,
            }));
            app.push_visual_edit(EditOperation::RotateClockwise);
            app.image_view.zoom = ZoomMode::Custom(6.0);
            app.image_view.pan = (13.0, -11.0);
            image_meshes(&mut app, &context, density);
            let before = image_meshes(&mut app, &context, density);
            assert!(!before.is_empty(), "the original image must be painted");
            let texture = app.image.as_ref().expect("image").texture.id();
            let _ = context.tex_manager().write().take_delta();
            let rotation = towavue_core::ImageRotation::new(50, (60, 100)).expect("rotation");
            app.push_visual_edit(EditOperation::RotateImage(rotation));
            app.image_edit_worker.clear();
            assert!(app.image_edit_pending);
            for _ in 0..3 {
                assert_eq!(image_meshes(&mut app, &context, density), before);
                assert_eq!(app.image.as_ref().expect("image").texture.id(), texture);
                assert!(context.tex_manager().write().take_delta().set.is_empty());
            }
            let abandoned_generation = app.image_edit_generation;
            let saved = app.take_image_tab_state();
            app.reset_image_edits();
            app.restore_image_tab(saved);
            app.image_edit_worker.clear();
            assert!(app.image_edit_pending);
            assert_eq!(image_meshes(&mut app, &context, density), before);
            app.finish_image_edits(abandoned_generation, Err("departed worker".into()));
            assert!(app.image_error.is_none());
            assert_eq!(image_meshes(&mut app, &context, density), before);
            let operations = app.edits[&tab].operations().to_vec();
            let rendered = Arc::new(
                towavue_runtime_windows::render_image_edits(
                    &original,
                    &operations,
                    &Default::default(),
                )
                .expect("rendered rotation"),
            );
            app.finish_image_edits(
                app.image_edit_generation.wrapping_sub(1),
                Ok(rendered.clone()),
            );
            assert!(app.image_edit_pending);
            assert_eq!(image_meshes(&mut app, &context, density), before);
            if succeeds {
                app.finish_image_edits(app.image_edit_generation, Ok(rendered.clone()));
                assert!(!app.image_edit_pending);
                assert!(app.image.as_ref().expect("image").held_edit_view.is_none());
                assert_eq!(*app.image.as_ref().expect("image").decoded, *rendered);
                assert!(!image_meshes(&mut app, &context, density).is_empty());
                // The next materialized edit holds the already edited texture, not the source.
                let edited = image_meshes(&mut app, &context, density);
                let next =
                    towavue_core::ImageRotation::new(-317, rotation.size()).expect("next rotation");
                app.push_visual_edit(EditOperation::RotateImage(next));
                app.image_edit_worker.clear();
                assert_eq!(image_meshes(&mut app, &context, density), edited);
                app.undo_edit(false);
                app.image_edit_worker.clear();
                assert_eq!(image_meshes(&mut app, &context, density), edited);
                app.finish_image_edits(app.image_edit_generation, Ok(rendered));
            } else {
                app.finish_image_edits(
                    app.image_edit_generation,
                    Err("controlled resample failure".into()),
                );
                assert!(app.image_error.is_some());
                assert_eq!(image_meshes(&mut app, &context, density), before);
            }
            app.undo_edit(false);
            assert!(!app.image_edit_pending);
            assert!(app.image_error.is_none());
            assert!(app.image.as_ref().expect("image").held_edit_view.is_none());
            assert!(Arc::ptr_eq(
                &app.image.as_ref().expect("source").decoded,
                &original
            ));
            assert_eq!(app.edits[&tab].operations().len(), 2);
            let restored = image_meshes(&mut app, &context, density);
            app.undo_edit(true);
            app.image_edit_worker.clear();
            assert!(app.image_edit_pending);
            assert_eq!(image_meshes(&mut app, &context, density), restored);
        }
    }
}
