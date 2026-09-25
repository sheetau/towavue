use crate::localization::Text;
use crate::*;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Owner {
    tab: TabId,
    instance: u64,
    generation: PlaybackGeneration,
    selection: Option<towavue_core::TimeRange>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    SelectAll,
    DeselectAll,
    Delete,
    Crop,
    Silence,
    Play,
}

impl Action {
    fn command(self) -> Option<CommandId> {
        Some(match self {
            Self::SelectAll => CommandId::SelectAll,
            Self::DeselectAll => CommandId::ClearSelection,
            Self::Delete => CommandId::DeleteTimeSelection,
            Self::Crop => CommandId::KeepTimeSelection,
            Self::Play => CommandId::PlayTimeSelection,
            Self::Silence => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Intent {
    owner: Owner,
    action: Action,
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    fn timeline_menu_owner(&self) -> Option<Owner> {
        Some(Owner {
            tab: self.tabs.active_id()?,
            instance: self.media_generation,
            generation: self.generation,
            selection: self.time_selection,
        })
    }

    fn timeline_menu_enabled(&self, action: Action) -> bool {
        self.timeline_is_visible()
            && !self.modal_input_blocked()
            && !self.palette_open
            && !self.grid_open
            && !self.filmstrip_open
            && !matches!(self.state, PlaybackState::Loading | PlaybackState::Faulted)
            && self
                .playback_duration()
                .is_some_and(|duration| !duration.is_zero())
            && (matches!(action, Action::SelectAll | Action::Silence)
                || self.time_selection.is_some())
            && action.command().is_none_or(|command| {
                command_definitions().iter().any(|definition| {
                    definition.id == command && definition.is_enabled(self.command_context())
                })
            })
    }

    pub(super) fn draw_timeline_menu(
        &self,
        ui: &egui::Ui,
        response: &egui::Response,
        actions: &mut Vec<UiAction>,
    ) {
        let language = localization::language(ui.ctx());
        let popup = egui::Popup::default_response_id(response);
        let owner_id = popup.with("timeline-owner");
        let owner = self.timeline_menu_owner();
        if egui::Popup::is_id_open(ui.ctx(), popup)
            && (ui
                .ctx()
                .data(|data| data.get_temp::<Option<Owner>>(owner_id))
                != Some(owner)
                || self.modal_input_blocked()
                || self.palette_open
                || self.grid_open
                || self.filmstrip_open)
        {
            egui::Popup::close_id(ui.ctx(), popup);
            return;
        }
        if self.modal_input_blocked() || self.palette_open || self.grid_open || self.filmstrip_open
        {
            return;
        }
        // Child gain/endpoint controls also belong to this context. Observe only a
        // secondary press in our visible layer, without adding a primary-input overlay.
        let (pressed, position) = ui.input(|input| {
            (
                input.pointer.button_pressed(egui::PointerButton::Secondary),
                input.pointer.interact_pos(),
            )
        });
        let pointer_pressed = pressed
            && position.is_some_and(|position| {
                let layer = ui.ctx().layer_id_at(position);
                // egui retains a just-closed popup's hit geometry for one pass.
                // It must not block reopening the same owner after an action.
                let closed_popup = layer.is_some_and(|layer| layer.id == popup)
                    && !egui::Popup::is_id_open(ui.ctx(), popup);
                response.rect.intersect(ui.clip_rect()).contains(position)
                    && (layer == Some(ui.layer_id()) || closed_popup)
            });
        let chosen = tab_menu::popup_with_pointer(ui, response, response, pointer_pressed, |ui| {
            chrome::flat_buttons(ui);
            ui.set_min_width(170.0);
            let keyboard = menu::MenuKeyboard::begin(ui);
            let mut items = Vec::new();
            let mut chosen = None;
            for (action, title) in [
                (
                    Action::SelectAll,
                    Text::CommandSelectAll.in_language(language),
                ),
                (
                    Action::DeselectAll,
                    Text::CommandClearSelection.in_language(language),
                ),
                (Action::Delete, Text::TimelineDelete.in_language(language)),
                (Action::Crop, Text::TimelineCrop.in_language(language)),
                (Action::Silence, Text::TimelineSilence.in_language(language)),
                (Action::Play, Text::TimelinePlay.in_language(language)),
            ] {
                if matches!(action, Action::Delete | Action::Play) {
                    chrome::separator(ui);
                }
                let enabled = self.timeline_menu_enabled(action);
                let shortcut = action.command().map_or_else(String::new, |command| {
                    self.shortcuts.label(command, self.command_context())
                });
                let response = ui.add_enabled(
                    enabled,
                    egui::Button::new(title)
                        .shortcut_text(menu::shortcut_text(ui, shortcut, enabled)),
                );
                if enabled {
                    items.push(response.id);
                }
                if response.clicked() {
                    chosen = Some(action);
                    ui.close();
                }
            }
            keyboard.finish(ui, items);
            chosen
        });
        if egui::Popup::is_id_open(ui.ctx(), popup) {
            ui.ctx().data_mut(|data| data.insert_temp(owner_id, owner));
        } else {
            ui.ctx()
                .data_mut(|data| data.remove::<Option<Owner>>(owner_id));
        }
        if let (Some(owner), Some((action, _))) = (owner, chosen) {
            actions.push(UiAction::TimelineMenu(Intent { owner, action }));
        }
    }

    pub(super) fn handle_timeline_menu(&mut self, intent: Intent) {
        if self.timeline_menu_owner() != Some(intent.owner)
            || !self.timeline_menu_enabled(intent.action)
            || self
                .ui_context
                .as_ref()
                .is_some_and(egui::Popup::is_any_open)
        {
            return;
        }
        if let Some(command) = intent.action.command() {
            self.dispatch(command);
        } else if let Some(range) = intent.owner.selection.or_else(|| {
            towavue_core::TimeRange::new(MediaTime::ZERO, media_time(self.playback_duration()?))
        }) {
            self.handle_ui_action(UiAction::TimeAdjustment(
                intent.owner.tab,
                intent.owner.generation,
                intent.owner.selection,
                towavue_core::TimelineEdit::ScaleVolume(range, 0.0),
            ));
        }
    }
}
