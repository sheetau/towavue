use egui::{Color32, Context, Rect, pos2, vec2};

pub(super) fn draw(context: &Context, blocked: bool) {
    if !context.input(|input| !input.raw.hovered_files.is_empty()) {
        return;
    }
    let viewport = context.content_rect();
    let painter = context
        .layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("external-file-drop"),
        ))
        .with_clip_rect(viewport);
    painter.rect_filled(viewport, 0.0, Color32::from_black_alpha(204));
    let size = (viewport.size() - vec2(32.0, 32.0))
        .max(vec2(0.0, 0.0))
        .min(vec2(344.0, 200.0));
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    let card = Rect::from_center_size(viewport.center(), size);
    // Paint only: the centered guide never narrows the native whole-window drop target.
    dashed_border(&painter, card, context.pixels_per_point());
    let painter = painter.with_clip_rect(card.shrink(8.0));
    let label = if blocked {
        "Close the dialog before dropping files"
    } else {
        "Open with towavue"
    };
    let text = painter.layout(
        label.into(),
        egui::FontId::proportional(16.0),
        Color32::WHITE,
        (card.width() - 32.0).max(1.0),
    );
    let logo_size = (card.height() - text.size().y - 56.0).clamp(0.0, 48.0);
    let gap = if logo_size > 0.0 { 24.0 } else { 0.0 };
    let top = card.center().y - (logo_size + gap + text.size().y) * 0.5;
    if logo_size > 0.0 {
        crate::chrome::paint_logo(
            &painter,
            Rect::from_center_size(
                pos2(card.center().x, top + logo_size * 0.5),
                vec2(logo_size, logo_size),
            ),
            None,
            0.0,
            true,
        );
    }
    painter.galley(
        pos2(card.center().x - text.size().x * 0.5, top + logo_size + gap),
        text,
        Color32::WHITE,
    );
    context.accesskit_node_builder(egui::Id::new("external-file-drop-notice"), |node| {
        node.set_role(egui::accesskit::Role::Label);
        node.set_label(label);
        node.set_bounds(egui::accesskit::Rect::new(
            card.left().into(),
            card.top().into(),
            card.right().into(),
            card.bottom().into(),
        ));
        node.set_description(if blocked {
            "Dropping is unavailable while a dialog is open"
        } else {
            "Drop media files or a folder anywhere in the window; the outline is a visual guide"
        });
    });
}

