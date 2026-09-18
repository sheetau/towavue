use super::*;

#[test]
fn gallery_media_durations_have_readable_backdrops_above_ready_thumbnails() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::duration_tests::gallery_media_durations_have_readable_backdrops_above_ready_thumbnails",
    ) else {
        return;
    };
    let paths = [
        root.join("movie.mp4"),
        root.join("audio.flac"),
        root.join("image.png"),
    ];
    for density in [1.0, 1.25, 2.0] {
        for width in [260.0, 660.0] {
            let context = crate::fonts::test_context();
            context.global_style_mut(crate::chrome::style);
            let mut strip =
                Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                    .expect("preview worker");
            let mut textures = Vec::new();
            for (index, duration) in [
                Some(Duration::from_secs(65)),
                Some(Duration::from_secs(3601)),
                None,
            ]
            .into_iter()
            .enumerate()
            {
                let texture = context.load_texture(
                    format!("badge-fixture-{index}"),
                    egui::ColorImage::filled([3, 2], Color32::WHITE),
                    egui::TextureOptions::LINEAR,
                );
                textures.push(texture.id());
                strip
                    .previews
                    .insert(paths[index].clone(), Ok((texture, duration)));
            }
            for enabled in [true, false] {
                for _ in 0..3 {
                    let mut actions = Vec::new();
                    let output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 500.0),
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
                        |ui| {
                            strip.show_recent(ui, &paths, 1, enabled, &mut actions);
                        },
                    );
                    assert!(actions.is_empty());
                    assert_eq!(output.pixels_per_point, density);
                    let backgrounds = output.shapes.iter().filter(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == Color32::from_black_alpha(210))).count();
                    assert_eq!(backgrounds, 2, "only video/audio durations receive badges");
                    for (index, seconds) in [65, 3601].into_iter().enumerate() {
                        let label = format_time(media_time(Duration::from_secs(seconds)));
                        let (text_index, text) = output
                            .shapes
                            .iter()
                            .enumerate()
                            .find_map(|(index, shape)| match &shape.shape {
                                egui::Shape::Text(text) if text.galley.text() == label => {
                                    Some((index, text))
                                }
                                _ => None,
                            })
                            .expect("duration label");
                        let egui::Shape::Rect(background) = &output.shapes[text_index - 1].shape
                        else {
                            panic!("backdrop before duration text");
                        };
                        assert_eq!(background.fill, Color32::from_black_alpha(210));
                        assert_eq!(
                            background.rect,
                            Rect::from_min_size(text.pos, text.galley.size()).expand(1.0)
                        );
                        assert_eq!(background.corner_radius, egui::CornerRadius::same(1));
                        let image_index = output.shapes.iter().position(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == textures[index])).expect("ready thumbnail");
                        assert!(image_index < text_index - 1, "badge is above its thumbnail");
                        assert!(
                            output.shapes[text_index]
                                .clip_rect
                                .contains_rect(background.rect)
                        );
                    }
                }
            }
        }
    }
}
