use crate::*;
use towavue_core::release::ReleaseVersion;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Action {
    Check,
    Install,
    NextLaunch,
    Cancel,
}

#[derive(Clone, Copy)]
pub(super) struct Notice {
    pub version: ReleaseVersion,
    pub failed: bool,
}

pub(super) struct Close {
    pub token: u64,
    pub approved: Option<BTreeMap<TabId, EditHistory>>,
    pub committing: bool,
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn update_save_token(&self) -> Option<u64> {
        let GuardedAction::UpdateExit(token) =
            self.active_export.as_ref()?.continuation.as_ref()?
        else {
            return None;
        };
        self.update_close
            .as_ref()
            .filter(|close| close.token == *token)
            .map(|close| close.token)
    }

    pub(super) fn update_is_held(&self) -> bool {
        self.update_close
            .as_ref()
            .is_some_and(|close| close.approved.is_some())
    }

    pub(super) fn update_can_prompt(&self) -> bool {
        !self.exit_requested
            && (self.window.is_none() || self.renderer.is_some())
            && !self.modal_input_blocked()
            && self.active_export.is_none()
            && !self.image_loading
            && !self.image_edit_pending
            && self.image_handoff.is_none()
            && self.state != PlaybackState::Loading
            && self.pending_tab_drop.is_none()
            && self.pending_window_open.is_none()
            && self.pending_window_launches.is_empty()
            && self.pending_guard.is_none()
    }

    pub(super) fn handle_update_action(&mut self, action: Action) {
        if action == Action::Cancel {
            if self
                .update_close
                .as_ref()
                .is_none_or(|close| close.committing)
            {
                return;
            }
            // Invalidate approval synchronously, before a queued helper-ready
            // event can be routed. The host cancels the other windows as a unit.
            self.update_close = None;
        } else if matches!(action, Action::Install | Action::NextLaunch)
            && self.update_notice.take().is_none()
        {
            return;
        }
        (self.notify)(AppEvent::Update(action));
        self.request_redraw();
    }

    pub(super) fn draw_update(&self, context: &egui::Context, actions: &mut Vec<UiAction>) {
        let held = self.update_is_held();
        let cancellable = self
            .update_close
            .as_ref()
            .is_some_and(|close| !close.committing);
        let modal = chrome::modal(context, "towavue-update".into(), false).show(context, |ui| {
            let buttons: &[&str] = if held {
                if cancellable { &["Cancel"] } else { &[] }
            } else {
                &["Install now", "Install on next launch"]
            };
            chrome::modal_body(ui, 400.0, "towavue update", buttons, |ui| {
                if held {
                    ui.label(if cancellable {
                        "Preparing update. Complete any save prompts in the other windows."
                    } else {
                        "Restarting to install the update…"
                    });
                } else if let Some(notice) = self.update_notice {
                    ui.label(format!("Version {} is downloaded and ready to install.", notice.version));
                    if notice.failed {
                        ui.label("The previous installation did not finish. Your documents are still available.");
                    }
                }
            });
            chrome::flat_buttons(ui);
            ui.horizontal(|ui| {
                if held {
                    if cancellable && ui.button("Cancel").clicked() {
                        actions.push(UiAction::Update(Action::Cancel));
                    }
                } else {
                    if ui.button("Install now").clicked() {
                        actions.push(UiAction::Update(Action::Install));
                    }
                    if ui.button("Install on next launch").clicked() {
                        actions.push(UiAction::Update(Action::NextLaunch));
                    }
                }
            });
        });
        if held
            && cancellable
            && modal.is_top_modal
            && !modal.any_popup_open
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            actions.push(UiAction::Update(Action::Cancel));
        }
    }
}
