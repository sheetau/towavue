use super::*;
use crate::preview_transport::{Action, Transport};
use towavue_core::{CommandId, MediaTime, PlaybackState};

#[test]
fn video_thumbnail_and_sheet_keep_the_same_viewport_without_extra_horizontal_padding() {
    for density in [1.0, 1.25, 2.0] {
        for (width, height) in [(240, 135), (90, 160), (240, 100)] {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            context.enable_accesskit();
            context.global_style_mut(crate::chrome::style);
            let mut tabs = TabSet::default();
            tabs.open_new("video.mp4".into(), MediaKind::Video);
            let mut preview = TabPreview::new().expect("preview worker");
            let target = preview
                .target(tabs.active().expect("tab"))
                .expect("file-backed preview");
            preview.target = Some(target.clone());
            preview.finish(
                &context,
                target.clone(),
                preview.generation,
                Ok(PreviewImage {
                    width,
                    height,
                    rgba: vec![255; (width * height * 4) as usize].into(),
                }),
            );
            let source =
                egui::Rect::from_min_size(egui::pos2(220.0, 20.0), egui::vec2(100.0, 24.0));
            let mut time = 0.0;
            let mut frame = |preview: &TabPreview, point| {
                time += 0.05;
                context.run_ui(
                    egui::RawInput {
                        time: Some(time),
                        focused: true,
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(600.0, 400.0),
                        )),
                        events: vec![egui::Event::PointerMoved(point)],
                        ..Default::default()
                    },
                    |ui| {
                        let response =
                            ui.interact(source, "video-tab".into(), egui::Sense::click());
                        preview.show_with_transport(
                            &response,
                            &target,
                            None,
                            Some(&Transport {
                                instance: 1,
                                kind: MediaKind::Video,
                                state: PlaybackState::Paused,
                                position: MediaTime::ZERO,
                                duration: Some(MediaTime::from_nanoseconds(100_000_000_000)),
                                enabled: true,
                                previous: false,
                                next: false,
                            }),
                            None,
                        );
                    },
                )
            };
            let seek_bounds = |output: &egui::FullOutput| {
                output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("tree")
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some("Preview playback position (seconds)"))
                    .expect("seek control")
                    .1
                    .bounds()
                    .expect("seek bounds")
            };
            for _ in 0..3 {
                frame(&preview, source.center());
            }
            let before = seek_bounds(&frame(&preview, source.center()));
            let pointer = egui::pos2(
                ((before.x0 + before.x1) * 0.5) as f32,
                before.y0 as f32 - 20.0,
            );
            frame(&preview, pointer);
            preview.finish_sheet(
                &context,
                target.clone(),
                preview.generation,
                Ok(towavue_runtime_windows::VideoPreviewSheet {
                    layout: towavue_runtime_windows::VideoSheetLayout::for_position(
                        Duration::from_secs(100),
                        Duration::ZERO,
                    )
                    .expect("layout"),
                    image: PreviewImage {
                        width: 960,
                        height: 640,
                        rgba: vec![255; 960 * 640 * 4].into(),
                    },
                }),
            );
            let texture = preview
                .texture
                .as_ref()
                .expect("texture")
                .as_ref()
                .expect("ready")
                .id();
            let after = frame(&preview, pointer);
            assert_eq!(
                seek_bounds(&after),
                before,
                "sheet arrival keeps controls stationary"
            );
            let image = after
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.texture_id == texture => {
                        Some(mesh.calc_bounds())
                    }
                    _ => None,
                })
                .reduce(egui::Rect::union)
                .expect("sheet pixels");
            assert!(
                (image.width() - 240.0).abs() <= 1.0 / density,
                "sheet cannot acquire extra horizontal padding: {image:?}"
            );
            assert!((image.height() - 160.0).abs() <= 1.0 / density);
        }
    }
}

