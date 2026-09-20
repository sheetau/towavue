use crate::{AppEvent, Application, UiAction, chrome};
use towavue_runtime_windows::ProjectLink;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Action {
    Close,
    Licenses,
    Link(ProjectLink),
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn draw_about(&self, context: &egui::Context, actions: &mut Vec<UiAction>) {
        let modal = chrome::modal(context, "about-towavue".into(), false).show(context, |ui| {
            chrome::modal_body_with_header(ui, 360.0, "About towavue", &["OK"], identity, |ui| {
                ui.label("Media viewer for Windows");
                ui.horizontal(|ui| {
                    ui.label("Creator:");
                    if ui.link("sheeta").clicked() {
                        actions.push(UiAction::About(Action::Link(ProjectLink::Author)));
                    }
                });
                if ui.link("GitHub").clicked() {
                    actions.push(UiAction::About(Action::Link(ProjectLink::Repository)));
                }
                ui.add_space(8.0);
                ui.label("MIT OR Apache-2.0. Provided without warranty.");
                if ui.link("Licenses and sources").clicked() {
                    actions.push(UiAction::About(Action::Licenses));
                }
            });
            chrome::flat_buttons(ui);
            if ui.button("OK").clicked() {
                actions.push(UiAction::About(Action::Close));
            }
        });
        if modal.is_top_modal
            && !modal.any_popup_open
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            actions.push(UiAction::About(Action::Close));
        }
    }

    pub(super) fn handle_about(&mut self, action: Action) {
        match action {
            Action::Close => self.about_open = false,
            Action::Licenses => {
                self.about_open = false;
                self.show_licenses();
            }
            Action::Link(link) => {
                if let Err(error) = link.open() {
                    self.set_status(format!("Could not open link: {error}"));
                }
            }
        }
        self.request_redraw();
    }
}

fn identity(ui: &mut egui::Ui) -> egui::Response {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
        chrome::paint_logo(ui.painter(), rect, None, 0.0, true);
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!("towavue / Version {}", env!("CARGO_PKG_VERSION")))
                    .size(18.0)
                    .color(chrome::FOREGROUND),
            )
            .wrap_mode(egui::TextWrapMode::Truncate),
        );
    })
    .response
}

#[cfg(test)]
mod tests {
    use super::*;
    use towavue_core::CommandId;

    #[test]
    fn about_modal_preserves_document_and_dismisses_at_supported_densities() {
        let Some(_root) = crate::tests::isolated_test_root(
            "about::tests::about_modal_preserves_document_and_dismisses_at_supported_densities",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("app");
        for density in [1.0, 1.25, 2.0] {
            for size in [egui::vec2(960.0, 576.0), egui::vec2(320.0, 220.0)] {
                let context = crate::fonts::test_context();
                context.global_style_mut(chrome::style);
                context.set_pixels_per_point(density);
                app.ui_context = Some(context.clone());
                app.dispatch(CommandId::About);
                assert!(app.about_open && app.modal_input_blocked());
                let original_tab = app.tabs.active_id();
                app.dispatch(CommandId::OpenGallery);
                assert_eq!(app.tabs.active_id(), original_tab);
                for pass in 0..4 {
                    let mut actions = Vec::new();
                    let output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                            events: if pass == 3 {
                                vec![egui::Event::Key {
                                    key: egui::Key::Escape,
                                    physical_key: None,
                                    pressed: true,
                                    repeat: false,
                                    modifiers: egui::Modifiers::NONE,
                                }]
                            } else {
                                Vec::new()
                            },
                            ..Default::default()
                        },
                        |_| app.draw_about(&context, &mut actions),
                    );
                    assert!(!output.shapes.is_empty());
                    let texts: Vec<_> = output
                        .shapes
                        .iter()
                        .filter_map(|shape| {
                            if let egui::Shape::Text(text) = &shape.shape {
                                Some(text)
                            } else {
                                None
                            }
                        })
                        .collect();
                    assert!(
                        texts
                            .iter()
                            .all(|text| text.galley.text() != "About towavue")
                    );
                    if pass == 2 {
                        let identity = texts
                            .iter()
                            .find(|text| text.galley.text().starts_with("towavue / Version "))
                            .expect("single identity line");
                        assert_eq!(identity.galley.rows.len(), 1);
                        assert_eq!(
                            identity.galley.job.sections[0].format.color,
                            chrome::FOREGROUND
                        );
                    }
                    for action in actions {
                        app.handle_ui_action(action);
                    }
                }
                assert!(!app.about_open && !app.modal_input_blocked());
                assert_eq!(app.tabs.active_id(), original_tab);
            }
        }
    }
}
