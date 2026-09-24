use super::*;
use crate::localization::{Language, Settings};
use egui::accesskit::{Action, ActionRequest, NodeId, TreeId};

fn node<'a>(output: &'a egui::FullOutput, label: &str) -> (NodeId, &'a egui::accesskit::Node) {
    let (id, node) = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree")
        .nodes
        .iter()
        .find(|(_, n)| {
            n.label()
                .is_some_and(|name| name.trim_end_matches('\u{23f5}').trim() == label)
        })
        .unwrap_or_else(|| panic!("missing {label}"));
    (*id, node)
}
fn access(id: NodeId, action: Action) -> egui::Event {
    egui::Event::AccessKitActionRequest(ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: id,
        data: None,
    })
}
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn language_menu_selects_with_pointer_and_keyboard_at_supported_densities() {
    for density in [1.0, 1.25, 2.0] {
        for display in [Language::English, Language::Japanese] {
            for compact in [false, true] {
                for pointer in [false, true] {
                    let context = crate::localization::test_ui::japanese_context(density);
                    crate::localization::set_language(&context, display);
                    let size = if compact {
                        egui::vec2(320.0, 240.0)
                    } else {
                        egui::vec2(480.0, 2400.0)
                    };
                    let saving = std::cell::Cell::new(false);
                    let frame = |events| {
                        let mut data = MenuData {
                            language: Settings {
                                display,
                                next: Language::English,
                                saving: saving.get(),
                            },
                            ..Default::default()
                        };
                        let output = context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    size,
                                )),
                                focused: true,
                                events,
                                ..Default::default()
                            },
                            |ui| {
                                ui.menu_button("Fixture", |ui| {
                                    if compact {
                                        language_menu(ui, None, &mut data);
                                    } else {
                                        assert!(
                                            show_section_with_recent(
                                                ui,
                                                CommandContext::default(),
                                                &ShortcutBindings::default(),
                                                Some(Section::View),
                                                &mut data
                                            )
                                            .is_none()
                                        );
                                    }
                                });
                            },
                        );
                        (output, data.language_action)
                    };
                    let mut output = frame(vec![]).0;
                    for label in ["Fixture", Text::DisplayLanguage.in_language(display)] {
                        if !pointer && label != "Fixture" && !compact {
                            frame(vec![access(node(&output, label).0, Action::Focus)]);
                            frame(vec![key(egui::Key::ArrowRight)]);
                        } else {
                            frame(vec![access(node(&output, label).0, Action::Click)]);
                        }
                        for _ in 0..4 {
                            output = frame(vec![]).0;
                        }
                    }
                    let (_, japanese) = node(&output, Text::LanguageJapanese.in_language(display));
                    assert!(!japanese.is_disabled());
                    assert_eq!(japanese.toggled(), Some(egui::accesskit::Toggled::False));
                    assert_eq!(
                        node(&output, "English").1.toggled(),
                        Some(egui::accesskit::Toggled::True)
                    );
                    let bounds = japanese.bounds().expect("bounds");
                    assert!(
                        bounds.x0 >= 0.0
                            && bounds.x1 <= f64::from(size.x)
                            && bounds.y0 >= 0.0
                            && bounds.y1 <= f64::from(size.y),
                        "{bounds:?} at {size:?}"
                    );
                    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == Text::LanguageJapanese.in_language(display) && !text.galley.elided)));
                    let selected = if pointer {
                        let pos = egui::pos2(
                            ((bounds.x0 + bounds.x1) / 2.0) as f32,
                            ((bounds.y0 + bounds.y1) / 2.0) as f32,
                        );
                        let button = |pressed| egui::Event::PointerButton {
                            pos,
                            pressed,
                            button: egui::PointerButton::Primary,
                            modifiers: egui::Modifiers::NONE,
                        };
                        frame(vec![egui::Event::PointerMoved(pos), button(true)]);
                        frame(vec![button(false)]).1
                    } else {
                        frame(vec![access(node(&output, "English").0, Action::Focus)]);
                        frame(vec![key(egui::Key::ArrowDown)]);
                        frame(vec![key(egui::Key::Enter)]).1
                    };
                    assert_eq!(selected, Some(Language::Japanese));
                    assert_eq!(
                        crate::localization::language(&context),
                        display,
                        "choice must not change live text"
                    );
                    saving.set(true);
                    let output = frame(vec![]).0;
                    frame(vec![access(node(&output, "Fixture").0, Action::Click)]);
                    for _ in 0..4 {
                        frame(vec![]);
                    }
                    let output = frame(vec![]).0;
                    assert!(
                        node(&output, Text::DisplayLanguage.in_language(display))
                            .1
                            .is_disabled()
                    );
                }
            }
        }
    }
}
