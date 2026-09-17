use super::*;

pub fn tab_title(ui: &mut Ui, rect: Rect, label: &str, active: bool) -> egui::Response {
    let response = ui.put(
        rect,
        egui::Button::new("")
            .min_size(rect.size())
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .sense(egui::Sense::click_and_drag()),
    );
    if ui.is_rect_visible(rect) {
        let text = egui::RichText::new(label);
        let text = if active { text.color(FOREGROUND) } else { text };
        let galley = egui::WidgetText::from(text).into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Button,
        );
        let origin = egui::pos2(
            rect.left() + ui.spacing().button_padding.x,
            rect.center().y - galley.size().y / 2.0,
        );
        ui.painter().with_clip_rect(rect).galley(
            origin,
            galley,
            ui.style().interact(&response).fg_stroke.color,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), label)
    });
    response
}

pub fn tab_title_fade(ui: &Ui, rect: Rect, active: bool, hovered: bool) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    // Match Monapad's name-wrap mask: unchanged through 80%, transparent at
    // the close-button boundary. Paint only over the label on its actual tab
    // background, leaving the leading audio slot and trailing controls intact.
    let background = if hovered {
        HOVER
    } else if active {
        BORDER
    } else {
        BACKGROUND
    };
    let left = rect.left() + ui.spacing().button_padding.x;
    let start = egui::lerp(left..=rect.right(), 0.8);
    ui.painter()
        .with_clip_rect(rect)
        .add(egui::Shape::gradient_rect(
            Rect::from_min_max(egui::pos2(start, rect.top()), rect.right_bottom()),
            egui::Direction::LeftToRight,
            [Color32::TRANSPARENT, background],
        ));
}

