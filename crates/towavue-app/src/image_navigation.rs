use crate::*;

mod sequence;
pub(super) use sequence::ImageSequence;
#[cfg(test)]
pub(super) use sequence::ImageStep;

#[cfg(test)]
pub(crate) mod performance_tests;
#[cfg(test)]
mod sequence_tests;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn scrub_image(&mut self, path: PathBuf, generation: u64, owner: egui::Id) {
        if self.media_kind != Some(MediaKind::Image)
            || generation != self.media_generation
            || self.path.as_ref() == Some(&path)
            || self.modal_input_blocked()
            || self.filmstrip_open
            || self.palette_open
            || self.grid_open
            || !self.folder_snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .items_of_kind(MediaKind::Image)
                    .any(|item| item.path == path)
            })
        {
            return;
        }
        let Some(context) = self.ui_context.clone() else {
            return;
        };
        let Some(continuation) = timeline_input::SeekContinuation::capture(&context, owner) else {
            return;
        };
        let tab = self.tabs.active_id();
        self.request_guarded(GuardedAction::Navigate(path.clone()));
        if self.path.as_ref() == Some(&path)
            && self.tabs.active_id() == tab
            && !self.modal_input_blocked()
        {
            continuation.resume(&context);
        }
    }

    #[cfg(feature = "presentation-verification")]
    pub(super) fn trace_burst(&self, event: towavue_runtime_windows::BurstEvent, value: u64) {
        let state = u64::from(self.image_loading)
            | (u64::from(self.image_sequence.awaiting.is_some()) << 1)
            | (u64::from(self.image_error.is_some()) << 2)
            | (u64::from(self.reading_mode) << 3)
            | (u64::from(self.pending_folder.is_some()) << 4)
            | (u64::from(self.image.is_some()) << 5)
            | (u64::from(self.image_handoff.is_some()) << 6);
        towavue_runtime_windows::record_burst(
            event,
            self.media_generation,
            self.path.as_deref(),
            [value, self.image_sequence.steps.len() as u64, state],
        );
    }

    pub(super) fn jump_images(&mut self, offset: i32) {
        if self.media_kind != Some(MediaKind::Image) {
            return;
        }
        let (Some(snapshot), Some(path)) = (&self.folder_snapshot, &self.path) else {
            return;
        };
        let images: Vec<_> = snapshot.items_of_kind(MediaKind::Image).collect();
        let Some(current) = images.iter().position(|item| &item.path == path) else {
            return;
        };
        let target =
            (current as i128 + i128::from(offset)).clamp(0, images.len() as i128 - 1) as usize;
        if target == current {
            return;
        }
        self.image_navigation_forward = offset > 0;
        let path = images[target].path.clone();
        self.request_guarded(GuardedAction::Navigate(path));
    }

    pub(super) fn shifted_image_digit(
        &self,
        physical: PhysicalKey,
        stroke: KeyStroke,
    ) -> KeyStroke {
        if self.media_kind != Some(MediaKind::Image)
            || !stroke.modifiers.control
            || !stroke.modifiers.shift
            || stroke.modifiers.alt
            || stroke.modifiers.logo
        {
            return stroke;
        }
        let PhysicalKey::Code(code) = physical else {
            return stroke;
        };
        use winit::keyboard::KeyCode::*;
        let digit = match code {
            Digit0 => '0',
            Digit1 => '1',
            Digit2 => '2',
            Digit3 => '3',
            Digit4 => '4',
            Digit5 => '5',
            Digit6 => '6',
            Digit7 => '7',
            Digit8 => '8',
            Digit9 => '9',
            _ => return stroke,
        };
        let resolve = |stroke: &KeyStroke| {
            let mut entered = self.entered_shortcut.clone();
            entered.push(stroke.clone());
            self.shortcuts.resolve(&entered, self.command_context()) != ShortcutMatch::None
                || self
                    .shortcuts
                    .resolve(std::slice::from_ref(stroke), self.command_context())
                    != ShortcutMatch::None
        };
        // Shift changes the text symbol (!, @, etc.). Explicit symbol bindings
        // and prefixes keep priority; otherwise use the numbered top-row key.
        if resolve(&stroke) {
            return stroke;
        }
        let numbered = KeyStroke {
            key: Key::Character(digit),
            modifiers: stroke.modifiers,
        };
        if resolve(&numbered) { numbered } else { stroke }
    }
}

#[cfg(test)]
mod tests;
