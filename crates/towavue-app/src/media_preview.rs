use egui::{Align2, InnerResponse, Response, Ui};

pub struct Preview {
    response: Response,
    seek: Option<f32>,
}

pub fn hover_pos(response: &Response) -> Option<egui::Pos2> {
    // The drop overlay owns this hover even though it is painted without hit testing.
    if response
        .ctx
        .input(|input| !input.raw.hovered_files.is_empty())
    {
        return None;
    }
    response.ctx.pointer_hover_pos().filter(|&pointer| {
        response.enabled()
            && response.interact_rect.contains(pointer)
            && response.ctx.layer_id_at(pointer).is_none_or(|layer| {
                layer == response.layer_id || layer.order == egui::Order::Tooltip
            })
    })
}

impl Preview {
    pub fn seek(response: &Response, ratio: f32) -> Self {
        Self {
            response: response.clone(),
            seek: Some(ratio),
        }
    }

    pub fn tab(response: &Response) -> Self {
        Self {
            response: response.clone(),
            seek: None,
        }
    }

    pub fn show<R>(self, content: impl FnOnce(&mut Ui) -> R) -> Option<InnerResponse<R>> {
        let response = self.response;
        let context = &response.ctx;
        let dragging = self.seek.is_some() && crate::timeline_input::is_dragging(&response);
        if !response.enabled()
            || egui::Popup::is_any_open(context)
            || context.input(|input| !input.raw.hovered_files.is_empty())
            || !(dragging
                || (hover_pos(&response).is_some()
                    && !context.input(|input| input.pointer.any_down())))
        {
            return None;
        }
        let (anchor, pivot, width) = if let Some(ratio) = self.seek {
            (
                egui::pos2(
                    egui::lerp(response.rect.x_range(), ratio.clamp(0.0, 1.0)),
                    response.rect.top() - 4.0,
                ),
                Align2::CENTER_BOTTOM,
                160.0,
            )
        } else {
            (
                response.rect.center_bottom() + egui::vec2(0.0, 4.0),
                Align2::CENTER_TOP,
                240.0,
            )
        };
        let id = response.id.with("media-preview");
        let previous = egui::AreaState::load(context, id).and_then(|state| state.size);
        let output = egui::Area::new(id)
            .order(egui::Order::Tooltip)
            .interactable(false)
            .movable(false)
            .fade_in(false)
            .pivot(pivot)
            .fixed_pos(anchor)
            .default_width(width)
            .show(context, |ui| {
                ui.style_mut().interaction.selectable_labels = false;
                egui::Frame::popup(ui.style())
                    .show(ui, |ui| {
                        ui.set_max_width(width);
                        ui.with_layout(egui::Layout::top_down(egui::Align::Center), content)
                            .inner
                    })
                    .inner
            });
        // Resolve initial sizing and asynchronous content changes before presenting the frame.
        if previous.is_none_or(|size| (size - output.response.rect.size()).length() > 0.1) {
            context.request_discard("media preview size changed");
        }
        Some(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_file_drag_suppresses_hover_content_until_it_leaves() {
        use crate::hover_help::HoverHelp;

        for focused in [true, false] {
            for mode in 0..3 {
                let context = crate::fonts::test_context();
                context.global_style_mut(|style| {
                    style.interaction.tooltip_delay = 0.0;
                    style.interaction.show_tooltips_only_when_still = false;
                });
                let source =
                    egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(120.0, 30.0));
                let mut time = 0.0;
                let mut frame = |external_drag| {
                    time += 0.1;
                    let mut hovered = false;
                    let output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(500.0, 400.0),
                            )),
                            time: Some(time),
                            focused,
                            events: vec![egui::Event::PointerMoved(source.center())],
                            hovered_files: if external_drag {
                                vec![egui::HoveredFile {
                                    mime: "image/png".into(),
                                    ..Default::default()
                                }]
                            } else {
                                Vec::new()
                            },
                            ..Default::default()
                        },
                        |ui| {
                            let response =
                                ui.interact(source, "hover-source".into(), egui::Sense::hover());
                            hovered = hover_pos(&response).is_some();
                            match mode {
                                0 => {
                                    Preview::tab(&response).show(|ui| ui.label("Hover content"));
                                }
                                1 => {
                                    Preview::seek(&response, 0.5)
                                        .show(|ui| ui.label("Hover content"));
                                }
                                _ => {
                                    response.help_text("Hover content");
                                }
                            }
                        },
                    );
                    let shown = output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "Hover content"));
                    (hovered, shown)
                };
                frame(false);
                frame(false);
                assert!(
                    frame(false).1,
                    "seed hover content: {mode}, focused={focused}"
                );
                let (hovered, shown) = frame(true);
                assert!(
                    !hovered,
                    "external drag must not request background previews"
                );
                assert!(!shown, "external drag must hide content: {mode}");
                assert!(!frame(true).1, "no stale content while drag remains");
                frame(false);
                assert!(frame(false).1, "hover resumes after external drag leaves");
                assert!(context.memory(egui::Memory::focused).is_none());
            }
        }
    }

    #[test]
    fn previews_replace_previous_tooltips_immediately_and_reanchor_size_changes() {
        for density in [1.0, 1.25, 2.0] {
            for seek in [false, true] {
                for focused in [false, true] {
                    let context = crate::fonts::test_context();
                    context.set_pixels_per_point(density);
                    context.global_style_mut(|style| {
                        style.interaction.tooltip_delay = 60.0;
                        style.interaction.show_tooltips_only_when_still = true;
                    });
                    let source = egui::Rect::from_min_size(
                        egui::pos2(200.0, 200.0),
                        egui::vec2(100.0, 20.0),
                    );
                    let mut time = 0.0;
                    let mut frame = |phase, height, enabled| {
                        time += 0.001;
                        let mut bounds = None;
                        let output = context.run_ui(
                            egui::RawInput {
                                time: Some(time),
                                focused,
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(600.0, 500.0),
                                )),
                                events: vec![egui::Event::PointerMoved(
                                    if phase == 1 || phase == 3 {
                                        source.center()
                                    } else {
                                        egui::pos2(20.0, 20.0)
                                    },
                                )],
                                ..Default::default()
                            },
                            |ui| {
                                let other = ui.interact(
                                    egui::Rect::from_min_size(
                                        egui::Pos2::ZERO,
                                        egui::vec2(60.0, 40.0),
                                    ),
                                    "other".into(),
                                    egui::Sense::hover(),
                                );
                                if phase == 0 {
                                    egui::Tooltip::for_widget(&other).show(|ui| {
                                        ui.allocate_space(egui::vec2(300.0, 230.0));
                                        ui.label("Previous tooltip");
                                    });
                                }
                                if phase == 3 {
                                    egui::Area::new("blocking-overlay".into())
                                        .order(egui::Order::Foreground)
                                        .fixed_pos(source.min)
                                        .show(ui.ctx(), |ui| {
                                            ui.allocate_exact_size(
                                                source.size(),
                                                egui::Sense::click(),
                                            );
                                        });
                                }
                                ui.add_enabled_ui(enabled, |ui| {
                                    let response = ui.interact(
                                        source,
                                        "preview-source".into(),
                                        egui::Sense::hover(),
                                    );
                                    let preview = if seek {
                                        Preview::seek(&response, 0.5)
                                    } else {
                                        Preview::tab(&response)
                                    };
                                    bounds = preview
                                        .show(|ui| {
                                            ui.allocate_space(egui::vec2(100.0, height));
                                            ui.label("Immediate preview");
                                        })
                                        .map(|output| output.response.rect);
                                });
                            },
                        );
                        (bounds, output)
                    };
                    frame(0, 0.0, true);
                    frame(0, 0.0, true);
                    for height in [0.0, 108.0, 20.0, 160.0] {
                        let (bounds, output) = frame(1, height, true);
                        let bounds = bounds.unwrap_or_else(|| panic!("no preview: seek={seek}, focused={focused}, density={density}, height={height}, pointer={:?}", context.pointer_hover_pos()));
                        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "Immediate preview")));
                        assert!((bounds.center().x - source.center().x).abs() <= 1.0 / density);
                        if seek {
                            assert!(
                                (bounds.bottom() - source.top() + 4.0).abs() <= 1.0 / density,
                                "{bounds:?}"
                            );
                        } else {
                            assert!(
                                (bounds.top() - source.bottom() - 4.0).abs() <= 1.0 / density,
                                "{bounds:?}"
                            );
                        }
                        assert!(context.memory(egui::Memory::focused).is_none());
                    }
                    assert!(frame(1, 108.0, false).0.is_none());
                    egui::Popup::open_id(&context, "blocking-popup".into());
                    assert!(frame(1, 108.0, true).0.is_none());
                    egui::Popup::close_id(&context, "blocking-popup".into());
                    frame(3, 108.0, true);
                    assert!(frame(3, 108.0, true).0.is_none());
                    let (bounds, output) = frame(2, 108.0, true);
                    assert!(bounds.is_none());
                    assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "Immediate preview")));
                }
            }
        }
    }
}