pub fn tab_strip_fades(ui: &Ui, rect: Rect, content_width: f32, offset: f32) {
    let width = 20.0_f32.min(rect.width() / 2.0);
    if width <= 0.0 {
        return;
    }
    let pixel = (1.0 / ui.ctx().pixels_per_point()).min(width);
    let painter = ui.painter().with_clip_rect(rect);
    for (needed, left) in [
        (offset > 0.0, true),
        (content_width - rect.width() - offset > 0.0, false),
    ] {
        if !needed {
            continue;
        }
        let (solid, fade, direction) = if left {
            (
                Rect::from_min_max(rect.min, egui::pos2(rect.left() + pixel, rect.bottom())),
                Rect::from_min_max(
                    egui::pos2(rect.left() + pixel, rect.top()),
                    egui::pos2(rect.left() + width, rect.bottom()),
                ),
                egui::Direction::LeftToRight,
            )
        } else {
            (
                Rect::from_min_max(egui::pos2(rect.right() - pixel, rect.top()), rect.max),
                Rect::from_min_max(
                    egui::pos2(rect.right() - width, rect.top()),
                    egui::pos2(rect.right() - pixel, rect.bottom()),
                ),
                egui::Direction::RightToLeft,
            )
        };
        painter.rect_filled(solid, 0.0, BACKGROUND);
        painter.add(egui::Shape::gradient_rect(
            fade,
            direction,
            [BACKGROUND, Color32::TRANSPARENT],
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_keep_full_text_and_fade_the_final_fifth_without_touching_controls() {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            context.global_style_mut(super::super::style);
            for width in [84.0, 150.0, 220.0] {
                for audio in [false, true] {
                    for (active, hovered) in
                        [(false, false), (true, false), (true, true), (false, true)]
                    {
                        let left = 10.0
                            + if audio {
                                super::super::TAB_AUDIO_WIDTH
                            } else {
                                0.0
                            };
                        let rect = Rect::from_min_max(
                            egui::pos2(left, 10.0),
                            egui::pos2(10.0 + width - TAB_CLOSE_WIDTH, 34.0),
                        );
                        let label = "A long tab filename that must keep its complete text.png";
                        let output = context.run_ui(Default::default(), |ui| {
                            ui.spacing_mut().button_padding =
                                egui::vec2(if audio { 0.0 } else { TAB_PADDING }, 0.0);
                            let response = tab_title(ui, rect, label, active);
                            assert_eq!(response.rect, rect);
                            tab_title_fade(ui, rect, active, hovered);
                        });
                        let text = output
                            .shapes
                            .iter()
                            .find_map(|shape| match &shape.shape {
                                egui::Shape::Text(text) if text.galley.text() == label => {
                                    Some((text, shape.clip_rect))
                                }
                                _ => None,
                            })
                            .expect("full title text");
                        assert!(!text.0.galley.elided);
                        assert_eq!(text.1, rect);
                        let text_left = left + if audio { 0.0 } else { TAB_PADDING };
                        assert_eq!(text.0.pos.x, text_left);
                        let fade = output
                            .shapes
                            .iter()
                            .find_map(|shape| match &shape.shape {
                                egui::Shape::Mesh(mesh) => Some(mesh),
                                _ => None,
                            })
                            .expect("title fade");
                        let bounds = fade.calc_bounds();
                        assert!(
                            (bounds.left() - egui::lerp(text_left..=rect.right(), 0.8)).abs()
                                < 0.01
                        );
                        assert_eq!(bounds.right(), rect.right());
                        let background = if hovered {
                            HOVER
                        } else if active {
                            BORDER
                        } else {
                            BACKGROUND
                        };
                        for vertex in &fade.vertices {
                            assert_eq!(
                                vertex.color,
                                if vertex.pos.x == bounds.left() {
                                    Color32::TRANSPARENT
                                } else {
                                    background
                                }
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn clipped_tabs_keep_their_accessible_name_without_painting_text_or_fades() {
        let context = crate::fonts::test_context();
        context.enable_accesskit();
        let label = "An offscreen media tab.png";
        let output = context.run_ui(Default::default(), |ui| {
            ui.set_clip_rect(Rect::from_min_size(Pos2::ZERO, egui::vec2(200.0, 32.0)));
            let rect = Rect::from_min_size(egui::pos2(300.0, 4.0), egui::vec2(120.0, 24.0));
            let response = tab_title(ui, rect, label, true);
            assert_eq!(response.rect, rect);
            tab_title_fade(ui, rect, true, false);
        });
        assert!(
            !output
                .shapes
                .iter()
                .any(|shape| matches!(shape.shape, egui::Shape::Text(_) | egui::Shape::Mesh(_)))
        );
        assert!(
            output
                .platform_output
                .accesskit_update
                .expect("accessibility tree")
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some(label))
        );
    }

    #[test]
    fn strip_edges_reach_opaque_black_only_where_content_is_clipped() {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            let rect = Rect::from_min_size(egui::pos2(40.0, 10.0), egui::vec2(200.0, 24.0));
            for (content, offset, edges) in [
                (100.0, 0.0, 0),
                (400.0, 0.0, 1),
                (400.0, 0.25, 2),
                (400.0, 100.0, 2),
                (400.0, 200.0, 1),
            ] {
                let output = context.run_ui(Default::default(), |ui| {
                    tab_strip_fades(ui, rect, content, offset)
                });
                let fades: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Mesh(mesh) => Some(mesh),
                        _ => None,
                    })
                    .collect();
                assert_eq!(fades.len(), edges);
                for fade in fades {
                    assert!(rect.contains_rect(fade.calc_bounds()));
                    assert!(fade.vertices.iter().any(|v| v.color == BACKGROUND));
                    assert!(
                        fade.vertices
                            .iter()
                            .any(|v| v.color == Color32::TRANSPARENT)
                    );
                }
                let solids: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Rect(solid) if solid.fill == BACKGROUND => Some(solid),
                        _ => None,
                    })
                    .collect();
                assert_eq!(solids.len(), edges);
                for solid in solids {
                    assert!((solid.rect.width() * density - 1.0).abs() < 0.001);
                    assert!(solid.rect.left() == rect.left() || solid.rect.right() == rect.right());
                }
            }
        }
    }
}
