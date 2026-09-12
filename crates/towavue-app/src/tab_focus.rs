use egui::{Context, Id, Response};
use std::collections::BTreeMap;
use towavue_core::TabId;

#[derive(Clone, Default)]
struct State {
    active: Option<TabId>,
    saved: BTreeMap<TabId, Id>,
    widgets: Vec<Id>,
    pending: Option<Id>,
    settled: bool,
    enabled: bool,
}

fn state_id() -> Id {
    Id::new("tab-media-focus")
}

pub(super) fn take(context: &Context, tab: TabId) -> Option<Id> {
    let key = context.data_mut(|data| {
        data.get_temp_mut_or_default::<State>(state_id())
            .saved
            .get(&tab)
            .copied()
    });
    forget(context, tab);
    key
}

pub(super) fn adopt(context: &Context, tab: TabId, key: Id) {
    context.data_mut(|data| {
        data.get_temp_mut_or_default::<State>(state_id())
            .saved
            .insert(tab, key)
    });
}

pub(super) fn forget(context: &Context, tab: TabId) {
    let focused = context.memory(|memory| memory.focused());
    let surrender = context.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<State>(state_id());
        state.saved.remove(&tab);
        if state.active == Some(tab) {
            state.pending = None;
        }
        state.active == Some(tab) && focused.is_some_and(|id| state.widgets.contains(&id))
    });
    if surrender && let Some(id) = focused {
        context.memory_mut(|memory| memory.surrender_focus(id));
    }
}

pub(super) fn begin(context: &Context, active: Option<TabId>, enabled: bool) {
    let focused = context.memory(|memory| memory.focused());
    let input = context.input(|input| {
        input.pointer.any_pressed()
            || input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key { pressed: true, .. } | egui::Event::AccessKitActionRequest(_)
                )
            })
    });
    let surrender = context.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<State>(state_id());
        let changed = state.active != active;
        let surrender = changed && focused.is_some_and(|id| state.widgets.contains(&id));
        if changed {
            state.settled = false;
            state.pending = (!input)
                .then(|| active.and_then(|tab| state.saved.get(&tab).copied()))
                .flatten();
        } else if input {
            state.pending = None;
        }
        state.active = active;
        state.enabled = enabled;
        state.widgets.clear();
        surrender
    });
    if surrender && let Some(id) = focused {
        context.memory_mut(|memory| memory.surrender_focus(id));
    }
}

pub(super) fn observe(response: &Response, key: impl std::hash::Hash + std::fmt::Debug) {
    if !response.enabled() || !response.interact_rect.is_positive() {
        return;
    }
    let context = &response.ctx;
    let focused = response.has_focus();
    let key = Id::new(key);
    let restore = context.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<State>(state_id());
        let Some(tab) = state.active.filter(|_| state.enabled) else {
            return false;
        };
        state.widgets.push(response.id);
        let restore = state.pending == Some(key);
        if restore {
            state.pending = None;
        }
        if focused || restore {
            state.saved.insert(tab, key);
        }
        restore
    });
    if restore {
        response.request_focus();
        context.request_repaint();
    }
}

pub(super) fn wants_controls(context: &Context) -> bool {
    let pending =
        context.data_mut(|data| data.get_temp_mut_or_default::<State>(state_id()).pending);
    pending.is_some_and(|key| {
        [
            Id::new(("media-button", crate::chrome::Icon::Play as u8)),
            Id::new(("media-button", crate::chrome::Icon::ExitFullscreen as u8)),
            Id::new("reading-mode"),
            Id::new((
                "media-value",
                Id::new("compact-seek-bar"),
                "Playback position (seconds)",
            )),
            Id::new(("media-value", Id::new("compact-seek-bar"), "Image position")),
        ]
        .contains(&key)
    })
}

#[cfg(test)]
pub(crate) mod tests;

pub(super) fn finish(context: &Context, loading: bool, tab_bar_visible: bool) {
    let (waiting, fallback) = context.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<State>(state_id());
        if !state.enabled || loading || state.pending.is_none() {
            return (false, false);
        }
        if state.settled {
            state.pending = None;
            (false, true)
        } else {
            // A newly visible Area first lays out disabled sizing widgets.
            state.settled = true;
            (true, false)
        }
    });
    if waiting {
        context.request_repaint();
    }
    if fallback && tab_bar_visible {
        context.data_mut(|data| data.insert_temp("filmstrip-return-tab".into(), true));
        context.request_repaint();
    }
}
