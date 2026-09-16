use egui::{Context, Event, Popup};

#[cfg(test)]
mod tests;

/// Dismiss an existing menu before laying out the destination of an outside
/// press. Keep the original event so that destination owns its normal gesture.
pub fn dismiss_menu_on_outside_press(context: &Context) {
    if !Popup::is_any_open(context) {
        return;
    }
    let Some(position) = context.input(|input| {
        input.events.iter().find_map(|event| match event {
            Event::PointerButton {
                pos, pressed: true, ..
            } => Some(*pos),
            _ => None,
        })
    }) else {
        return;
    };
    let layers = context.memory(|memory| memory.layer_ids().collect::<Vec<_>>());
    let Some(root) = layers
        .into_iter()
        .find(|layer| Popup::is_id_open(context, layer.id))
    else {
        // A newly opened popup has no completed geometry yet.
        return;
    };
    // Let the opener perform its ordinary toggle, instead of closing now and
    // reopening it when the same button receives its release.
    let opener = context.interaction_snapshot(|snapshot| {
        snapshot
            .contains_pointer
            .iter()
            .copied()
            .collect::<Vec<_>>()
    });
    if opener
        .into_iter()
        .filter_map(|id| context.read_response(id))
        .any(|response| {
            response.rect.contains(position) && Popup::default_response_id(&response) == root.id
        })
    {
        return;
    }
    let mut menu = Some(root.id);
    let mut seen = Vec::new();
    while let Some(id) = menu {
        if seen.contains(&id) {
            return;
        }
        seen.push(id);
        let Some(rect) = context.memory(|memory| memory.area_rect(id)) else {
            return;
        };
        if rect.contains(position) {
            return;
        }
        menu = egui::containers::menu::MenuState::from_id(context, id, |state| state.open_item);
    }
    Popup::close_all(context);
}
