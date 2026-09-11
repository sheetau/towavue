use egui::accesskit::{Action, ActionData, Orientation, TreeId};
use egui::{Id, Rect, Ui};
use towavue_core::{MediaKind, PixelCrop, UnitRect};

pub fn has_focus(context: &egui::Context) -> bool {
    context
        .data(|data| data.get_temp::<Id>(Id::new("selection-value-focus")))
        .is_some_and(|id| context.memory(|memory| memory.has_focus(id)))
}

pub fn focus_hint(context: &egui::Context) -> Option<String> {
    has_focus(context)
        .then(|| {
            context.data(|data| data.get_temp::<String>(Id::new("selection-value-description")))
        })
        .flatten()
}

pub fn release_focus(context: &egui::Context) {
    if let Some(id) = context.data(|data| data.get_temp::<Id>(Id::new("selection-value-focus"))) {
        context.memory_mut(|memory| memory.surrender_focus(id));
    }
}

#[cfg(test)]
#[test]
fn selection_paints_only_the_inverted_outline_and_reports_edge_focus_in_status() {
    let context = crate::fonts::test_context();
    let identity = Id::new("outline-focus-test");
    let selected = UnitRect {
        min: towavue_core::UnitPoint { x: 0.25, y: 0.25 },
        max: towavue_core::UnitPoint { x: 0.75, y: 0.75 },
    };
    context.memory_mut(|memory| memory.request_focus(identity.with(0_usize)));
    let output = context.run_ui(egui::RawInput::default(), |ui| {
        let rect = Rect::from_min_max(egui::pos2(20.0, 20.0), egui::pos2(420.0, 220.0));
        crate::paint_selection(ui.painter(), rect, selected);
        controls(
            ui,
            identity,
            rect,
            selected,
            (400, 200),
            MediaKind::Image,
            true,
        );
    });
    assert_eq!(
        output.shapes.len(),
        1,
        "no shade, handles or extra focus rectangle"
    );
    assert!(matches!(output.shapes[0].shape, egui::Shape::Callback(_)));
    assert!(
        focus_hint(&context)
            .expect("focused edge description")
            .contains("Selection left (pixels): 100")
    );
    context.memory_mut(|memory| memory.surrender_focus(identity.with(0_usize)));
    assert!(focus_hint(&context).is_none());
}

pub fn focus_first(context: &egui::Context, identity: Id) {
    context.memory_mut(|memory| memory.request_focus(identity.with(0_usize)));
    context.data_mut(|data| data.insert_temp(identity.with("reveal"), 0_usize));
}

pub fn reveal_offset(
    context: &egui::Context,
    identity: Id,
    viewport: Rect,
    image: Rect,
    selection: UnitRect,
) -> egui::Vec2 {
    let requested = context.data_mut(|data| data.remove_temp::<usize>(identity.with("reveal")));
    let Some(index) =
        requested.filter(|index| context.memory(|memory| memory.has_focus(identity.with(*index))))
    else {
        return egui::Vec2::ZERO;
    };
    if context.input(|input| input.pointer.any_down()) {
        return egui::Vec2::ZERO;
    }
    let selected = crate::selection_rect(image, selection);
    let center = [
        selected.left_center(),
        selected.right_center(),
        selected.center_top(),
        selected.center_bottom(),
    ][index];
    let inset = egui::vec2(
        (viewport.width() * 0.5).min(10.0),
        (viewport.height() * 0.5).min(10.0),
    );
    let visible = viewport.shrink2(inset);
    center.clamp(visible.min, visible.max) - center
}

