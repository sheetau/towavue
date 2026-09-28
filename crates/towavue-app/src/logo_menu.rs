use crate::*;
use menu::Section;

pub(crate) mod drag;

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
    left_button: bool,
    section: Option<Section>,
}

#[derive(Clone, Default)]
struct State {
    drag: Option<Drag>,
    claimed: Option<u64>,
    processed: Option<u64>,
    release: Option<(u64, egui::Pos2)>,
    click_frame: Option<u64>,
    last_frame: u64,
    suppress: bool,
    section: Option<Section>,
    keyboard_origin: bool,
}

fn state_id() -> egui::Id {
    egui::Id::new("logo-menu-gesture")
}

pub(super) fn cancel(context: &egui::Context) -> bool {
    let active = context.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<State>(state_id());
        let active = state.drag.take();
        state.release = None;
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
    let angle = delta.x.atan2(-delta.y).to_degrees().rem_euclid(360.0);
    if angle < 112.5 {
        Some(Section::File)
    } else if angle < 157.5 {
        Some(Section::Edit)
    } else if angle <= 270.0 {
        Some(Section::View)
    } else {
        None
    }
}

pub(super) fn show_with_recent(
    ui: &mut egui::Ui,
    height: f32,
    commands: CommandContext,
    shortcuts: &ShortcutBindings,
    source: Source,
    allowed: bool,
    recent: &mut menu::MenuData<'_>,
) -> egui::InnerResponse<Option<Option<CommandId>>> {
    let response = ui.add_enabled(
        allowed,
        egui::Button::new("")
            .min_size(egui::vec2(28.0, height))
            .stroke(egui::Stroke::NONE)
            .frame(false)
            .sense(egui::Sense::click_and_drag()),
    );
    let context = ui.ctx();
    let popup = egui::Popup::default_response_id(&response);
    let was_open = egui::Popup::is_id_open(context, popup);
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
                egui::Event::WindowFocused(false) | egui::Event::Key { pressed: true, .. }
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
        state.release = None;
        state.suppress = true;
        egui::Popup::close_id(context, popup);
        if context.dragged_id() == Some(response.id) {
            context.stop_dragging();
        }
        ui.input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
        });
        response.surrender_focus();
    }
    let mut open = None;
    let pointer_event = events
        .iter()
        .any(|event| matches!(event, egui::Event::PointerButton { .. }));
    if state.processed != Some(frame) {
        state.processed = Some(frame);
        state.release = None;
        if state.drag.is_some() {
            // An already owned gesture consumes this batch, including any second press.
            state.claimed = Some(frame);
        }
        for event in &events {
            match event {
                egui::Event::PointerGone if state.release.is_none() => {
                    if state.drag.take().is_some() {
                        state.suppress = true;
                        state.section = None;
                        open = None;
                        egui::Popup::close_id(context, popup);
                        context.stop_dragging();
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } if !interrupted
                    && state.claimed != Some(frame)
                    && response.interact_rect.contains(*pos)
                    && context.layer_id_at(*pos) == Some(response.layer_id)
                    && context.dragged_id().is_none_or(|id| id == response.id) =>
                {
                    state.drag = Some(Drag {
                        id: response.id,
                        popup,
                        origin: *pos,
                        source,
                        screen: context.content_rect(),
                        density: context.pixels_per_point(),
                        crossed: false,
                        left_button: false,
                        section: None,
                    });
                    state.keyboard_origin = false;
                    state.claimed = Some(frame);
                    state.suppress = false;
                    response.surrender_focus();
                }
                egui::Event::PointerMoved(pos)
                | egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } => {
                    let released = matches!(event, egui::Event::PointerButton { .. });
                    if let Some(owned) = &mut state.drag {
                        if !response.interact_rect.contains(*pos) {
                            owned.left_button = true;
                        }
                        if owned.section.is_some()
                            && owned.left_button
                            && response.interact_rect.contains(*pos)
                        {
                            // Reentering the opener re-arms directional selection, without
                            // reinterpreting ordinary movement among menu rows.
                            owned.section = None;
                            owned.left_button = false;
                            owned.origin = *pos;
                            state.section = None;
                            open = None;
                            egui::Popup::close_id(context, popup);
                        } else if owned.section.is_none()
                            && let Some(section) = direction(*pos - owned.origin)
                            && context.content_rect().contains(*pos)
                        {
                            owned.crossed = true;
                            owned.section = Some(section);
                            state.section = Some(section);
                            open = Some(section);
                        }
                        owned.crossed |= (*pos - owned.origin).length_sq() >= 64.0;
                        state.suppress |= owned.crossed;
                        if released {
                            if owned.crossed {
                                state.release = Some((frame, *pos));
                            } else if response.interact_rect.contains(*pos) {
                                state.click_frame = Some(frame);
                            }
                            state.drag = None;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let selected = state.drag.and_then(|drag| drag.section);
    if state.drag.is_some() {
        if !down || pointer.is_none() {
            state.drag = None;
            state.suppress = true;
            egui::Popup::close_id(context, popup);
        } else {
            // Keep the original owner even when egui sees only the final point
            // of a batched press/move; leaves receive the release explicitly.
            context.set_dragged_id(response.id);
        }
    }
    state.last_frame = frame;
    let suppress = (state.suppress && (pointer_event || down))
        || (pointer_event && state.click_frame != Some(frame));
    if (response.clicked() && !suppress)
        || (open.is_none() && !egui::Popup::is_id_open(context, popup))
    {
        state.section = None;
    }
    let section = state.section;
    if response.clicked() && !pointer_event && !suppress {
        state.keyboard_origin = true;
    }
    let shift = context.animate_value_with_time(
        response.id.with("shaft-shift"),
        if selected.is_some() { 1.0 } else { 0.0 },
        0.08,
    );
    chrome::logo(
        ui,
        response.rect,
        selected,
        shift,
        response.hovered() || response.has_focus() || egui::Popup::is_id_open(context, popup),
    );
    let set_open = if open.is_some() {
        Some(egui::SetOpenCommand::Bool(true))
    } else if response.clicked() && !suppress {
        Some(egui::SetOpenCommand::Toggle)
    } else {
        None
    };
    context.data_mut(|data| data.insert_temp(state_id(), state.clone()));
    let mut popup_ui = egui::Popup::menu(&response).open_memory(set_open);
    if state.drag.is_some() || state.release.is_some_and(|(at, _)| at == frame) {
        // The captured release belongs to the hit-tested leaf, not popup click dismissal.
        let config = egui::containers::menu::MenuConfig::new()
            .close_behavior(egui::PopupCloseBehavior::IgnoreClicks);
        popup_ui = popup_ui
            .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
            .info(
                egui::UiStackInfo::new(egui::UiKind::Menu)
                    .with_tag_value(egui::containers::menu::MenuConfig::MENU_CONFIG_TAG, config),
            );
    }
    let inner = popup_ui.show(|ui| {
        let mut builder = egui::UiBuilder::new();
        if set_open.is_some() {
            // This popup alternates between categories and direct submenus.
            let size = context.content_rect().size();
            ui.set_max_size(
                (size - egui::Frame::popup(ui.style()).total_margin().sum()).max(egui::Vec2::ZERO),
            );
            builder = builder.sizing_pass();
        }
        ui.scope_builder(builder, |ui| {
            menu::show_section_with_recent(ui, commands, shortcuts, section, recent)
        })
        .inner
    });
    if state.release.is_some_and(|(at, _)| at == frame) {
        egui::Popup::close_id(context, popup);
        state.section = None;
        context.stop_dragging();
        response.surrender_focus();
    }
    // Only explicit keyboard/accessibility entry returns focus to the opener.
    // Pointer gestures already paint held/open feedback without taking key focus.
    if was_open
        && !egui::Popup::is_id_open(context, egui::Popup::default_response_id(&response))
        && !egui::Popup::is_any_open(context)
        && state.keyboard_origin
        && (events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                }
            )
        }) || inner.as_ref().is_some_and(|inner| inner.inner.is_some()))
    {
        response.request_focus();
    }
    context.data_mut(|data| data.insert_temp(state_id(), state));
    egui::InnerResponse {
        response,
        inner: inner.map(|inner| inner.inner),
    }
}

#[cfg(test)]
pub(crate) mod tests;
