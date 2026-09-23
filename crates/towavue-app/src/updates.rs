use crate::*;
use towavue_core::release::ReleaseVersion;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Action {
    Check,
    Install,
    NextLaunch,
    Cancel,
    Dismiss,
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
        } else if matches!(
            action,
            Action::Install | Action::NextLaunch | Action::Dismiss
        ) && self.update_notice.is_none()
        {
            return;
        }
        (self.notify)(AppEvent::Update(action));
        self.request_redraw();
    }

    fn update_status_enabled(&self) -> bool {
        self.update_close.as_ref().is_some_and(|close| {
            self.update_is_held()
                && !self.dialog_input_blocked_for_update_save(Some(close.token))
                && self.pending_guard.is_none()
                && self.active_export.is_none()
        })
    }

    pub(super) fn update_cancel_key(&self, context: &egui::Context, actions: &mut Vec<UiAction>) {
        if self.update_status_enabled()
            && self
                .update_close
                .as_ref()
                .is_some_and(|close| !close.committing)
            && !egui::Popup::is_any_open(context)
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            actions.push(UiAction::Update(Action::Cancel));
        }
    }

    pub(super) fn draw_update_status(&self, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        let Some(close) = &self.update_close else {
            return;
        };
        let message = if close.committing {
            "Restarting to install the update…"
        } else {
            "Preparing update. Complete any save prompts in the other windows."
        };
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::hover());
        // The update hold blocks media controls. This separate nonmodal area
        // retains Cancel, just like save progress; another dialog still blocks it.
        let area = egui::Area::new("update-status".into());
        if ui.layer_id().order == egui::Order::Middle {
            ui.ctx().set_sublayer(ui.layer_id(), area.layer());
        }
        area.order(egui::Order::Middle)
            .fixed_pos(rect.min)
            .default_size(rect.size())
            .movable(false)
            .constrain(false)
            .enabled(self.update_status_enabled())
            .show(ui.ctx(), |ui| {
                ui.set_width(rect.width());
                ui.set_height(rect.height());
                ui.set_clip_rect(rect.intersect(ui.ctx().content_rect()));
                chrome::flat_buttons(ui);
                ui.spacing_mut().item_spacing.x = chrome::STATUS_BUTTON_GAP;
                ui.spacing_mut().button_padding = egui::Vec2::ZERO;
                ui.horizontal_centered(|ui| {
                    if !close.committing {
                        let cancel = chrome::status_button(
                            ui,
                            egui::vec2(54.0, chrome::STATUS_BUTTON_SIZE),
                            egui::Button::new("")
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE),
                        );
                        let background = if cancel.hovered() || cancel.is_pointer_button_down_on() {
                            chrome::HOVER
                        } else {
                            chrome::SURFACE_HOVER
                        };
                        ui.painter().rect_filled(cancel.rect, 2.0, background);
                        ui.painter().text(
                            cancel.rect.center()
                                - egui::vec2(0.0, 1.0 / ui.ctx().pixels_per_point()),
                            egui::Align2::CENTER_CENTER,
                            "Cancel",
                            egui::FontId::proportional(12.0),
                            ui.style().interact(&cancel).text_color(),
                        );
                        cancel.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                cancel.enabled(),
                                "Cancel update",
                            )
                        });
                        if cancel.help_text("Cancel update · Escape").clicked() {
                            actions.push(UiAction::Update(Action::Cancel));
                        }
                    }
                    ui.add(egui::Spinner::new().size(12.0));
                    ui.add(
                        egui::Label::new(
                            RichText::new(message).size(12.0).color(chrome::FOREGROUND),
                        )
                        .truncate(),
                    )
                    .help_text(message);
                });
            });
    }
}
