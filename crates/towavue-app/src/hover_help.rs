use egui::{Response, Tooltip, Ui, WidgetText};

pub trait HoverHelp {
    fn help_text(self, text: impl Into<WidgetText>) -> Self;
    fn disabled_help_text(self, text: impl Into<WidgetText>) -> Self;
    fn help_ui(self, content: impl FnOnce(&mut Ui)) -> Self;
}

impl HoverHelp for Response {
    fn help_text(self, text: impl Into<WidgetText>) -> Self {
        self.help_ui(|ui| text_content(ui, text))
    }

    fn disabled_help_text(self, text: impl Into<WidgetText>) -> Self {
        if !self.enabled() {
            show(&self, |ui| text_content(ui, text));
        }
        self
    }

    fn help_ui(self, content: impl FnOnce(&mut Ui)) -> Self {
        if self.enabled() {
            show(&self, content);
        }
        self
    }
}

fn text_content(ui: &mut Ui, text: impl Into<WidgetText>) {
    ui.set_max_width(ui.spacing().tooltip_width);
    ui.label(text);
}

fn show(response: &Response, content: impl FnOnce(&mut Ui)) {
    let open = response.is_tooltip_open();
    let own_layer = egui::LayerId::new(egui::Order::Tooltip, Tooltip::tooltip_id(response.id, 0));
    // Help belongs to the visible source, not an unclipped rect or a path toward the popup.
    // Its own large tooltip may cover the source without ending the hover.
    let source_hovered = response.ctx.pointer_hover_pos().is_some_and(|pointer| {
        let layer = response.ctx.layer_id_at(pointer);
        response.interact_rect.contains(pointer)
            && ((response.contains_pointer() && layer == Some(response.layer_id))
                || (open && layer == Some(own_layer)))
    });
    if !source_hovered {
        if open {
            // egui's previous-frame tooltip owner can otherwise stall the next source.
            response.ctx.request_repaint();
        }
        return;
    }
    if Tooltip::should_show_tooltip(response, false) {
        Tooltip::for_widget(response).show(content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_help_stays_only_while_over_its_source() {
        let context = crate::fonts::test_context();
        let source = egui::Rect::from_min_size(egui::pos2(180.0, 120.0), egui::vec2(60.0, 20.0));
        let mut time = 0.0;
        let mut frame = |seed, pointer| {
            time += 0.1;
            let mut shown = false;
            let mut bounds = None;
            let _ = context.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 300.0),
                    )),
                    events: vec![egui::Event::PointerMoved(pointer)],
                    ..Default::default()
                },
                |ui| {
                    let response = ui.interact(source, "large-help".into(), egui::Sense::hover());
                    let content = |ui: &mut Ui| {
                        shown = true;
                        ui.allocate_space(egui::vec2(300.0, 250.0));
                        ui.label("Large help");
                    };
                    if seed {
                        bounds = Tooltip::for_widget(&response)
                            .show(content)
                            .map(|output| output.response.rect);
                    } else {
                        response.help_ui(content);
                    }
                },
            );
            (shown, bounds)
        };
        frame(true, source.center());
        let bounds = frame(true, source.center())
            .1
            .expect("seeded tooltip bounds");
        assert!(
            bounds.contains(source.center()),
            "fixture must cover its source"
        );
        assert!(frame(false, source.center()).0);
        let outside_source = source.center() + egui::vec2(0.0, 40.0);
        assert!(bounds.contains(outside_source));
        assert!(!frame(false, outside_source).0);
    }

    #[test]
    fn help_lifetime_preserves_delay_disabled_help_and_source_actions() {
        for density in [1.0, 1.25, 2.0] {
            for enabled in [false, true] {
                let context = crate::fonts::test_context();
                context.set_pixels_per_point(density);
                context.global_style_mut(|style| {
                    style.interaction.tooltip_delay = 0.5;
                    style.interaction.show_tooltips_only_when_still = false;
                });
                let source =
                    egui::Rect::from_min_size(egui::pos2(80.0, 80.0), egui::vec2(120.0, 40.0));
                let mut time = 0.0;
                let mut previous_pointer = None;
                let mut frame = |elapsed, pointer, clipped, overlay, click| {
                    time += elapsed;
                    let mut events = Vec::new();
                    if previous_pointer != Some(pointer) {
                        events.push(egui::Event::PointerMoved(pointer));
                        previous_pointer = Some(pointer);
                    }
                    if click {
                        for pressed in [true, false] {
                            events.push(egui::Event::PointerButton {
                                pos: pointer,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            });
                        }
                    }
                    let mut clicked = false;
                    let output = context.run_ui(
                        egui::RawInput {
                            time: Some(time),
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(500.0, 400.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ui| {
                            if let Some(order) = overlay {
                                egui::Area::new("cover".into())
                                    .order(order)
                                    .fixed_pos(source.min)
                                    .show(ui.ctx(), |ui| {
                                        ui.allocate_exact_size(source.size(), egui::Sense::click());
                                    });
                            }
                            ui.add_enabled_ui(enabled, |ui| {
                                if clipped {
                                    ui.set_clip_rect(egui::Rect::from_min_max(
                                        source.min,
                                        egui::pos2(source.right(), source.top() + 5.0),
                                    ));
                                }
                                let response = ui
                                    .interact(source, "source".into(), egui::Sense::click())
                                    .help_text("Enabled help")
                                    .disabled_help_text("Disabled help");
                                clicked = response.clicked();
                            });
                            ui.interact(
                                source.translate(egui::vec2(200.0, 0.0)),
                                "next".into(),
                                egui::Sense::hover(),
                            )
                            .help_text("Next help");
                        },
                    );
                    let labels: Vec<_> = output
                        .shapes
                        .iter()
                        .filter_map(|shape| {
                            if let egui::Shape::Text(text) = &shape.shape {
                                Some(text.galley.text().to_owned())
                            } else {
                                None
                            }
                        })
                        .collect();
                    (labels, clicked)
                };
                let label = if enabled {
                    "Enabled help"
                } else {
                    "Disabled help"
                };
                assert!(
                    frame(0.01, source.center(), false, None, false)
                        .0
                        .is_empty()
                );
                assert!(frame(0.1, source.center(), false, None, false).0.is_empty());
                frame(0.6, source.center(), false, None, false);
                assert_eq!(frame(0.1, source.center(), false, None, false).0, [label]);
                assert!(context.memory(|memory| memory.focused()).is_none());
                assert!(frame(0.01, source.center(), true, None, false).0.is_empty());
                frame(0.01, source.center(), false, None, false);
                assert_eq!(frame(0.01, source.center(), false, None, false).0, [label]);
                let next = source.center() + egui::vec2(200.0, 0.0);
                assert!(
                    frame(0.01, next, false, None, false)
                        .0
                        .iter()
                        .all(|text| text != label)
                );
                frame(0.01, next, false, None, false);
                assert_eq!(frame(0.01, next, false, None, false).0, ["Next help"]);
                assert!(
                    frame(0.01, egui::pos2(10.0, 10.0), false, None, false)
                        .0
                        .is_empty()
                );
                frame(0.6, source.center(), false, None, false);
                frame(0.1, source.center(), false, None, false);
                let (labels, clicked) = frame(0.01, source.center(), false, None, true);
                assert!(labels.is_empty());
                assert_eq!(clicked, enabled);
                for order in [egui::Order::Foreground, egui::Order::Tooltip] {
                    frame(0.6, source.center(), false, None, false);
                    frame(0.6, source.center(), false, None, false);
                    frame(0.01, source.center(), false, Some(order), false);
                    assert!(
                        frame(0.01, source.center(), false, Some(order), false)
                            .0
                            .is_empty()
                    );
                    frame(0.1, egui::pos2(10.0, 10.0), false, None, false);
                }
            }
        }
    }
    #[test]
    fn clipped_source_closes_help() {
        let context = crate::fonts::test_context();
        context.global_style_mut(|style| {
            style.interaction.tooltip_delay = 0.0;
            style.interaction.show_tooltips_only_when_still = false;
        });
        let source = egui::Rect::from_min_size(egui::pos2(80.0, 80.0), egui::vec2(120.0, 40.0));
        let mut time = 0.0;
        let mut frame = |clipped| {
            time += 0.1;
            let mut shown = false;
            let _ = context.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(500.0, 400.0),
                    )),
                    events: vec![egui::Event::PointerMoved(source.center())],
                    ..Default::default()
                },
                |ui| {
                    if clipped {
                        ui.set_clip_rect(egui::Rect::from_min_max(
                            source.min,
                            egui::pos2(source.right(), source.top() + 5.0),
                        ));
                    }
                    let response = ui.interact(source, "help-source".into(), egui::Sense::hover());
                    response.help_ui(|ui| {
                        shown = true;
                        ui.label("Help");
                    });
                },
            );
            shown
        };
        frame(false);
        assert!(frame(false));
        assert!(!frame(true), "help survived outside the source clip");
    }
}