#[test]
fn audio_card_navigation_keeps_pointer_ownership_when_artwork_shrinks_or_loads() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.enable_accesskit();
        context.set_pixels_per_point(density);
        context.global_style_mut(crate::chrome::style);
        let mut tabs = TabSet::default();
        let id = tabs.open_new("first.wav".into(), MediaKind::Audio);
        let mut preview = TabPreview::new().expect("preview worker");
        let large = context.load_texture(
            "large cover",
            egui::ColorImage::filled([240, 160], egui::Color32::RED),
            TextureOptions::LINEAR,
        );
        let small = context.load_texture(
            "small cover",
            egui::ColorImage::filled([240, 40], egui::Color32::BLUE),
            TextureOptions::LINEAR,
        );
        let source = egui::Rect::from_min_size(egui::pos2(220.0, 20.0), egui::vec2(100.0, 24.0));
        let mut time = 0.0;
        let mut frame = |preview: &mut TabPreview, stage, events| {
            time += 0.01;
            let target = Target {
                tab: id,
                path: if stage == 0 { "first.wav" } else { "next.wav" }.into(),
                kind: MediaKind::Audio,
                position: Duration::ZERO,
            };
            preview.target = Some(target.clone());
            preview.texture = match stage {
                0 | 4 => Some(Ok(large.clone())),
                2 => Some(Ok(small.clone())),
                3 => Some(Err("no artwork".into())),
                _ => None,
            };
            let transport = Transport {
                instance: if stage == 0 { 1 } else { 2 },
                kind: MediaKind::Audio,
                state: PlaybackState::Paused,
                position: MediaTime::ZERO,
                duration: Some(crate::media_time(Duration::from_secs(60))),
                enabled: stage != 1,
                previous: stage != 1,
                next: stage != 1,
            };
            let mut action = None;
            let output = context.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600.0, 400.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let response =
                        ui.interact(source, "audio-tab".into(), egui::Sense::click_and_drag());
                    action = preview.show_with_transport(
                        &response,
                        &target,
                        None,
                        Some(&transport),
                        None,
                    );
                },
            );
            (output, action)
        };
        for _ in 0..3 {
            frame(
                &mut preview,
                0,
                vec![egui::Event::PointerMoved(source.center())],
            );
        }
        let layer = egui::Id::new("audio-tab").with("media-preview");
        let card = context
            .memory(|memory| memory.area_rect(layer))
            .expect("card");
        let thumbnail_center = egui::pos2(card.center().x, card.top() + 81.0);
        let output = frame(
            &mut preview,
            0,
            vec![egui::Event::PointerMoved(thumbnail_center)],
        )
        .0;
        let bounds = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("tree")
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Next track"))
            .expect("next button")
            .1
            .bounds()
            .expect("bounds");
        let point = egui::pos2(
            ((bounds.x0 + bounds.x1) / 2.0) as f32,
            ((bounds.y0 + bounds.y1) / 2.0) as f32,
        );
        let button = |pressed| egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&mut preview, 0, vec![egui::Event::PointerMoved(point)]);
        frame(&mut preview, 0, vec![button(true)]);
        assert_eq!(
            frame(&mut preview, 0, vec![button(false)]).1,
            Some(Action::Command(CommandId::NextMedia))
        );
        for stage in [1, 2, 3, 4] {
            for _ in 0..3 {
                let output = frame(&mut preview, stage, vec![]).0;
                assert!(
                    output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Text(text) if text.galley.text() == "next.wav")),
                    "track replacement must keep the card open: stage={stage}, density={density}"
                );
                let next = output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("new tree")
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some("Next track"))
                    .expect("next track after replacement");
                assert_eq!(
                    next.1.bounds(),
                    Some(bounds),
                    "track replacement keeps the button under the pointer"
                );
                assert_eq!(next.1.is_disabled(), stage == 1);
            }
        }
        assert_eq!(frame(&mut preview, 4, vec![button(true)]).1, None);
        assert_eq!(
            frame(&mut preview, 4, vec![button(false)]).1,
            Some(Action::Command(CommandId::NextMedia))
        );
        frame(
            &mut preview,
            2,
            vec![egui::Event::PointerMoved(egui::pos2(5.0, 350.0))],
        );
        for _ in 0..3 {
            frame(
                &mut preview,
                2,
                vec![egui::Event::PointerMoved(source.center())],
            );
        }
        let reopened = context
            .memory(|memory| memory.area_rect(layer))
            .expect("reopened card");
        assert!(
            reopened.height() < card.height() - 100.0,
            "reopening restores the current artwork's natural height"
        );
        let small_center = egui::pos2(reopened.center().x, reopened.top() + 21.0);
        frame(
            &mut preview,
            2,
            vec![egui::Event::PointerMoved(small_center)],
        );
        for _ in 0..3 {
            let output = frame(&mut preview, 4, vec![]).0;
            assert!(
                output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Mesh(mesh) if mesh.texture_id == large.id())),
                "new artwork is displayed"
            );
            assert_eq!(
                context.memory(|memory| memory.area_rect(layer)),
                Some(reopened),
                "artwork growth also preserves the operated card"
            );
        }
    }
}
