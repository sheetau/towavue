use crate::*;
use menu::Section;

type Source = (Option<TabId>, u64, u64);

#[derive(Clone, Copy)]
struct Drag {
    id: egui::Id,
    popup: egui::Id,
    origin: egui::Pos2,
    source: Source,
    screen: egui::Rect,
    density: f32,
    crossed: bool,
}

#[derive(Clone, Default)]
struct State {
    drag: Option<Drag>,
    claimed: Option<u64>,
    click_frame: Option<u64>,
    last_frame: u64,
    suppress: bool,
    anchor: Option<egui::Pos2>,
}

fn state_id() -> egui::Id {
    egui::Id::new("logo-menu-gesture")
}

pub(super) fn cancel(context: &egui::Context) -> bool {
    let active = context.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<State>(state_id());
        let active = state.drag.take();
        state.suppress |= active.is_some();
        active
    });
    if let Some(drag) = active {
        egui::Popup::close_id(context, drag.popup);
        if context.dragged_id() == Some(drag.id) {
            context.stop_dragging();
        }
    }
    active.is_some()
}

fn direction(delta: egui::Vec2) -> Option<Section> {
    if delta.length_sq() < 64.0 {
        return None;
    }
    if delta.x >= 0.0 {
        Some(if delta.y >= 0.0 {
            Section::Edit
        } else {
            Section::File
        })
    } else if delta.y >= 0.0 {
        Some(Section::View)
    } else {
        None
    }
}

pub(super) fn show(
    ui: &mut egui::Ui,
    commands: CommandContext,
    shortcuts: &ShortcutBindings,
    source: Source,
    allowed: bool,
) -> egui::InnerResponse<Option<Option<CommandId>>> {
    let response = ui.add(
        egui::Button::new("")
            .min_size(egui::vec2(28.0, chrome::TAB_HEIGHT))
            .stroke(egui::Stroke::NONE)
            .sense(egui::Sense::click_and_drag()),
    );
    let context = ui.ctx();
    let popup = egui::Popup::default_response_id(&response);
    let mut state = context
        .data(|data| data.get_temp::<State>(state_id()))
        .unwrap_or_default();
    let frame = context.cumulative_frame_nr();
    let (events, focused, pointer, down) = ui.input(|input| {
        (
            input.events.clone(),
            input.focused,
            input.pointer.hover_pos(),
            input.pointer.primary_down(),
        )
    });
    let interrupted = !allowed
        || !response.enabled()
        || !focused
        || events.iter().any(|event| {
            matches!(
                event,
                egui::Event::PointerGone
                    | egui::Event::WindowFocused(false)
                    | egui::Event::Key { pressed: true, .. }
            )
        })
        || (egui::Popup::is_any_open(context) && !egui::Popup::is_id_open(context, popup));
    if state.drag.is_some_and(|drag| {
        interrupted
            || drag.source != source
            || drag.screen != context.content_rect()
            || drag.density != context.pixels_per_point()
            || state.last_frame + 1 < frame
            || context.dragged_id().is_some_and(|id| id != response.id)
    }) {
        state.drag = None;
        state.suppress = true;
        egui::Popup::close_id(context, popup);
        if context.dragged_id() == Some(response.id) {
            context.stop_dragging();
        }
        ui.input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
        });
        response.request_focus();
    }
    let mut open = None;
    let mut pointer_event = false;
    for event in &events {
        match event {
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                ..
            } => {
                pointer_event = true;
                if *pressed
                    && !interrupted
                    && state.claimed != Some(frame)
                    && response.interact_rect.contains(*pos)
                    && context.layer_id_at(*pos) == Some(response.layer_id)
                    && context.dragged_id().is_none_or(|id| id == response.id)
                {
                    state.drag = Some(Drag {
                        id: response.id,
                        popup,
                        origin: *pos,
                        source,
                        screen: context.content_rect(),
                        density: context.pixels_per_point(),
                        crossed: false,
                    });
                    state.claimed = Some(frame);
                    state.suppress = false;
                    response.request_focus();
                } else if !pressed && let Some(mut drag) = state.drag.take() {
                    drag.crossed |= (*pos - drag.origin).length_sq() >= 64.0;
                    state.suppress |= drag.crossed;
                    if !drag.crossed && response.interact_rect.contains(*pos) {
                        state.click_frame = Some(frame);
                    }
                    open = direction(*pos - drag.origin)
                        .filter(|_| drag.crossed && context.content_rect().contains(*pos));
                    state.anchor = open.map(|_| *pos + egui::vec2(8.0, 8.0));
                }
            }
            egui::Event::PointerMoved(pos) => {
                if let Some(drag) = &mut state.drag {
                    drag.crossed |= (*pos - drag.origin).length_sq() >= 64.0;
                }
            }
            _ => {}
        }
    }
    let mut selected = None;
    if let Some(drag) = &state.drag {
        selected = pointer.and_then(|position| direction(position - drag.origin));
        if drag.crossed {
            state.suppress = true;
            egui::Popup::close_id(context, popup);
        }
        if !down || pointer.is_none() {
            state.drag = None;
            state.suppress = true;
            selected = None;
        }
    }
    state.last_frame = frame;
    let suppress = (state.suppress && (pointer_event || down))
        || (pointer_event && state.click_frame != Some(frame));
    if (response.clicked() && !suppress)
        || (open.is_none() && !egui::Popup::is_id_open(context, popup))
    {
        state.anchor = None;
    }
    let anchor = state.anchor;
    context.data_mut(|data| data.insert_temp(state_id(), state));
    let shift = context.animate_value_with_time(
        response.id.with("shaft-shift"),
        if selected.is_some() { 1.0 } else { 0.0 },
        0.08,
    );
    chrome::logo(ui, response.rect, selected, shift);
    let set_open = if open.is_some() {
        Some(egui::SetOpenCommand::Bool(true))
    } else if response.clicked() && !suppress {
        Some(egui::SetOpenCommand::Toggle)
    } else {
        None
    };
    let mut popup = egui::Popup::menu(&response).open_memory(set_open);
    if open.is_some() {
        // A sparse/batched drag release must not immediately close the newly opened menu.
        let config = egui::containers::menu::MenuConfig::new()
            .close_behavior(egui::PopupCloseBehavior::IgnoreClicks);
        popup = popup
            .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
            .info(
                egui::UiStackInfo::new(egui::UiKind::Menu)
                    .with_tag_value(egui::containers::menu::MenuConfig::MENU_CONFIG_TAG, config),
            );
    }
    if let Some(anchor) = anchor {
        popup = popup.at_position(anchor);
    }
    let inner = popup.show(|ui| menu::show_section(ui, commands, shortcuts, open));
    egui::InnerResponse {
        response,
        inner: inner.map(|inner| inner.inner),
    }
}

#[cfg(test)]
pub(crate) mod tests;
