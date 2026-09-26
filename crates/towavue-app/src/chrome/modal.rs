use egui::{Context, Id, Rect, Ui};

use crate::scroll_style::ScrollAreaStyle;

const MARGIN: f32 = 8.0;
const FRAME_SPACE: f32 = 18.0;
const MAX_CONTENT_WIDTH: f32 = 400.0;

pub fn set_modal_bounds(context: &Context, bounds: Rect) {
    let pass = context.cumulative_pass_nr();
    context.data_mut(|data| data.insert_temp(Id::new("modal-media-bounds"), (pass, bounds)));
}

fn bounds(context: &Context) -> Rect {
    let rect = context
        .data(|data| data.get_temp::<(u64, Rect)>(Id::new("modal-media-bounds")))
        .filter(|(pass, _)| *pass == context.cumulative_pass_nr())
        .map_or_else(|| context.content_rect(), |(_, rect)| rect);
    // Release the outer vertical gap before clipping compact dialog controls.
    let vertical_gap = MARGIN.min(((rect.height() - 112.0) * 0.5).max(0.0));
    rect.shrink2(egui::vec2(MARGIN, vertical_gap))
}

pub fn modal(context: &Context, id: Id, preview: bool) -> egui::Modal {
    let media = bounds(context);
    let client = context.content_rect();
    // Keep the backdrop's clip and hit area over the full client. Only the
    // dialog's anchor and body budget follow the media viewport.
    let (anchor, offset) = if preview {
        (
            egui::Align2::RIGHT_BOTTOM,
            media.right_bottom() - client.right_bottom(),
        )
    } else {
        (
            egui::Align2::CENTER_CENTER,
            media.center() - client.center(),
        )
    };
    let area = egui::Modal::default_area(id)
        .constrain_to(client)
        .anchor(anchor, offset);
    let frame = egui::Frame::popup(&context.global_style()).inner_margin(MARGIN);
    let modal = egui::Modal::new(id).area(area).frame(frame);
    if preview {
        modal.backdrop_color(egui::Color32::TRANSPARENT)
    } else {
        modal
    }
}

