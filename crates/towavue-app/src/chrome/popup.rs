use egui::{Context, Event, Id, PointerButton, Popup, Response, SetOpenCommand};

#[derive(Clone, Default)]
struct Toggle {
    frame: Option<u64>,
    suppress_release: bool,
    command: Option<SetOpenCommand>,
}

/// Closing on press prevents a parent modal from covering its open dropdown.
/// The matching release and discarded sizing passes must not reopen it.
pub fn menu_popup(response: &Response, popup: Id) -> Popup<'static> {
    let context: &Context = &response.ctx;
    crate::overlay_input::menu_opener_shown(context, popup, response.id);
    let id = popup.with("opener-toggle");
    let frame = context.cumulative_frame_nr();
    let mut state = context
        .data(|data| data.get_temp::<Toggle>(id))
        .unwrap_or_default();
    if state.frame != Some(frame) {
        state.frame = Some(frame);
        state.command = None;
        let mut suppress = state.suppress_release;
        let open = Popup::is_id_open(context, popup);
        let events = context.input(|input| input.events.clone());
        {
            for event in &events {
                match event {
                    Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed: true,
                        ..
                    } if open
                        && response.enabled()
                        && response.rect.contains(*pos)
                        && context.layer_id_at(*pos) == Some(response.layer_id) =>
                    {
                        state.command = Some(SetOpenCommand::Bool(false));
                        state.suppress_release = true;
                        suppress = true;
                    }
                    Event::PointerButton {
                        button: PointerButton::Primary,
                        pressed: false,
                        ..
                    }
                    | Event::WindowFocused(false)
                    | Event::PointerGone => {
                        state.suppress_release = false;
                    }
                    _ => {}
                }
            }
        }
        if response.clicked() && !suppress {
            state.command = Some(SetOpenCommand::Bool(!open));
        }
        context.data_mut(|data| data.insert_temp(id, state.clone()));
    }
    Popup::menu(response).id(popup).open_memory(state.command)
}
