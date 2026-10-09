use egui::{Event, PointerButton, Popup, PopupCloseBehavior, Response, SetOpenCommand};

#[derive(Clone, Default)]
struct Gesture {
    held: bool,
    frame: Option<u64>,
    open: bool,
    release: Option<egui::Pos2>,
    consumed: bool,
}

/// A primary press opens the menu; its matching release may choose a row.
/// Retain the frame decision across egui sizing passes without replaying it.
pub(super) fn show(
    response: &Response,
    content: impl FnOnce(&mut egui::Ui, &mut dyn FnMut(&Response) -> bool),
) {
    let context = &response.ctx;
    let id = response.id.with("filter-gesture");
    let frame = context.cumulative_frame_nr();
    let mut gesture = context
        .data(|data| data.get_temp::<Gesture>(id))
        .unwrap_or_default();
    if gesture.frame != Some(frame) {
        gesture.frame = Some(frame);
        gesture.open = false;
        gesture.release = None;
        gesture.consumed = false;
        let (focused, events) = context.input(|input| (input.focused, input.events.clone()));
        if !focused || !response.enabled() {
            gesture.held = false;
        } else {
            for event in &events {
                match event {
                    Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed: true,
                        ..
                    } if response.rect.contains(*pos)
                        && context.layer_id_at(*pos) == Some(response.layer_id) =>
                    {
                        gesture.held = true;
                        gesture.open = true;
                    }
                    Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed: false,
                        ..
                    } if gesture.held => {
                        gesture.held = false;
                        gesture.release = Some(*pos);
                        break;
                    }
                    Event::PointerGone
                    | Event::WindowFocused(false)
                    | Event::Key { pressed: true, .. } => {
                        gesture.held = false;
                    }
                    _ => {}
                }
            }
        }
    }
    let owned = gesture.held || gesture.release.is_some() || gesture.open;
    let command = if gesture.open && !gesture.consumed {
        Some(SetOpenCommand::Bool(true))
    } else if !owned && response.clicked() {
        Some(SetOpenCommand::Toggle)
    } else {
        None
    };
    let popup = Popup::menu(response)
        .open_memory(command)
        .close_behavior(if owned {
            PopupCloseBehavior::IgnoreClicks
        } else {
            PopupCloseBehavior::CloseOnClick
        })
        .style(crate::chrome::menu_style)
        .show(|ui| {
            content(ui, &mut |row: &Response| {
                // The opener owns the held press, so egui suppresses ordinary
                // hover on rows. Keep its normal menu visual under the pointer.
                let hovered = gesture.held && row.enabled() && row.contains_pointer();
                if hovered {
                    context.highlight_widget(row.id);
                }
                if hovered != row.highlighted() {
                    context.request_discard("Gallery filter drag hover changed");
                }
                let released = gesture.release.is_some_and(|pos| {
                    row.rect.contains(pos) && context.layer_id_at(pos) == Some(row.layer_id)
                });
                let selected = !gesture.consumed && row.enabled() && (row.clicked() || released);
                gesture.consumed |= selected;
                selected
            });
        });
    if let (Some(pos), Some(popup)) = (gesture.release, popup)
        && !popup.response.rect.contains(pos)
        && !response.rect.contains(pos)
    {
        Popup::close_id(context, Popup::default_response_id(response));
    }
    context.data_mut(|data| data.insert_temp(id, gesture));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer(pos: egui::Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn press_opens_release_selects_and_cancellation_or_disabled_rows_do_not() {
        for cancel in [false, true] {
            for enabled in [false, true] {
                let context = crate::fonts::test_context();
                let opener = std::cell::Cell::new(egui::Rect::NOTHING);
                let target = std::cell::Cell::new(egui::Rect::NOTHING);
                let choices = std::cell::Cell::new(0);
                let highlighted = std::cell::Cell::new(false);
                let frame = |events| {
                    context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(480.0, 400.0),
                            )),
                            focused: true,
                            events,
                            ..Default::default()
                        },
                        |ui| {
                            let response = ui.button("Filter");
                            opener.set(response.rect);
                            show(&response, |ui, selected| {
                                let row = ui.add_enabled(enabled, egui::Button::new("Images"));
                                target.set(row.rect);
                                highlighted.set(row.highlighted());
                                if selected(&row) {
                                    choices.set(choices.get() + 1);
                                    ui.close();
                                }
                            });
                        },
                    )
                };
                frame(vec![]);
                frame(vec![
                    Event::PointerMoved(opener.get().center()),
                    pointer(opener.get().center(), true),
                ]);
                assert!(Popup::is_any_open(&context), "opens on press");
                for _ in 0..3 {
                    frame(vec![]);
                }
                assert_eq!(choices.get(), 0);
                frame(vec![Event::PointerMoved(target.get().center())]);
                assert_eq!(
                    highlighted.get(),
                    enabled,
                    "held press highlights the target"
                );
                frame(vec![Event::PointerMoved(egui::pos2(450.0, 350.0))]);
                assert!(!highlighted.get(), "moving away clears the highlight");
                frame(vec![Event::PointerMoved(target.get().center())]);
                if cancel {
                    frame(vec![Event::PointerGone]);
                }
                frame(vec![pointer(target.get().center(), false)]);
                assert_eq!(choices.get(), usize::from(enabled && !cancel));
                for _ in 0..3 {
                    frame(vec![]);
                }
                assert_eq!(choices.get(), usize::from(enabled && !cancel), "no replay");
                // A normal click on the opener leaves the menu open for a later click.
                Popup::close_all(&context);
                frame(vec![]);
                frame(vec![
                    Event::PointerMoved(opener.get().center()),
                    pointer(opener.get().center(), true),
                ]);
                frame(vec![pointer(opener.get().center(), false)]);
                assert!(Popup::is_any_open(&context));
                for _ in 0..3 {
                    frame(vec![]);
                }
                frame(vec![
                    Event::PointerMoved(target.get().center()),
                    pointer(target.get().center(), true),
                ]);
                frame(vec![pointer(target.get().center(), false)]);
                assert_eq!(
                    choices.get(),
                    usize::from(enabled && !cancel) + usize::from(enabled)
                );
            }
        }
    }
}
