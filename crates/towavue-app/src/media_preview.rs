use egui::{Align2, InnerResponse, Response, Ui};

pub struct Preview {
    response: Response,
    seek: Option<f32>,
}

#[derive(Clone)]
struct TabHover {
    source: egui::Id,
    source_rect: egui::Rect,
    card: egui::Rect,
    frame: u64,
}

fn tab_hover_id() -> egui::Id {
    egui::Id::new("open-tab-preview")
}

pub fn tab_hovered(response: &Response) -> bool {
    let context = &response.ctx;
    if !response.enabled()
        || egui::Popup::is_any_open(context)
        || context.input(|input| !input.raw.hovered_files.is_empty())
    {
        return false;
    }
    if hover_pos(response).is_some() && !context.input(|input| input.pointer.any_down()) {
        return true;
    }
    let Some(pointer) = context.pointer_hover_pos() else {
        return false;
    };
    let Some(open) = context.data(|data| data.get_temp::<TabHover>(tab_hover_id())) else {
        return false;
    };
    if open.source != response.id
        || open.source_rect != response.interact_rect
        || open.frame.saturating_add(1) < context.cumulative_frame_nr()
    {
        return false;
    }
    let layer = egui::LayerId::new(egui::Order::Tooltip, response.id.with("media-preview"));
    if open.card.contains(pointer) {
        return context.layer_id_at(pointer) == Some(layer);
    }
    let bridge = egui::Rect::from_min_max(
        egui::pos2(
            open.card.left().max(response.rect.left()),
            response.rect.bottom(),
        ),
        egui::pos2(
            open.card.right().min(response.rect.right()),
            open.card.top(),
        ),
    );
    bridge.contains(pointer)
        && !context.input(|input| input.pointer.any_down())
        && context
            .layer_id_at(pointer)
            .is_none_or(|top| top == layer || top == response.layer_id)
}

pub fn caption<R>(ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::NONE.inner_margin(6).show(ui, content).inner
}

