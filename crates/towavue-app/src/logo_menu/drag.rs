use super::*;
use crate::scroll_style::ScrollAreaStyle;

pub(crate) fn held(context: &egui::Context) -> bool {
    context
        .data(|data| data.get_temp::<State>(state_id()))
        .is_some_and(|state| state.drag.is_some() && state.section.is_some())
}

pub(crate) fn captured(context: &egui::Context) -> bool {
    context
        .data(|data| data.get_temp::<State>(state_id()))
        .is_some_and(|state| {
            state.drag.is_some() || state.release == Some(context.cumulative_frame_nr())
        })
}

fn hit(response: &egui::Response, position: egui::Pos2) -> bool {
    response.enabled()
        && response.interact_rect.contains(position)
        && response.ctx.layer_id_at(position) == Some(response.layer_id)
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
    response.clicked() && !captured(&response.ctx)
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

pub(crate) fn scroll<R>(
    ui: &mut egui::Ui,
    salt: egui::Id,
    height: f32,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::scroll_area::ScrollAreaOutput<R> {
    let key = ui.id().with(("logo-drag-scroll", salt));
    let context = ui.ctx().clone();
    let frame = context.cumulative_frame_nr();
    let mut area = egui::ScrollArea::vertical()
        .id_salt(salt)
        .max_height(height);
    // egui blocks wheel scrolling while another widget owns a drag. Consume its
    // already normalized/smoothed wheel delta without releasing the logo owner.
    if held(&context)
        && let Some(view) = context.data(|data| data.get_temp::<ScrollView>(key))
        && (view.frame == frame || view.frame.checked_add(1) == Some(frame))
        && context
            .input(|input| input.pointer.hover_pos())
            .is_some_and(|point| {
                view.bounds.contains(point) && context.layer_id_at(point) == Some(ui.layer_id())
            })
    {
        let only_direction = ui.style().always_scroll_the_only_direction;
        let delta = context.input(|input| {
            let delta = input.smooth_scroll_delta();
            if only_direction {
                delta.x + delta.y
            } else {
                delta.y
            }
        });
        if (delta > 0.0 && view.offset > 0.0) || (delta < 0.0 && view.offset < view.maximum) {
            area = area.vertical_scroll_offset((view.offset - delta).clamp(0.0, view.maximum));
            context.input_mut(|input| {
                if only_direction {
                    input.smooth_scroll_delta = egui::Vec2::ZERO;
                } else {
                    input.smooth_scroll_delta.y = 0.0;
                }
            });
            context.request_repaint();
        }
    }
    let output = area.show_styled(ui, content);
    context.data_mut(|data| {
        data.insert_temp(
            key,
            ScrollView {
                frame,
                bounds: output.inner_rect.intersect(ui.clip_rect()),
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
    fn held_menu_allows_hover_and_wheel_without_edge_scroll_or_release_selection() {
        for density in [1.0, 1.25, 2.0] {
            let context = fonts::test_context();
            context.set_pixels_per_point(density);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
            let mut time = 0.0;
            let mut render = |events: Vec<egui::Event>| {
                time += 0.05;
                let mut result = (egui::Rect::NOTHING, 0.0, false, false);
                let mut input = egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(time),
                    events,
                    ..Default::default()
                };
                wheel_input::prepare_native_input(&context, &mut input);
                let _ = context.run_ui(input, |ui| {
                    let opener = ui.button("Opener");
                    if ui.input(|input| input.pointer.primary_down()) {
                        context.set_dragged_id(opener.id);
                        context.data_mut(|data| {
                            data.insert_temp(
                                state_id(),
                                State {
                                    drag: Some(Drag {
                                        id: opener.id,
                                        popup: opener.id.with("popup"),
                                        origin: opener.rect.center(),
                                        source: (None, 0, 0),
                                        screen,
                                        density,
                                        crossed: true,
                                    }),
                                    section: Some(Section::View),
                                    ..Default::default()
                                },
                            )
                        });
                    }
                    let output = scroll(ui, egui::Id::new("test-menu"), 180.0, |ui| {
                        for index in 0..50 {
                            let response = ui.button(format!("Item {index}"));
                            result.2 |= hovered(&response);
                            result.3 |= clicked(&response);
                        }
                    });
                    result.0 = output.inner_rect;
                    result.1 = output.state.offset.y;
                });
                result
            };
            for _ in 0..3 {
                render(vec![]);
            }
            let origin = egui::pos2(20.0, 15.0);
            let press = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let bounds = render(vec![egui::Event::PointerMoved(origin), press(origin, true)]).0;
            let edge = egui::pos2(bounds.left() + 15.0, bounds.bottom() - 4.0);
            render(vec![egui::Event::PointerMoved(edge)]);
            for _ in 0..40 {
                let result = render(vec![]);
                assert_eq!(result.1, 0.0, "stationary edge must not scroll");
                assert!(!result.3);
            }
            let center = egui::pos2(bounds.left() + 15.0, bounds.top() + 10.0);
            let hovered = render(vec![egui::Event::PointerMoved(center)]);
            assert!(hovered.2, "menu row hover remains available during capture");
            render(vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -80.0),
                modifiers: egui::Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            }]);
            let mut offset = 0.0;
            for _ in 0..30 {
                offset = render(vec![]).1;
            }
            assert!(
                offset > 50.0,
                "ordinary wheel scrolls during capture: {offset}"
            );
            let result = render(vec![press(center, false)]);
            assert!(!result.3, "release is not a menu click");
        }
    }
}