fn dashed_border(painter: &egui::Painter, card: Rect, density: f32) {
    let pixel = 1.0 / density;
    let card = card.shrink(pixel * 0.5);
    let radius = 4.0_f32
        .min(card.width() * 0.5)
        .min(card.height() * 0.5)
        .max(0.0);
    let mut points = Vec::with_capacity(21);
    for (center, quarter) in [
        (card.left_top() + vec2(radius, radius), 2),
        (card.right_top() + vec2(-radius, radius), 3),
        (card.right_bottom() + vec2(-radius, -radius), 0),
        (card.left_bottom() + vec2(radius, -radius), 1),
    ] {
        for step in 0..=4 {
            let angle = (quarter as f32 + step as f32 / 4.0) * std::f32::consts::FRAC_PI_2;
            points.push(center + vec2(angle.cos(), angle.sin()) * radius);
        }
    }
    points.push(points[0]);
    painter.extend(egui::Shape::dashed_line(
        &points,
        egui::Stroke::new(pixel, Color32::WHITE),
        2.0 * pixel,
        2.0 * pixel,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_guide_matches_reference_geometry_and_retires_after_hover() {
        for density in [1.0, 1.25, 2.0] {
            for size in [vec2(1228.0, 708.0), vec2(240.0, 140.0), vec2(80.0, 64.0)] {
                for blocked in [false, true] {
                    let context = crate::fonts::test_context();
                    context.set_pixels_per_point(density);
                    context.enable_accesskit();
                    let viewport = Rect::from_min_size(egui::Pos2::ZERO, size);
                    let frame = |hovered| {
                        context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(viewport),
                                hovered_files: if hovered {
                                    vec![egui::HoveredFile::default()]
                                } else {
                                    vec![]
                                },
                                events: vec![egui::Event::PointerMoved(pos2(2.0, 2.0))],
                                ..Default::default()
                            },
                            |_| draw(&context, blocked),
                        )
                    };
                    frame(true);
                    let output = frame(true);
                    let dim = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Rect(rect)
                                if rect.fill == Color32::from_black_alpha(204) =>
                            {
                                Some(rect.rect)
                            }
                            _ => None,
                        })
                        .expect("full-client dimming");
                    assert_eq!(dim, viewport);
                    let card = Rect::from_center_size(
                        viewport.center(),
                        (size - vec2(32.0, 32.0)).min(vec2(344.0, 200.0)),
                    );
                    let dashes: Vec<_> = output
                        .shapes
                        .iter()
                        .filter_map(|shape| match &shape.shape {
                            egui::Shape::LineSegment { points, stroke }
                                if (stroke.width * density - 1.0).abs() < 0.0001 =>
                            {
                                Some((points, stroke))
                            }
                            _ => None,
                        })
                        .collect();
                    assert!(
                        !dashes.is_empty(),
                        "guide uses two-pixel dashes, not one-pixel dots"
                    );
                    let mut outline = Rect::NOTHING;
                    for (points, stroke) in &dashes {
                        assert_eq!(stroke.color, Color32::WHITE);
                        assert!(points[0].distance(points[1]) * density <= 2.001);
                        outline.extend_with(points[0]);
                        outline.extend_with(points[1]);
                    }
                    let outline = outline.expand(0.5 / density);
                    assert!((outline.min - card.min).length() < 2.1 / density);
                    assert!((outline.max - card.max).length() < 2.1 / density);
                    for axis in [0, 1] {
                        for edge in [
                            card.min[axis] + 0.5 / density,
                            card.max[axis] - 0.5 / density,
                        ] {
                            let straight: Vec<_> = dashes
                                .iter()
                                .filter_map(|(points, _)| {
                                    let midpoint = points[0].lerp(points[1], 0.5);
                                    ((points[0][axis] - edge).abs() < 0.001
                                        && (points[1][axis] - edge).abs() < 0.001
                                        && midpoint[1 - axis]
                                            > card.min[1 - axis] + 4.0 + 3.0 / density
                                        && midpoint[1 - axis]
                                            < card.max[1 - axis] - 4.0 - 3.0 / density)
                                        .then_some(*points)
                                })
                                .collect();
                            assert!(straight.len() >= 2, "exercise every straight edge");
                            for points in &straight {
                                assert!(
                                    (points[0].distance(points[1]) * density - 2.0).abs() < 0.001,
                                    "two physical pixels per dash at density {density}"
                                );
                            }
                            for pair in straight.windows(2) {
                                assert!(
                                    (pair[0][1].distance(pair[1][0]) * density - 2.0).abs() < 0.001,
                                    "two physical pixels per gap at density {density}"
                                );
                            }
                        }
                    }
                    let label = if blocked {
                        "Close the dialog before dropping files"
                    } else {
                        "Open with towavue"
                    };
                    let text = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.text() == label => {
                                Some((shape.clip_rect, text))
                            }
                            _ => None,
                        })
                        .expect("drop instruction");
                    assert!(card.contains_rect(text.0));
                    if size.x >= 344.0 && !blocked {
                        assert!(
                            (text.1.pos.x + text.1.galley.size().x * 0.5 - card.center().x).abs()
                                < 0.001
                        );
                        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::LineSegment { stroke, .. } if (stroke.width - 3.3).abs() < 0.001)), "reuse the 48-point app logo");
                    }
                    assert!(
                        context.dragged_id().is_none(),
                        "guide paints without owning input"
                    );
                    let notice = output
                        .platform_output
                        .accesskit_update
                        .as_ref()
                        .expect("accessibility")
                        .nodes
                        .iter()
                        .find(|(_, node)| node.label() == Some(label))
                        .expect("drop notice");
                    assert_eq!(notice.1.role(), egui::accesskit::Role::Label);
                    let cleared = frame(false);
                    assert!(
                        cleared.shapes.is_empty(),
                        "leave/drop removes the entire guide"
                    );
                }
            }
        }
    }

    #[test]
    fn centered_guide_preserves_whole_window_folder_drop_and_modal_guard() {
        let Some(root) = crate::tests::isolated_test_root(
            "file_drop::tests::centered_guide_preserves_whole_window_folder_drop_and_modal_guard",
        ) else {
            return;
        };
        let folder = root.join("dropped-folder");
        std::fs::create_dir(&folder).expect("owned empty folder");
        let mut app = crate::Application::new(None, |_| {}).expect("app");
        let context = crate::fonts::test_context();
        app.ui_context = Some(context.clone());
        app.pending_dialog = Some(crate::DialogIntent::OpenFolder);
        for pointer in [pos2(2.0, 2.0), pos2(400.0, 250.0), pos2(798.0, 498.0)] {
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 500.0))),
                    hovered_files: vec![egui::HoveredFile {
                        path: Some(folder.clone()),
                        ..Default::default()
                    }],
                    events: vec![egui::Event::PointerMoved(pointer)],
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut vec![]),
            );
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "Close the dialog before dropping files")));
            app.open_dropped_path(folder.clone());
            assert!(app.pending_folder.is_none());
            app.pending_dialog = None;
            app.open_dropped_path(folder.clone());
            assert!(
                app.pending_folder.is_some(),
                "the central guide is not a drop hit rectangle"
            );
            app.pending_folder = None;
            app.pending_dialog = Some(crate::DialogIntent::OpenFolder);
        }
        assert!(app.edits.is_empty());
    }
}