pub fn image(
    ui: &Ui,
    texture: egui::TextureId,
    rect: egui::Rect,
    uv: egui::Rect,
    bounds: egui::Rect,
) {
    let radius = ui.visuals().menu_corner_radius;
    let corners = egui::CornerRadius {
        nw: radius.nw,
        ne: radius.ne,
        sw: 0,
        se: 0,
    };
    let left = egui::Rect::from_min_size(bounds.min, egui::Vec2::splat(f32::from(corners.nw)));
    let right = egui::Rect::from_min_max(
        bounds.right_top() - egui::vec2(f32::from(corners.ne), 0.0),
        bounds.right_top() + egui::vec2(0.0, f32::from(corners.ne)),
    );
    if !rect.intersects(left) && !rect.intersects(right) {
        ui.painter().image(texture, rect, uv, egui::Color32::WHITE);
        return;
    }
    // Clip the card's rounded silhouette to each fitted image/page, including
    // letterboxing narrower than the corner radius. No composite texture is needed.
    let map_uv = |point: egui::Pos2| {
        egui::pos2(
            egui::remap(point.x, rect.x_range(), uv.x_range()),
            egui::remap(point.y, rect.y_range(), uv.y_range()),
        )
    };
    let shape = egui::epaint::RectShape::filled(bounds, corners, egui::Color32::WHITE)
        .with_texture(
            texture,
            egui::Rect::from_min_max(map_uv(bounds.min), map_uv(bounds.max)),
        );
    for primitive in ui.ctx().tessellate(
        vec![egui::epaint::ClippedShape {
            clip_rect: ui.clip_rect(),
            shape: shape.into(),
        }],
        ui.ctx().pixels_per_point(),
    ) {
        if let egui::epaint::Primitive::Mesh(mut mesh) = primitive.primitive {
            for vertex in &mut mesh.vertices {
                vertex.pos = rect.clamp(vertex.pos);
                // Antialiasing extrapolates UVs; never sample an adjacent sheet cell.
                vertex.uv = uv.clamp(map_uv(vertex.pos));
            }
            ui.painter().with_clip_rect(rect).add(mesh);
        }
    }
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
                || if self.seek.is_none() {
                    tab_hovered(&response)
                } else {
                    hover_pos(&response).is_some()
                        && !context.input(|input| input.pointer.any_down())
                })
        {
            if self.seek.is_none() {
                context.data_mut(|data| {
                    if data
                        .get_temp::<TabHover>(tab_hover_id())
                        .is_some_and(|open| open.source == response.id)
                    {
                        data.remove::<TabHover>(tab_hover_id());
                    }
                });
            }
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
            .interactable(self.seek.is_none())
            .movable(false)
            .fade_in(false)
            .pivot(pivot)
            .fixed_pos(anchor)
            .default_width(width)
            .show(context, |ui| {
                ui.style_mut().interaction.selectable_labels = false;
                egui::Frame::popup(ui.style())
                    .inner_margin(0)
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.with_layout(egui::Layout::top_down(egui::Align::Center), content)
                            .inner
                    })
                    .inner
            });
        if self.seek.is_none() {
            let frame = context.cumulative_frame_nr();
            context.data_mut(|data| {
                data.insert_temp(
                    tab_hover_id(),
                    TabHover {
                        source: response.id,
                        source_rect: response.interact_rect,
                        card: output.response.rect,
                        frame,
                    },
                )
            });
        }
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
    fn fitted_images_and_pages_keep_geometry_and_uv_mapping_near_card_corners() {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            let bounds =
                egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(240.0, 160.0));
            let uv = egui::Rect::from_min_max(egui::pos2(0.25, 0.5), egui::pos2(0.5, 0.75));
            for rect in [
                bounds.shrink(2.0),
                egui::Rect::from_min_size(bounds.min, egui::vec2(80.0, 160.0)),
                egui::Rect::from_min_size(
                    bounds.min + egui::vec2(80.0, 0.0),
                    egui::vec2(160.0, 160.0),
                ),
            ] {
                let mut radius = 0.0;
                let output = context.run_ui(egui::RawInput::default(), |ui| {
                    radius = f32::from(ui.visuals().menu_corner_radius.nw);
                    image(ui, egui::TextureId::User(42), rect, uv, bounds);
                });
                let mesh = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Mesh(mesh) if mesh.texture_id == egui::TextureId::User(42) => {
                            Some(mesh)
                        }
                        _ => None,
                    })
                    .expect("fitted mesh");
                assert_eq!(mesh.calc_bounds(), rect);
                for vertex in &mesh.vertices {
                    assert!(rect.contains(vertex.pos));
                    assert!(uv.contains(vertex.uv));
                    let expected = egui::pos2(
                        egui::remap(vertex.pos.x, rect.x_range(), uv.x_range()),
                        egui::remap(vertex.pos.y, rect.y_range(), uv.y_range()),
                    );
                    assert!(
                        vertex.uv.distance(expected) < 0.000001,
                        "fitting must not stretch a sheet cell"
                    );
                    let center = bounds.min + egui::Vec2::splat(radius);
                    if vertex.color.a() > 0 && vertex.pos.x < center.x && vertex.pos.y < center.y {
                        assert!(
                            vertex.pos.distance(center) <= radius,
                            "image must stay inside the card corner"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn cards_have_tooltip_borders_flush_images_padded_captions_and_bounded_sheet_uvs() {
        for density in [1.0, 1.25, 2.0] {
            for seek in [false, true] {
                let context = crate::fonts::test_context();
                context.set_pixels_per_point(density);
                context.global_style_mut(crate::chrome::style);
                let texture = context.load_texture(
                    "preview-sheet",
                    egui::ColorImage::filled([8, 8], egui::Color32::WHITE),
                    egui::TextureOptions::LINEAR,
                );
                let uv = egui::Rect::from_min_max(egui::pos2(0.25, 0.25), egui::pos2(0.5, 0.5));
                let source =
                    egui::Rect::from_min_size(egui::pos2(200.0, 250.0), egui::vec2(100.0, 20.0));
                let mut card = egui::Rect::NOTHING;
                let mut pixels = egui::Rect::NOTHING;
                let mut label = egui::Rect::NOTHING;
                let mut output = egui::FullOutput::default();
                for frame in 0..3 {
                    output = context.run_ui(
                        egui::RawInput {
                            time: Some(f64::from(frame)),
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(600.0, 500.0),
                            )),
                            events: vec![egui::Event::PointerMoved(source.center())],
                            ..Default::default()
                        },
                        |ui| {
                            let response =
                                ui.interact(source, "source".into(), egui::Sense::hover());
                            let preview = if seek {
                                Preview::seek(&response, 0.5)
                            } else {
                                Preview::tab(&response)
                            };
                            card = preview
                                .show(|ui| {
                                    let size = if seek {
                                        egui::vec2(160.0, 108.0)
                                    } else {
                                        egui::vec2(240.0, 160.0)
                                    };
                                    pixels = ui.allocate_exact_size(size, egui::Sense::hover()).0;
                                    image(ui, texture.id(), pixels, uv, pixels);
                                    caption(ui, |ui| {
                                        label = ui.label("Caption").rect;
                                    });
                                })
                                .expect("preview")
                                .response
                                .rect;
                        },
                    );
                }
                let tolerance = 1.0 / density;
                let border = context.global_style().visuals.window_stroke();
                assert!((pixels.top() - card.top() - border.width).abs() <= tolerance);
                assert!((pixels.left() - card.left() - border.width).abs() <= tolerance);
                assert!((card.right() - pixels.right() - border.width).abs() <= tolerance);
                assert!(
                    output
                        .shapes
                        .iter()
                        .flat_map(|shape| match &shape.shape {
                            egui::Shape::Vec(shapes) => shapes.as_slice(),
                            shape => std::slice::from_ref(shape),
                        })
                        .any(|shape| matches!(shape, egui::Shape::Rect(rect)
                    if rect.rect == card && rect.stroke == border)),
                    "use the same border as ordinary tooltips"
                );
                assert!(label.top() >= pixels.bottom() + 5.0);
                assert!(label.left() >= card.left() + 5.0);
                assert!(card.bottom() >= label.bottom() + 5.0);
                let meshes: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Mesh(mesh) if mesh.texture_id == texture.id() => Some(mesh),
                        _ => None,
                    })
                    .collect();
                assert_eq!(meshes.len(), 1);
                let mesh = meshes[0];
                assert!(mesh.vertices.len() > 4, "rounded outline, not a plain quad");
                assert!(mesh.vertices.iter().all(|vertex| uv.contains(vertex.uv)));
                assert!(
                    mesh.vertices
                        .iter()
                        .any(|vertex| vertex.color == egui::Color32::TRANSPARENT)
                );
                assert!(
                    mesh.vertices
                        .iter()
                        .filter(|vertex| vertex.color.a() > 0)
                        .all(|vertex| {
                            vertex.pos.distance(pixels.left_top()) > 1.0
                                && vertex.pos.distance(pixels.right_top()) > 1.0
                        })
                );
                assert!(
                    context.tex_manager().write().take_delta().set.len() <= 2,
                    "rounding must not upload a cropped/composite image"
                );
            }
        }
    }

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
