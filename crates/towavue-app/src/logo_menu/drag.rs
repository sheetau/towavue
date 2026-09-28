use super::*;
use crate::scroll_style::ScrollAreaStyle;

pub(crate) fn held(context: &egui::Context) -> bool {
    context
        .data(|data| data.get_temp::<State>(state_id()))
        .and_then(|state| state.drag)
        .is_some_and(|drag| drag.section.is_some())
}

fn hit(response: &egui::Response, position: egui::Pos2) -> bool {
    response.enabled()
        && response.interact_rect.contains(position)
        && response.ctx.layer_id_at(position) == Some(response.layer_id)
}

pub(crate) fn released(response: &egui::Response) -> bool {
    response
        .ctx
        .data(|data| data.get_temp::<State>(state_id()))
        .filter(|state| state.section.is_some())
        .and_then(|state| state.release)
        .is_some_and(|(frame, position)| {
            frame == response.ctx.cumulative_frame_nr() && hit(response, position)
        })
}

fn hovered(response: &egui::Response) -> bool {
    held(&response.ctx)
        && response
            .ctx
            .input(|input| input.pointer.hover_pos())
            .is_some_and(|position| hit(response, position))
}

pub(crate) fn clicked(response: &egui::Response) -> bool {
    if hovered(response) {
        response.ctx.highlight_widget(response.id);
        if !response.highlighted() {
            response.ctx.request_repaint();
        }
    }
    response.clicked() || released(response)
}

pub(crate) fn submenu(ui: &egui::Ui, id: egui::Id) -> bool {
    ui.ctx()
        .read_response(id)
        .is_some_and(|response| hovered(&response))
}

#[derive(Clone)]
struct ScrollView {
    frame: u64,
    bounds: egui::Rect,
    offset: f32,
    maximum: f32,
}
#[derive(Clone)]
struct Edge {
    direction: i8,
    entered: f64,
    frame: u64,
    offset: f32,
}

