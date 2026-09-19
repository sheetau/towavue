use std::{ops::Range, sync::Arc};

use egui::{Rect, ScrollArea, Style, Ui, scroll_area::ScrollAreaOutput};

pub(crate) trait ScrollAreaStyle {
    fn show_styled<R>(self, ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R)
    -> ScrollAreaOutput<R>;
    fn show_rows_styled<R>(
        self,
        ui: &mut Ui,
        height: f32,
        count: usize,
        content: impl FnOnce(&mut Ui, Range<usize>) -> R,
    ) -> ScrollAreaOutput<R>;
    /// Virtual rows with scrolling top/bottom space, independent of the bar track.
    fn show_rows_padded_styled<R>(
        self,
        ui: &mut Ui,
        height: f32,
        count: usize,
        padding: [f32; 2],
        content: impl FnOnce(&mut Ui, Range<usize>) -> R,
    ) -> ScrollAreaOutput<R>;
    fn show_viewport_styled<R>(
        self,
        ui: &mut Ui,
        content: impl FnOnce(&mut Ui, Rect) -> R,
    ) -> ScrollAreaOutput<R>;
}

fn with_style<R>(
    ui: &mut Ui,
    show: impl FnOnce(&mut Ui, Arc<Style>) -> ScrollAreaOutput<R>,
) -> ScrollAreaOutput<R> {
    let original = ui.style().clone();
    // egui paints bars with the parent's widget visuals after laying out content.
    // Restore the content's style separately, without adding a scope or changing IDs.
    let widgets = &mut ui.visuals_mut().widgets;
    // Track and thumb hover share one opaque handle color. The scroll style
    // supplies idle opacity and preserves the tab strip's separate fade.
    let handle_color = egui::Color32::from_gray(0xcc);
    widgets.inactive.fg_stroke.color = handle_color;
    widgets.hovered.fg_stroke.color = handle_color;
    widgets.active.fg_stroke.color = handle_color;
    for visual in [
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
    ] {
        visual.corner_radius = egui::CornerRadius::same(u8::MAX);
    }
    let result = show(ui, original.clone());
    crate::wheel_input::record_scroll_area(ui, &result);
    ui.set_style(original);
    result
}

impl ScrollAreaStyle for ScrollArea {
    fn show_styled<R>(
        self,
        ui: &mut Ui,
        content: impl FnOnce(&mut Ui) -> R,
    ) -> ScrollAreaOutput<R> {
        with_style(ui, |ui, original| {
            self.show(ui, |ui| {
                ui.set_style(original);
                content(ui)
            })
        })
    }

    fn show_rows_styled<R>(
        self,
        ui: &mut Ui,
        height: f32,
        count: usize,
        content: impl FnOnce(&mut Ui, Range<usize>) -> R,
    ) -> ScrollAreaOutput<R> {
        with_style(ui, |ui, original| {
            self.show_rows(ui, height, count, |ui, rows| {
                ui.set_style(original);
                content(ui, rows)
            })
        })
    }