/// Header and action rows stay outside the sole scrolling body. Measure their
/// space at the current width, so a short dialog shrinks naturally and a long
/// one can use the media viewport before it needs to scroll.
pub fn modal_body<R>(
    ui: &mut Ui,
    title: &str,
    actions: &[&str],
    content: impl FnOnce(&mut Ui) -> R,
) -> R {
    let bounds = bounds(ui.ctx());
    // Reset the Area's remembered maximum to permit growth, without reserving
    // empty width. The body and footer determine the minimum on every pass.
    ui.set_max_width(MAX_CONTENT_WIDTH.min((bounds.width() - FRAME_SPACE).max(1.0)));
    // Area remembers its previous content size; reset the maximum so a body can
    // grow beyond that size before the scroll area computes its available space.
    ui.set_max_height((bounds.height() - FRAME_SPACE).max(1.0));
    if bounds.height() < 200.0 {
        ui.spacing_mut().item_spacing.y = 2.0;
    }
    ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
        node.set_role(egui::accesskit::Role::Dialog);
        node.set_label(title);
        node.set_modal();
    });
    let heading = ui.label(egui::RichText::new(title).color(super::FOREGROUND));
    let font = egui::TextStyle::Button.resolve(ui.style());
    let row_height =
        ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y;
    // Atom/button layout rounds its row allocation to logical pixels.
    let row_height = row_height.max(ui.spacing().interact_size.y).ceil();
    let mut rows = usize::from(!actions.is_empty());
    let mut used = 0.0;
    for action in actions {
        let width = ui
            .painter()
            .layout_no_wrap((*action).into(), font.clone(), super::MUTED)
            .size()
            .x
            + 2.0 * ui.spacing().button_padding.x;
        if used > 0.0 && used + ui.spacing().item_spacing.x + width > ui.available_width() {
            rows += 1;
            used = 0.0;
        }
        used += if used == 0.0 {
            width
        } else {
            ui.spacing().item_spacing.x + width
        };
    }
    let footer = rows as f32 * (row_height + ui.spacing().item_spacing.y);
    let height = (bounds.height()
        - FRAME_SPACE
        - heading.rect.height()
        - ui.spacing().item_spacing.y
        - footer)
        .max(1.0);
    let clip_margin = ui.visuals().clip_rect_margin;
    ui.visuals_mut().clip_rect_margin = 0.0;
    let body = egui::ScrollArea::vertical()
        .id_salt("modal-body")
        .auto_shrink([true, true])
        .max_height(height)
        .min_scrolled_height(1.0)
        .show_styled(ui, content);
    ui.visuals_mut().clip_rect_margin = clip_margin;
    body.inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modal_width_follows_content_and_shrinks_after_larger_content() {
        for density in [1.0, 1.25, 2.0] {
            for preview in [false, true] {
                let context = crate::fonts::test_context();
                context.global_style_mut(crate::chrome::style);
                context.set_pixels_per_point(density);
                for viewport_width in [960.0, 420.0] {
                    let viewport =
                        Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(viewport_width, 260.0));
                    let media = Rect::from_min_max(
                        egui::pos2(0.0, 32.0),
                        egui::pos2(viewport_width, 230.0),
                    );
                    for width in [260.0, 140.0, 300.0, 140.0] {
                        let mut outer = Rect::NOTHING;
                        let mut content = Rect::NOTHING;
                        let mut clip = Rect::NOTHING;
                        let mut buttons = Vec::new();
                        for _ in 0..4 {
                            buttons.clear();
                            let _ =
                                context.run_ui(
                                    egui::RawInput {
                                        screen_rect: Some(viewport),
                                        ..Default::default()
                                    },
                                    |ui| {
                                        set_modal_bounds(ui.ctx(), media);
                                        outer = modal(
                                            ui.ctx(),
                                            "intrinsic-modal-width".into(),
                                            preview,
                                        )
                                        .show(ui.ctx(), |ui| {
                                            modal_body(ui, "Size", &["Apply", "Cancel"], |ui| {
                                                content = ui
                                                    .allocate_exact_size(
                                                        egui::vec2(width, 50.0),
                                                        egui::Sense::hover(),
                                                    )
                                                    .0;
                                                clip = ui.clip_rect();
                                            });
                                            ui.horizontal_wrapped(|ui| {
                                                crate::chrome::flat_buttons(ui);
                                                buttons.push(ui.button("Apply").rect);
                                                buttons.push(ui.button("Cancel").rect);
                                            });
                                        })
                                        .response
                                        .rect;
                                    },
                                );
                        }
                        assert!(
                            (outer.width() - width - FRAME_SPACE).abs() <= 1.0 / density,
                            "content determines width at density={density}, preview={preview}: {width}, {outer:?}"
                        );
                        assert!(media.contains_rect(outer), "dialog stays inside media");
                        assert!(outer.contains_rect(content));
                        assert!(
                            clip.contains_rect(content),
                            "settled content is fully visible: density={density}, preview={preview}, width={width}, viewport={viewport_width}, clip={clip:?}, content={content:?}"
                        );
                        assert!(buttons.iter().all(|button| outer.contains_rect(*button)
                            && button.top() >= content.bottom()));
                        if preview {
                            assert!(
                                (media.right() - MARGIN - outer.right()).abs() <= 1.0 / density
                            );
                            assert!(
                                (media.bottom() - MARGIN - outer.bottom()).abs() <= 1.0 / density
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn modal_header_and_actions_stay_fixed_while_body_uses_the_media_height() {
        for density in [1.0, 1.25, 2.0] {
            for preview in [false, true] {
                for size in [
                    egui::vec2(240.0, 150.0),
                    egui::vec2(320.0, 240.0),
                    egui::vec2(960.0, 708.0),
                ] {
                    let context = crate::fonts::test_context();
                    context.global_style_mut(crate::chrome::style);
                    context.set_pixels_per_point(density);
                    let media = Rect::from_min_max(
                        egui::pos2(0.0, 32.0),
                        egui::pos2(size.x, size.y - 30.0),
                    );
                    let mut time = 0.0;
                    let mut frame = |events, rows| {
                        time += 0.1;
                        let mut geometry =
                            (Rect::NOTHING, Rect::NOTHING, Rect::NOTHING, Rect::NOTHING);
                        let output = context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
                                time: Some(time),
                                events,
                                ..Default::default()
                            },
                            |ui| {
                                set_modal_bounds(ui.ctx(), media);
                                geometry.0 = modal(ui.ctx(), Id::new("layout-control"), preview)
                                    .show(ui.ctx(), |ui| {
                                        modal_body(
                                            ui,
                                            "Dialog title",
                                            &["Save and continue", "Discard edits", "Cancel"],
                                            |ui| {
                                                geometry.1 = ui.clip_rect();
                                                for index in 0..rows {
                                                    let row = ui.label(format!("Body row {index}"));
                                                    if index == 0 {
                                                        geometry.2 = row.rect;
                                                    }
                                                }
                                            },
                                        );
                                        ui.horizontal_wrapped(|ui| {
                                            crate::chrome::flat_buttons(ui);
                                            for action in
                                                ["Save and continue", "Discard edits", "Cancel"]
                                            {
                                                let response = ui.button(action);
                                                if action == "Cancel" {
                                                    geometry.3 = response.rect;
                                                }
                                            }
                                        });
                                    })
                                    .response
                                    .rect;
                            },
                        );
                        let title = output.shapes.iter().find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.text() == "Dialog title" => {
                                Some(text)
                            }
                            _ => None,
                        });
                        let Some(title) = title else {
                            return (geometry, Rect::NOTHING);
                        };
                        assert_eq!(
                            title.galley.job.sections[0].format.color,
                            crate::chrome::FOREGROUND
                        );
                        assert_eq!(
                            title.galley.job.sections[0].format.font_id,
                            egui::TextStyle::Body.resolve(&context.global_style())
                        );
                        (geometry, title.galley.rect.translate(title.pos.to_vec2()))
                    };
                    for _ in 0..4 {
                        frame(vec![], 80);
                    }
                    let (before, title) = frame(vec![], 80);
                    eprintln!(
                        "MODAL_LAYOUT density={density} preview={preview} size={size:?} media={media:?} geometry={before:?} title={title:?}"
                    );
                    assert!(title.is_positive(), "settled modal paints its title");
                    let expected_width = 418.0_f32.min(media.width() - 16.0);
                    assert!(
                        before.0.width() <= expected_width + 1.0 / density,
                        "content stays within its wrapping limit: {:?}",
                        before.0
                    );
                    if size.x > 400.0 {
                        assert!(
                            before.0.width() < 360.0,
                            "short rows do not reserve 400 points"
                        );
                    }
                    assert!(
                        media.contains_rect(before.0),
                        "{density} {size:?}: {:?}",
                        before.0
                    );
                    assert!(
                        before.0.height() >= media.height() - 20.0,
                        "long body uses available height: {density} {preview} {size:?}: {before:?}"
                    );
                    for _ in 0..12 {
                        frame(
                            vec![
                                egui::Event::PointerMoved(egui::pos2(
                                    before.2.center().x,
                                    before.1.center().y,
                                )),
                                egui::Event::MouseWheel {
                                    unit: egui::MouseWheelUnit::Point,
                                    phase: egui::TouchPhase::Move,
                                    delta: egui::vec2(0.0, -200.0),
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ],
                            80,
                        );
                    }
                    let (after, after_title) = frame(vec![], 80);
                    assert_eq!(title, after_title, "heading does not scroll");
                    assert_eq!(before.3, after.3, "wrapped footer does not scroll");
                    assert!(
                        after.2.top() < before.2.top() - 100.0,
                        "body actually scrolled: {density} {preview} {size:?}: {before:?} -> {after:?}"
                    );
                    assert!(
                        after.3.top() >= after.1.bottom(),
                        "actions are outside body clip"
                    );
                    for _ in 0..4 {
                        frame(vec![], 1);
                    }
                    let (short, _) = frame(vec![], 1);
                    if size.y >= 240.0 {
                        assert!(
                            short.0.height() < before.0.height() - 20.0,
                            "short content shrinks naturally"
                        );
                        assert!(
                            short.1.contains_rect(short.2),
                            "short content is fully visible"
                        );
                    }
                }
            }
        }
    }
}