pub(crate) fn scroll<R>(
    ui: &mut egui::Ui,
    salt: egui::Id,
    height: f32,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::scroll_area::ScrollAreaOutput<R> {
    let key = ui.id().with(("logo-drag-scroll", salt));
    let edge_key = key.with("edge");
    let context = ui.ctx().clone();
    let frame = context.cumulative_frame_nr();
    let mut area = egui::ScrollArea::vertical()
        .id_salt(salt)
        .max_height(height);
    let previous = context
        .data(|data| data.get_temp::<ScrollView>(key))
        .filter(|view| view.frame == frame || view.frame.checked_add(1) == Some(frame));
    let pointer = context.input(|input| input.pointer.hover_pos());
    let mut at_edge = false;
    if held(&context)
        && let Some(view) = previous
        && let Some(pointer) = pointer
        && view.bounds.contains(pointer)
        && context.layer_id_at(pointer) == Some(ui.layer_id())
    {
        let depth = 20.0_f32.min(view.bounds.height() / 4.0);
        let top = ((view.bounds.top() + depth - pointer.y) / depth).clamp(0.0, 1.0);
        let bottom = ((pointer.y - view.bounds.bottom() + depth) / depth).clamp(0.0, 1.0);
        let speed = if top > 0.0 && view.offset > 0.0 {
            -top
        } else if view.offset < view.maximum {
            bottom
        } else {
            0.0
        };
        if speed != 0.0 {
            at_edge = true;
            let direction = if speed < 0.0 { -1 } else { 1 };
            let (now, dt) = context.input(|input| (input.time, input.stable_dt.min(0.05)));
            let mut edge = context
                .data(|data| data.get_temp::<Edge>(edge_key))
                .filter(|edge| edge.direction == direction)
                .unwrap_or(Edge {
                    direction,
                    entered: now,
                    frame: u64::MAX,
                    offset: view.offset,
                });
            let remaining = 0.3 - (now - edge.entered);
            if remaining > 0.0 {
                // A brief dwell lets the first/last visible row be selected before
                // scrolling begins. Release never moves content under the pointer.
                context.request_repaint_after(Duration::from_secs_f64(remaining));
            } else {
                if edge.frame != frame {
                    edge.frame = frame;
                    edge.offset = (view.offset + speed * 420.0 * dt).clamp(0.0, view.maximum);
                }
                area = area.vertical_scroll_offset(edge.offset);
                context.request_repaint();
            }
            context.data_mut(|data| data.insert_temp(edge_key, edge));
        }
    }
    if !at_edge {
        context.data_mut(|data| data.remove::<Edge>(edge_key));
    }
    let output = area.show_styled(ui, content);
    context.data_mut(|data| {
        data.insert_temp(
            key,
            ScrollView {
                frame,
                bounds: output.inner_rect,
                offset: output.state.offset.y,
                maximum: (output.content_size.y - output.inner_rect.height()).max(0.0),
            },
        )
    });
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_scroll_waits_scales_with_distance_and_stops_before_release_hit_testing() {
        for density in [1.0, 1.25, 2.0] {
            let context = fonts::test_context();
            context.set_pixels_per_point(density);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
            let mut bounds = egui::Rect::NOTHING;
            let mut offset = 0.0;
            let mut hit_row = None;
            let mut now = 0.0;
            let mut render = |position: Option<egui::Pos2>, release: bool, step: f64| {
                now += step;
                let at = context.cumulative_frame_nr();
                context.data_mut(|data| {
                    data.insert_temp(
                        state_id(),
                        State {
                            section: Some(Section::View),
                            drag: (!release).then_some(Drag {
                                id: egui::Id::new("owner"),
                                popup: egui::Id::new("popup"),
                                origin: egui::Pos2::ZERO,
                                source: (None, 0, 0),
                                screen,
                                density,
                                crossed: true,
                                left_button: true,
                                section: Some(Section::View),
                            }),
                            release: release.then(|| (at, position.expect("release point"))),
                            ..Default::default()
                        },
                    )
                });
                let mut selected = None;
                let _ = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        time: Some(now),
                        events: position
                            .map(egui::Event::PointerMoved)
                            .into_iter()
                            .collect(),
                        ..Default::default()
                    },
                    |ui| {
                        let output = scroll(ui, egui::Id::new("test"), 180.0, |ui| {
                            for row in 0..50 {
                                let response = ui.button(format!("Item {row}"));
                                if position.is_some_and(|pos| response.interact_rect.contains(pos))
                                {
                                    hit_row = Some(row);
                                }
                                if clicked(&response) {
                                    selected = Some(row);
                                }
                            }
                        });
                        bounds = output.inner_rect;
                        offset = output.state.offset.y;
                    },
                );
                (bounds, offset, hit_row, selected)
            };
            let (bounds, _, _, _) = render(None, false, 0.016);
            render(None, false, 0.016);
            let edge = egui::pos2(bounds.left() + 20.0, bounds.bottom() - 1.0);
            for _ in 0..10 {
                assert_eq!(
                    render(Some(edge), false, 0.016).1,
                    0.0,
                    "dwell protects edge rows"
                );
            }
            for _ in 0..25 {
                render(Some(edge), false, 0.016);
            }
            let (_, moved, row, _) = render(Some(edge), false, 0.016);
            assert!(moved > 50.0, "stationary edge scrolls: {moved}");
            let (_, released_offset, _, selected) = render(Some(edge), true, 0.016);
            assert_eq!(released_offset, moved, "release cannot move the target");
            assert_eq!(selected, row, "select the visible row on release");
            let center = bounds.center();
            assert_eq!(render(Some(center), false, 0.016).1, moved);
            let near = egui::pos2(edge.x, bounds.bottom() - 15.0);
            for _ in 0..22 {
                render(Some(near), false, 0.016);
            }
            let slow = render(Some(near), false, 0.016).1;
            let slow_step = render(Some(near), false, 0.016).1 - slow;
            let fast = render(Some(edge), false, 0.016).1;
            let fast_step = render(Some(edge), false, 0.016).1 - fast;
            assert!(fast_step > slow_step * 2.0, "speed follows edge distance");
        }
    }
}