    fn show_rows_padded_styled<R>(
        self,
        ui: &mut Ui,
        height: f32,
        count: usize,
        [top, bottom]: [f32; 2],
        content: impl FnOnce(&mut Ui, Range<usize>) -> R,
    ) -> ScrollAreaOutput<R> {
        let spacing = ui.spacing().item_spacing.y;
        let pitch = height + spacing;
        self.content_margin(egui::Margin::ZERO)
            .show_viewport_styled(ui, |ui, viewport| {
                ui.set_height((pitch * count as f32 - spacing).max(0.0) + top + bottom);
                // egui's ordinary show_rows assumes row zero starts at offset zero.
                // Subtract the leading space when virtualizing, or partial first rows
                // disappear one padding-height too early while scrolling.
                let start = (((viewport.top() - top).max(0.0) / pitch).floor() as usize).min(count);
                let end =
                    (((viewport.bottom() - top).max(0.0) / pitch).ceil() as usize + 1).min(count);
                let origin = ui.max_rect().top() + top;
                let rect = Rect::from_x_y_ranges(
                    ui.max_rect().x_range(),
                    (origin + start as f32 * pitch)..=(origin + end as f32 * pitch),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    ui.skip_ahead_auto_ids(start);
                    content(ui, start..end)
                })
                .inner
            })
    }

    fn show_viewport_styled<R>(
        self,
        ui: &mut Ui,
        content: impl FnOnce(&mut Ui, Rect) -> R,
    ) -> ScrollAreaOutput<R> {
        with_style(ui, |ui, original| {
            self.show_viewport(ui, |ui, viewport| {
                ui.set_style(original);
                content(ui, viewport)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padded_rows_keep_partial_rows_and_scroll_padding_inside_the_viewport() {
        for density in [1.0, 1.25, 2.0] {
            for (height, top) in [(24.0, 0.0), (32.0, 8.0)] {
                for count in [0, 2, 10_000] {
                    let context = egui::Context::default();
                    context.set_pixels_per_point(density);
                    context.global_style_mut(crate::chrome::style);
                    let viewport =
                        Rect::from_min_max(egui::pos2(8.0, 32.0), egui::pos2(232.0, 276.0));
                    let total = height * count as f32 + top + 8.0;
                    let maximum = (total - viewport.height()).max(0.0);
                    for offset in [0.0, 31.0_f32.min(maximum), 1234.0_f32.min(maximum), maximum] {
                        let mut rows = Vec::new();
                        let mut measured = None;
                        for _ in 0..3 {
                            rows.clear();
                            let _ = context.run_ui(
                                egui::RawInput {
                                    screen_rect: Some(Rect::from_min_size(
                                        egui::Pos2::ZERO,
                                        egui::vec2(240.0, 300.0),
                                    )),
                                    ..Default::default()
                                },
                                |ui| {
                                    let mut child =
                                        ui.new_child(egui::UiBuilder::new().max_rect(viewport));
                                    child.set_clip_rect(viewport);
                                    child.visuals_mut().clip_rect_margin = 0.0;
                                    child.spacing_mut().item_spacing.y = 0.0;
                                    let result = ScrollArea::vertical()
                                        .auto_shrink([false, false])
                                        .vertical_scroll_offset(offset)
                                        .show_rows_padded_styled(
                                            &mut child,
                                            height,
                                            count,
                                            [top, 8.0],
                                            |ui, range| {
                                                for index in range {
                                                    let response = ui.allocate_response(
                                                        egui::vec2(180.0, height),
                                                        egui::Sense::click(),
                                                    );
                                                    rows.push((
                                                        index,
                                                        response.rect,
                                                        ui.clip_rect(),
                                                    ));
                                                }
                                            },
                                        );
                                    measured = Some((result.content_size.y, result.state.offset.y));
                                },
                            );
                        }
                        let (content_height, actual_offset) =
                            measured.expect("laid out scroll area");
                        assert!(
                            (content_height - total).abs() <= 1.0 / density,
                            "content height: {content_height} vs {total}"
                        );
                        assert!((actual_offset - offset).abs() <= 1.0 / density);
                        assert!(rows.len() <= 13, "large lists remain virtualized");
                        if count == 0 {
                            assert!(rows.is_empty());
                            continue;
                        }
                        for index in 0..count {
                            let y = viewport.top() + top + index as f32 * height - offset;
                            if y < viewport.bottom() && y + height > viewport.top() {
                                let (_, rect, clip) = rows
                                    .iter()
                                    .find(|(row, _, _)| *row == index)
                                    .expect("every partially visible row must be laid out");
                                assert!((rect.top() - y).abs() <= 1.0 / density);
                                assert_eq!(clip.y_range(), viewport.y_range());
                            }
                        }
                        if maximum > 0.0 && offset == maximum {
                            let (_, last, _) = rows
                                .iter()
                                .find(|(index, _, _)| *index + 1 == count)
                                .expect("last virtual row");
                            assert!(
                                (viewport.bottom() - last.bottom() - 8.0).abs() <= 1.0 / density
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn scrollbars_have_only_pill_handles_until_hovered_without_restyling_content() {
        for density in [1.0, 1.25, 2.0] {
            for (kind, horizontal) in [(0, false), (0, true), (1, false), (2, false), (2, true)] {
                let context = egui::Context::default();
                context.global_style_mut(crate::chrome::style);
                context.global_style_mut(|style| style.animation_time = 0.0);
                let mut pointer = None;
                let mut pressed = false;
                let mut render = |point: Option<egui::Pos2>, down: bool| {
                    let mut events = vec![];
                    if let Some(point) = point {
                        events.push(egui::Event::PointerMoved(point));
                        if pressed != down {
                            events.push(egui::Event::PointerButton {
                                pos: point,
                                button: egui::PointerButton::Primary,
                                pressed: down,
                                modifiers: egui::Modifiers::NONE,
                            });
                        }
                    } else if pointer.is_some() {
                        events.push(egui::Event::PointerGone);
                    }
                    pointer = point;
                    pressed = down;
                    context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(240.0, 180.0),
                            )),
                            events,
                            viewports: [(
                                egui::ViewportId::ROOT,
                                egui::ViewportInfo {
                                    native_pixels_per_point: Some(density),
                                    ..Default::default()
                                },
                            )]
                            .into_iter()
                            .collect(),
                            ..Default::default()
                        },
                        |ui| {
                            let original = ui.style().clone();
                            let area = ScrollArea::new([horizontal, !horizontal])
                                .auto_shrink([false, false]);
                            let content = |ui: &mut Ui| {
                                assert_eq!(
                                    ui.visuals().widgets,
                                    original.visuals.widgets,
                                    "content keeps button geometry"
                                );
                                ui.set_min_size(if horizontal {
                                    egui::vec2(720.0, 100.0)
                                } else {
                                    egui::vec2(100.0, 540.0)
                                });
                            };
                            match kind {
                                0 => {
                                    area.show_styled(ui, content);
                                }
                                1 => {
                                    area.show_rows_styled(ui, 18.0, 30, |ui, _| content(ui));
                                }
                                2 => {
                                    area.show_viewport_styled(ui, |ui, _| content(ui));
                                }
                                _ => unreachable!(),
                            }
                            assert_eq!(
                                ui.visuals().widgets,
                                original.visuals.widgets,
                                "siblings keep button geometry"
                            );
                        },
                    )
                };
                let bars = |output: &egui::FullOutput| {
                    output
                        .shapes
                        .iter()
                        .filter_map(|shape| match &shape.shape {
                            egui::Shape::Rect(rect)
                                if rect.rect.size().min_elem() <= 5.0
                                    && rect.rect.size().max_elem() > 12.0 =>
                            {
                                Some(rect.clone())
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                };
                for _ in 0..3 {
                    render(None, false);
                }
                let idle = bars(&render(None, false));
                assert_eq!(idle[0].fill.a(), 0);
                let handle_color = egui::Color32::from_gray(0xcc);
                assert_eq!(idle[1].fill, handle_color.gamma_multiply(0.6));
                assert!(
                    f32::from(idle[1].corner_radius.nw) >= idle[1].rect.size().min_elem() / 2.0
                );
                let output = render(Some(egui::pos2(80.0, 80.0)), false);
                let shapes = bars(&output);
                assert_eq!(shapes.len(), 2, "track and handle are painted");
                assert_eq!(
                    shapes[0].fill.a(),
                    0,
                    "hovering content does not show a track"
                );
                let handle = shapes[1].rect;
                let track = shapes[0].rect;
                let track_point = if horizontal {
                    egui::pos2(track.right() - 2.0, track.center().y)
                } else {
                    egui::pos2(track.center().x, track.bottom() - 2.0)
                };
                assert!(!handle.contains(track_point));
                let track_hover = bars(&render(Some(track_point), false));
                assert_eq!(
                    track_hover[1].fill, handle_color,
                    "track hover shares the opaque handle color"
                );
                assert!(
                    (102..=128).contains(&track_hover[0].fill.a()),
                    "track remains legible over white content at 40-50% opacity"
                );
                for down in [false, true, false] {
                    let output = render(Some(handle.center()), down);
                    let shapes = bars(&output);
                    assert!(
                        shapes[0].fill.a() > 0,
                        "interacting with the bar shows its track"
                    );
                    let handle = &shapes[1];
                    assert_eq!(
                        handle.fill, handle_color,
                        "hover and press share the same opaque #ccc"
                    );
                    assert_eq!(output.pixels_per_point, density);
                    assert!(
                        f32::from(handle.corner_radius.nw) >= handle.rect.size().min_elem() / 2.0,
                        "handle has pill ends while hovered or dragged: {handle:?}"
                    );
                }
                render(Some(handle.center()), true);
                let outside = egui::pos2(80.0, 80.0);
                let dragging = bars(&render(Some(outside), true));
                assert_eq!(dragging[1].fill, handle_color, "captured drag stays opaque");
                render(Some(outside), false);
                let released = bars(&render(Some(outside), false));
                assert_eq!(
                    released[1].fill, idle[1].fill,
                    "release restores idle opacity"
                );
            }
        }
    }
}
