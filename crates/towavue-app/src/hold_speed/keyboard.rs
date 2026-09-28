use super::*;

const DELAY: Duration = Duration::from_millis(400);

#[derive(Clone, Copy)]
pub(crate) struct Press {
    tab: TabId,
    media: u64,
    started: Instant,
    long: bool,
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    fn video_space_allowed(&self) -> bool {
        let stroke = KeyStroke {
            modifiers: Modifiers::default(),
            key: Key::Space,
        };
        self.media_kind == Some(MediaKind::Video)
            && self.session.is_some()
            && matches!(
                self.state,
                PlaybackState::Playing | PlaybackState::Paused | PlaybackState::Ended
            )
            && self.modifiers.is_empty()
            && !self.modal_input_blocked()
            && !self.palette_open
            && !self.filmstrip_open
            && !self.keyboard_capture_active()
            && !self.native_ime_composing
            && self.entered_shortcut.is_empty()
            && self.ui_context.as_ref().is_some_and(|context| {
                !egui::Popup::is_any_open(context)
                    && !context.egui_wants_keyboard_input()
                    && context.input(|input| !input.pointer.any_down())
            })
            && matches!(
                self.shortcuts.resolve(&[stroke], self.command_context()),
                ShortcutMatch::Command(CommandId::TogglePause)
            )
    }

    pub(crate) fn space_hold_deadline(&self) -> Option<Instant> {
        self.space_hold
            .filter(|press| !press.long)
            .map(|press| press.started + DELAY)
    }

    pub(crate) fn video_space_key(&mut self, pressed: bool, repeat: bool, now: Instant) -> bool {
        if let Some(press) = self.space_hold {
            if !pressed {
                let short = !press.long
                    && now.saturating_duration_since(press.started) < DELAY
                    && self.tabs.active_id() == Some(press.tab)
                    && self.media_generation == press.media
                    && self.video_space_allowed();
                self.cancel_hold_speed();
                if short {
                    self.dispatch(CommandId::TogglePause);
                }
            } else if !self.video_space_allowed() {
                self.cancel_hold_speed();
            }
            return true;
        }
        if !pressed || repeat || !self.video_space_allowed() || self.held_speed.is_some() {
            return false;
        }
        let Some(tab) = self.tabs.active_id() else {
            return false;
        };
        self.space_hold = Some(Press {
            tab,
            media: self.media_generation,
            started: now,
            long: false,
        });
        self.request_redraw();
        true
    }

    pub(crate) fn update_space_hold(&mut self, now: Instant) {
        let Some(mut press) = self.space_hold else {
            return;
        };
        if self.tabs.active_id() != Some(press.tab)
            || self.media_generation != press.media
            || !self.video_space_allowed()
        {
            self.cancel_hold_speed();
            return;
        }
        if !press.long && now >= press.started + DELAY {
            press.long = true;
            self.space_hold = Some(press);
            if self.hold_enabled() {
                // Keyboard ownership is distinct from pointer gesture tokens.
                self.begin_hold_speed(u64::MAX);
                if self.held_speed.is_some() {
                    self.set_status(
                        localization::Text::HoldSpeedHint
                            .in_language(self.language())
                            .into(),
                    );
                }
            }
        }
    }
}
