use super::*;
use crate::localization::Language;
use egui::accesskit::{Action, ActionRequest, NodeId, Toggled, TreeId};

fn node<'a>(output: &'a egui::FullOutput, label: &str) -> (NodeId, &'a egui::accesskit::Node) {
    let (id, node) = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree")
        .nodes
        .iter()
        .find(|(_, node)| node.label().is_some_and(|name| name.starts_with(label)))
        .unwrap_or_else(|| panic!("missing {label}"));
    (*id, node)
}

fn access(target: NodeId, action: Action) -> egui::Event {
    egui::Event::AccessKitActionRequest(ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: target,
        data: None,
    })
}

#[test]
fn image_choices_keep_custom_shortcuts_before_arrows_and_current_state_is_a_no_op() {
    for (width, nested) in [(660.0, false), (480.0, true)] {
        for language in [Language::English, Language::Japanese] {
            for density in [1.0, 1.25, 2.0] {
                for (command, title) in [
                    (ToggleImageInterpolation, Text::ImageInterpolation),
                    (ToggleImageMinification, Text::ImageMinification),
                ] {
                    for current in [false, true] {
                        for same in [false, true] {
                            let context = crate::fonts::test_context();
                            context.enable_accesskit();
                            context.global_style_mut(crate::chrome::style);
                            context.set_pixels_per_point(density);
                            if language == Language::Japanese {
                                crate::localization::test_ui::configure_japanese(&context, density);
                            }
                            let mut bindings = crate::shortcuts::defaults();
                            bindings.set(command, "Ctrl+Alt+I".parse().expect("custom binding"));
                            let commands = CommandContext {
                                media_kind: Some(towavue_core::MediaKind::Image),
                                ..Default::default()
                            };
                            let shortcut = bindings.label(command, commands);
                            let frame = |events| {
                                let mut chosen = Vec::new();
                                let output = context.run_ui(
                                    egui::RawInput {
                                        screen_rect: Some(egui::Rect::from_min_size(
                                            egui::Pos2::ZERO,
                                            egui::vec2(width, 1100.0),
                                        )),
                                        events,
                                        ..Default::default()
                                    },
                                    |ui| {
                                        ui.menu_button("Menu", |ui| {
                                            let mut data = MenuData {
                                                choices: Choices {
                                                    nearest_images: current,
                                                    high_quality_minification: current,
                                                    ..Default::default()
                                                },
                                                ..Default::default()
                                            };
                                            if let Some(command) = show_section_with_recent(
                                                ui,
                                                commands,
                                                &bindings,
                                                (!nested).then_some(Section::View),
                                                &mut data,
                                            ) {
                                                chosen.push(command);
                                            }
                                        });
                                    },
                                );
                                (output, chosen)
                            };
                            let output = frame(vec![]).0;
                            frame(vec![access(node(&output, "Menu").0, Action::Click)]);
                            for _ in 0..4 {
                                frame(vec![]);
                            }
                            if nested {
                                let output = frame(vec![]).0;
                                frame(vec![access(
                                    node(&output, Text::MenuView.in_language(language)).0,
                                    Action::Click,
                                )]);
                                for _ in 0..4 {
                                    frame(vec![]);
                                }
                            }
                            let output = frame(vec![]).0;
                            let (parent, parent_node) = node(&output, title.in_language(language));
                            let bounds = parent_node.bounds().expect("parent bounds");
                            let texts = output
                                .shapes
                                .iter()
                                .filter_map(|shape| match &shape.shape {
                                    egui::Shape::Text(text)
                                        if f64::from(text.pos.y) >= bounds.y0
                                            && f64::from(text.pos.y) < bounds.y1
                                            && f64::from(text.pos.x) >= bounds.x0
                                            && f64::from(text.pos.x) < bounds.x1 =>
                                    {
                                        Some((
                                            text.galley.text(),
                                            egui::Rect::from_min_size(text.pos, text.galley.size()),
                                            shape.clip_rect,
                                            text.galley.elided,
                                        ))
                                    }
                                    _ => None,
                                })
                                .collect::<Vec<_>>();
                            let (_, shortcut_rect, clip, elided) = texts
                                .iter()
                                .find(|(text, ..)| *text == shortcut)
                                .expect("visible custom shortcut");
                            let (_, arrow_rect, _, _) = texts
                                .iter()
                                .find(|(text, ..)| {
                                    *text == egui::containers::menu::SubMenuButton::RIGHT_ARROW
                                })
                                .expect("submenu arrow");
                            assert!(
                                !elided && clip.contains_rect(*shortcut_rect),
                                "shortcut is fully visible"
                            );
                            assert!(
                                shortcut_rect.right() < arrow_rect.left(),
                                "shortcut precedes the separate arrow"
                            );
                            if same {
                                frame(vec![access(parent, Action::Focus)]);
                                frame(vec![egui::Event::Key {
                                    key: egui::Key::ArrowRight,
                                    physical_key: None,
                                    pressed: true,
                                    repeat: false,
                                    modifiers: egui::Modifiers::NONE,
                                }]);
                            } else {
                                frame(vec![access(parent, Action::Click)]);
                            }
                            for _ in 0..4 {
                                frame(vec![]);
                            }
                            let output = frame(vec![]).0;
                            let labels = if command == ToggleImageInterpolation {
                                [Text::StatusSmooth, Text::StatusNearest]
                            } else {
                                [Text::MinificationFast, Text::QualityHigh]
                            };
                            for (value, label) in [false, true].into_iter().zip(labels) {
                                assert_eq!(
                                    node(&output, label.in_language(language)).1.toggled(),
                                    Some(if value == current {
                                        Toggled::True
                                    } else {
                                        Toggled::False
                                    })
                                );
                            }
                            let target = labels[usize::from(if same { current } else { !current })];
                            let chosen = frame(vec![access(
                                node(&output, target.in_language(language)).0,
                                Action::Click,
                            )])
                            .1;
                            assert_eq!(chosen, if same { vec![] } else { vec![command] });
                            assert!(!egui::Popup::is_any_open(&context));
                            assert!(frame(vec![]).1.is_empty(), "no replay");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn image_choice_parents_preserve_unavailable_and_transition_guards() {
    for commands in [
        CommandContext::default(),
        CommandContext {
            media_kind: Some(towavue_core::MediaKind::Audio),
            ..Default::default()
        },
        CommandContext {
            media_kind: Some(towavue_core::MediaKind::Video),
            ..Default::default()
        },
        CommandContext {
            media_kind: Some(towavue_core::MediaKind::Image),
            image_transition: true,
            ..Default::default()
        },
    ] {
        let context = crate::fonts::test_context();
        context.enable_accesskit();
        context.global_style_mut(crate::chrome::style);
        let frame = |events| {
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(660.0, 1100.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.menu_button("Menu", |ui| {
                        assert!(
                            show_section_with_recent(
                                ui,
                                commands,
                                &crate::shortcuts::defaults(),
                                Some(Section::View),
                                &mut MenuData::default()
                            )
                            .is_none()
                        );
                    });
                },
            )
        };
        let output = frame(vec![]);
        frame(vec![access(node(&output, "Menu").0, Action::Click)]);
        for _ in 0..4 {
            frame(vec![]);
        }
        for title in [Text::ImageInterpolation, Text::ImageMinification] {
            let output = frame(vec![]);
            let (id, item) = node(&output, title.in_language(Language::English));
            assert!(item.is_disabled());
            let output = frame(vec![access(id, Action::Click)]);
            assert!(
                !output
                    .platform_output
                    .accesskit_update
                    .expect("tree")
                    .nodes
                    .iter()
                    .any(|(_, node)| node.toggled().is_some()),
                "disabled parent cannot open its choices"
            );
        }
    }
}
