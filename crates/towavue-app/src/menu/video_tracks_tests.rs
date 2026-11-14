use super::*;
use egui::accesskit::{Action, ActionRequest, TreeId};

#[test]
fn video_tracks_menu_keeps_localized_long_rows_bounded_and_selectable() {
    for density in [1.0, 1.25, 2.0] {
        for japanese in [false, true] {
            let context = if japanese {
                crate::localization::test_ui::japanese_context(density)
            } else {
                crate::fonts::test_context()
            };
            context.set_pixels_per_point(density);
            context.enable_accesskit();
            context.global_style_mut(crate::chrome::style);
            let track = towavue_core::VideoTrack {
                id: towavue_core::VideoTrackId::from_index(3),
                title: Some("Long camera description ".repeat(8)),
                language: Some("eng".into()),
            };
            let catalog = towavue_core::VideoTrackCatalog {
                tracks: vec![track.clone()],
                preferred: Some(track.id),
            };
            let chosen = std::cell::Cell::new(None);
            let frame = |events| {
                let mut data = MenuData {
                    video_tracks: Some(&catalog),
                    ..Default::default()
                };
                let result = context.run_ui(
                    egui::RawInput {
                        events,
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(480.0, 700.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        ui.menu_button("Menu", |ui| {
                            show_items(
                                ui,
                                Text::MenuView,
                                CommandContext {
                                    media_kind: Some(towavue_core::MediaKind::Video),
                                    ..Default::default()
                                },
                                &crate::shortcuts::defaults(),
                                &mut data,
                                None,
                            );
                        });
                    },
                );
                if data.video_action.is_some() {
                    chosen.set(data.video_action);
                }
                result
            };
            let click = |id| {
                egui::Event::AccessKitActionRequest(ActionRequest {
                    action: Action::Click,
                    target_tree: TreeId::ROOT,
                    target_node: id,
                    data: None,
                })
            };
            for label in [
                "Menu".to_owned(),
                text(&context, Text::VideoTracks).to_owned(),
            ] {
                for _ in 0..4 {
                    frame(vec![]);
                }
                let result = frame(vec![]);
                let tree = result
                    .platform_output
                    .accesskit_update
                    .expect("video track verification");
                let row = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| {
                        node.label()
                            .is_some_and(|value| value.trim_end_matches('⏵').trim() == label)
                    })
                    .unwrap_or_else(|| panic!("missing {label}"));
                frame(vec![click(row.0)]);
            }
            for _ in 0..4 {
                frame(vec![]);
            }
            let result = frame(vec![]);
            let tree = result
                .platform_output
                .accesskit_update
                .expect("video track verification");
            let label = crate::video_tracks::track_label(language(&context), 0, &track);
            let row = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label.as_str()))
                .expect("track label");
            let bounds = row.1.bounds().expect("video track verification");
            assert!(bounds.x0 >= 0.0 && bounds.x1 <= 481.0);
            frame(vec![click(row.0)]);
            assert_eq!(chosen.get(), Some(track.id));
        }
    }
}