pub fn controls(
    ui: &mut Ui,
    identity: Id,
    rect: Rect,
    selection: UnitRect,
    size: (u32, u32),
    kind: MediaKind,
    enabled: bool,
) -> (Option<UnitRect>, bool) {
    let Some(crop) = PixelCrop::from_selection(selection, size, kind) else {
        return (None, false);
    };
    let step = if kind == MediaKind::Video { 2 } else { 1 };
    let values = [crop.x, crop.x + crop.width, crop.y, crop.y + crop.height];
    let limits = [
        size.0 / step * step,
        size.0 / step * step,
        size.1 / step * step,
        size.1 / step * step,
    ];
    let selected = crate::selection_rect(rect, selection);
    let centers = [
        selected.left_center(),
        selected.right_center(),
        selected.center_top(),
        selected.center_bottom(),
    ];
    let labels = [
        "Selection left (pixels)",
        "Selection right (pixels)",
        "Selection top (pixels)",
        "Selection bottom (pixels)",
    ];
    let enabled = enabled && ui.is_enabled() && !egui::Popup::is_any_open(ui.ctx());
    let requested_focus = ui.input(|input| {
        input.events.iter().rev().find_map(|event| {
            if let egui::Event::AccessKitActionRequest(request) = event
                && request.target_tree == TreeId::ROOT
                && request.action == Action::Focus
            {
                (0..4_usize)
                    .find(|index| identity.with(*index).accesskit_id() == request.target_node)
            } else {
                None
            }
        })
    });
    let responses = ui
        .add_enabled_ui(enabled, |ui| {
            (0..4)
                .map(|index| {
                    let mut control_rect =
                        Rect::from_center_size(centers[index], egui::vec2(14.0, 14.0));
                    if kind == MediaKind::Image && control_rect.intersects(ui.clip_rect()) {
                        control_rect = control_rect.intersect(ui.clip_rect());
                    }
                    let response = ui.interact(
                        control_rect,
                        identity.with(index),
                        egui::Sense::focusable_noninteractive(),
                    );
                    crate::tab_focus::observe(&response, ("selection-edge", index));
                    response.widget_info(|| {
                        egui::WidgetInfo::slider(enabled, f64::from(values[index]), labels[index])
                    });
                    ui.ctx().accesskit_node_builder(response.id, |node| {
                        node.set_orientation(if index < 2 {
                            Orientation::Horizontal
                        } else {
                            Orientation::Vertical
                        });
                        node.set_min_numeric_value(0.0);
                        node.set_max_numeric_value(f64::from(limits[index]));
                        node.set_numeric_value_step(f64::from(step));
                        if enabled {
                            node.add_action(Action::SetValue);
                            if values[index] < limits[index] {
                                node.add_action(Action::Increment);
                            }
                            if values[index] > 0 {
                                node.add_action(Action::Decrement);
                            }
                        }
                    });
                    if response.has_focus() && enabled {
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(Id::new("selection-value-focus"), response.id);
                            data.insert_temp(
                                Id::new("selection-value-description"),
                                format!("{}: {} · Arrow keys adjust", labels[index], values[index]),
                            );
                        });
                        ui.ctx().memory_mut(|memory| {
                            memory.set_focus_lock_filter(
                                response.id,
                                egui::EventFilter {
                                    horizontal_arrows: true,
                                    vertical_arrows: true,
                                    ..Default::default()
                                },
                            )
                        });
                    }
                    response
                })
                .collect::<Vec<_>>()
        })
        .inner;
    if !enabled {
        ui.ctx()
            .data_mut(|data| data.remove_temp::<usize>(identity.with("reveal")));
        return (None, false);
    }
    let mut pending = values;
    let mut invalid = false;
    let mut keyboard = false;
    let focused = responses.iter().position(egui::Response::has_focus);
    // The next edge request must see earlier accepted changes, regardless of drawing order.
    ui.ctx().input_mut(|input| {
        input.events.retain(|event| {
            let (index, value) = match event {
                egui::Event::AccessKitActionRequest(request)
                    if request.target_tree == TreeId::ROOT =>
                {
                    let Some(index) = responses
                        .iter()
                        .position(|r| r.id.accesskit_id() == request.target_node)
                    else {
                        return true;
                    };
                    let value = match (&request.action, &request.data) {
                        (Action::SetValue, Some(ActionData::NumericValue(value)))
                            if value.is_finite() =>
                        {
                            *value
                        }
                        (Action::Increment, _) => f64::from(pending[index]) + f64::from(step),
                        (Action::Decrement, _) => f64::from(pending[index]) - f64::from(step),
                        _ => return true,
                    };
                    (index, value)
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if *modifiers == egui::Modifiers::NONE => {
                    let Some(index) = focused else {
                        return true;
                    };
                    let value = match key {
                        egui::Key::ArrowLeft | egui::Key::ArrowUp => {
                            f64::from(pending[index]) - f64::from(step)
                        }
                        egui::Key::ArrowRight | egui::Key::ArrowDown => {
                            f64::from(pending[index]) + f64::from(step)
                        }
                        egui::Key::Home => 0.0,
                        egui::Key::End => f64::from(limits[index]),
                        _ => return true,
                    };
                    keyboard = true;
                    (index, value)
                }
                _ => return true,
            };
            let mut candidate = pending;
            candidate[index] = ((value.clamp(0.0, f64::from(limits[index])) / f64::from(step))
                .round() as u32)
                * step;
            if candidate[0] < candidate[1] && candidate[2] < candidate[3] {
                pending = candidate;
            } else {
                invalid = true;
            }
            false
        })
    });
    if keyboard {
        ui.ctx()
            .memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
    }
    let changed = (pending != values).then(|| {
        PixelCrop {
            x: pending[0],
            y: pending[2],
            width: pending[1] - pending[0],
            height: pending[3] - pending[2],
        }
        .unit_rect(size)
    });
    if let Some(index) = focused
        && (responses[index].gained_focus() || requested_focus == Some(index) || changed.is_some())
    {
        ui.ctx()
            .data_mut(|data| data.insert_temp(identity.with("reveal"), index));
    }
    (changed, invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_is_single_use_and_yields_to_disabled_controls_and_pointer_buttons() {
        let identity = Id::new("reveal image");
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 60.0));
        let image = Rect::from_min_size(egui::pos2(-100.0, -50.0), egui::vec2(300.0, 160.0));
        for (enabled, held, expected) in [
            (true, false, egui::vec2(110.0, 0.0)),
            (false, false, egui::Vec2::ZERO),
            (true, true, egui::Vec2::ZERO),
        ] {
            let context = egui::Context::default();
            let _ = context.run_ui(
                egui::RawInput {
                    events: if held {
                        vec![egui::Event::PointerButton {
                            pos: egui::pos2(50.0, 30.0),
                            button: egui::PointerButton::Secondary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        }]
                    } else {
                        vec![]
                    },
                    ..Default::default()
                },
                |ui| {
                    focus_first(&context, identity);
                    controls(
                        ui,
                        identity,
                        image,
                        UnitRect::FULL,
                        (300, 160),
                        MediaKind::Image,
                        enabled,
                    );
                    assert_eq!(
                        reveal_offset(&context, identity, viewport, image, UnitRect::FULL),
                        expected
                    );
                    assert_eq!(
                        reveal_offset(&context, identity, viewport, image, UnitRect::FULL),
                        egui::Vec2::ZERO
                    );
                },
            );
        }
    }

    #[test]
    fn video_edges_snap_to_even_pixels_and_do_not_retarget_another_media() {
        let context = egui::Context::default();
        context.enable_accesskit();
        let identity = Id::new("video");
        let frame = |identity, selected, events| {
            let mut result = (None, false);
            let output = context.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    result = controls(
                        ui,
                        identity,
                        Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(400.0, 200.0)),
                        selected,
                        (401, 201),
                        MediaKind::Video,
                        true,
                    );
                },
            );
            (
                result,
                output.platform_output.accesskit_update.expect("tree"),
            )
        };
        let (_, tree) = frame(identity, UnitRect::FULL, vec![]);
        let ids: Vec<_> = tree
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == egui::accesskit::Role::Slider)
            .map(|(id, node)| {
                assert_eq!(node.numeric_value_step(), Some(2.0));
                *id
            })
            .collect();
        assert_eq!(ids.len(), 4);
        let request = |index, value| {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action: Action::SetValue,
                target_tree: TreeId::ROOT,
                target_node: ids[index],
                data: Some(ActionData::NumericValue(value)),
            })
        };
        let ((selected, invalid), _) = frame(
            identity,
            UnitRect::FULL,
            vec![
                request(0, 3.0),
                request(1, 99.0),
                request(2, 5.0),
                request(3, 101.0),
            ],
        );
        assert!(!invalid);
        let selected = selected.expect("selection");
        assert_eq!(
            PixelCrop::from_selection(selected, (401, 201), MediaKind::Video),
            Some(PixelCrop {
                x: 4,
                y: 6,
                width: 96,
                height: 96
            })
        );
        let ((changed, invalid), _) =
            frame(identity, selected, vec![request(0, 100.0), request(3, 6.0)]);
        assert!(changed.is_none());
        assert!(invalid);
        assert_eq!(
            frame(
                Id::new("other video"),
                UnitRect::FULL,
                vec![request(0, 20.0)]
            )
            .0,
            (None, false)
        );
    }
}
