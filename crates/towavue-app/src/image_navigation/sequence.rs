use super::*;

const HELD_LAG_LIMIT: Duration = Duration::from_millis(150);

#[derive(Clone, Debug)]
pub(crate) struct ImageStep {
    pub forward: bool,
    repeat: Option<RepeatStep>,
}

#[derive(Clone, Debug)]
struct RepeatStep {
    key: Key,
    queued: Instant,
    released: bool,
}

impl ImageStep {
    pub fn press(forward: bool) -> Self {
        Self {
            forward,
            repeat: None,
        }
    }
}

#[derive(Default)]
pub(crate) struct ImageSequence {
    pub awaiting: Option<u64>,
    pub steps: std::collections::VecDeque<ImageStep>,
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(crate) fn repeat_image_shortcut(&mut self, stroke: KeyStroke) {
        // Repeat only standalone image navigation, never a chord prefix or a
        // command rebound to the same physical key. Reuse the presentation queue.
        if self.image_sequence_blocked() || self.filmstrip_open || !self.entered_shortcut.is_empty()
        {
            return;
        }
        if let ShortcutMatch::Command(command @ (CommandId::PreviousImage | CommandId::NextImage)) =
            self.shortcuts
                .resolve(std::slice::from_ref(&stroke), self.command_context())
        {
            self.coalesce_image_repeats(Instant::now());
            if self.image_sequence.awaiting.is_some() {
                let forward = command == CommandId::NextImage;
                self.image_navigation_forward = forward;
                self.enqueue_image_step(ImageStep {
                    forward,
                    repeat: Some(RepeatStep {
                        key: stroke.key,
                        queued: Instant::now(),
                        released: false,
                    }),
                });
                self.coalesce_image_repeats(Instant::now());
            } else {
                self.dispatch(command);
            }
        }
    }

    fn image_sequence_blocked(&self) -> bool {
        self.media_kind != Some(MediaKind::Image)
            || self.reading_mode
            || self.image_edit_pending
            || self.modal_input_blocked()
            || self.active_export.is_some()
            || self.command_context().has_unsaved_edits
            || self.palette_open
            || self.grid_open
            || self.displayed_tab.is_none()
            || self.displayed_tab != self.tabs.active().map(|tab| tab.id)
            || self
                .ui_context
                .as_ref()
                .is_some_and(egui::Popup::is_any_open)
    }

    pub(crate) fn queue_image_step(&mut self, forward: bool) -> bool {
        self.enqueue_image_step(ImageStep::press(forward))
    }

    fn enqueue_image_step(&mut self, step: ImageStep) -> bool {
        #[cfg(feature = "presentation-verification")]
        let forward = step.forward;
        if self.image_sequence.awaiting.is_none() {
            return false;
        }
        if self.image_sequence_blocked() {
            #[cfg(feature = "presentation-verification")]
            self.trace_burst(towavue_runtime_windows::BurstEvent::QueueCancelled, 1);
            self.image_sequence = ImageSequence::default();
            return false;
        }
        if self.image_sequence.steps.len() == 256 {
            #[cfg(feature = "presentation-verification")]
            self.trace_burst(
                towavue_runtime_windows::BurstEvent::QueueFull,
                u64::from(forward),
            );
            self.set_status(
                "Image navigation queue is full. Wait for accepted steps to finish.".into(),
            );
        } else {
            self.image_sequence.steps.push_back(step);
            #[cfg(feature = "presentation-verification")]
            self.trace_burst(
                towavue_runtime_windows::BurstEvent::Queued,
                u64::from(forward),
            );
        }
        true
    }

    pub(crate) fn release_image_repeats(&mut self, key: &Key, cancel: bool) {
        if cancel || self.filmstrip_open || self.image_sequence_blocked() {
            self.image_sequence
                .steps
                .retain(|step| step.repeat.as_ref().is_none_or(|repeat| &repeat.key != key));
            return;
        }
        for step in &mut self.image_sequence.steps {
            if let Some(repeat) = &mut step.repeat
                && &repeat.key == key
            {
                repeat.released = true;
            }
        }
        self.coalesce_image_repeats(Instant::now());
    }

    pub(crate) fn cancel_image_repeats(&mut self) {
        self.image_sequence
            .steps
            .retain(|step| step.repeat.is_none());
    }

    pub(super) fn coalesce_image_repeats(&mut self, now: Instant) -> bool {
        if self.image_sequence_blocked() {
            return false;
        }
        let Some(repeat) = self
            .image_sequence
            .steps
            .front()
            .and_then(|step| step.repeat.as_ref())
        else {
            return false;
        };
        if !repeat.released
            && !(self.image_loading
                && now.saturating_duration_since(repeat.queued) >= HELD_LAG_LIMIT)
            && self.image_sequence.steps.len() < 256
        {
            return false;
        }
        // Fold only this consecutive held-key run. Ordinary presses and other
        // keys keep their place, including a held run behind a pending press.
        let count = self
            .image_sequence
            .steps
            .iter()
            .take_while(|step| {
                step.repeat
                    .as_ref()
                    .is_some_and(|next| next.key == repeat.key)
            })
            .count();
        let (Some(snapshot), Some(path)) = (&self.folder_snapshot, &self.path) else {
            return false;
        };
        let images: Vec<_> = snapshot.items_of_kind(MediaKind::Image).collect();
        let Some(mut index) = images.iter().position(|item| &item.path == path) else {
            return false;
        };
        for step in self.image_sequence.steps.iter().take(count) {
            index = if step.forward {
                if index + 1 < images.len() {
                    index + 1
                } else if self.folder_navigation_loop {
                    0
                } else {
                    index
                }
            } else if index > 0 {
                index - 1
            } else if self.folder_navigation_loop {
                images.len() - 1
            } else {
                index
            };
        }
        let target = images[index].path.clone();
        let released = repeat.released;
        let forward = self.image_sequence.steps[count - 1].forward;
        self.image_sequence.steps.drain(..count);
        #[cfg(feature = "presentation-verification")]
        self.trace_burst(
            towavue_runtime_windows::BurstEvent::QueueCancelled,
            if released { 3 } else { 4 },
        );
        #[cfg(not(feature = "presentation-verification"))]
        let _ = released;
        if self.path.as_ref() == Some(&target) {
            return false;
        }
        let remaining = std::mem::take(&mut self.image_sequence.steps);
        self.image_navigation_forward = forward;
        self.request_guarded(GuardedAction::Navigate(target));
        if self.image_sequence.awaiting.is_some() {
            self.image_sequence.steps = remaining;
        }
        true
    }

    pub(crate) fn image_sequence_token(&self, output: &egui::FullOutput) -> Option<u64> {
        let generation = self.image_sequence.awaiting?;
        if generation != self.media_generation || self.image_loading || self.image_error.is_some() {
            return None;
        }
        let image = self.image.as_ref()?;
        let viewport = self.ui_context.as_ref()?.content_rect();
        output
            .shapes
            .iter()
            .any(|shape| {
                let clip = shape.clip_rect.intersect(viewport);
                matches!(&shape.shape, egui::Shape::Mesh(mesh)
                    if mesh.texture_id == image.texture.id() && clip.width() > 0.0
                        && clip.height() > 0.0 && clip.intersects(mesh.calc_bounds()))
            })
            .then_some(generation)
    }

    pub(crate) fn finish_image_sequence_frame(&mut self, token: Option<u64>) {
        let Some(token) = token else {
            return;
        };
        if self.image_sequence.awaiting != Some(token) || self.media_generation != token {
            return;
        }
        if self.image_sequence_blocked() {
            #[cfg(feature = "presentation-verification")]
            self.trace_burst(towavue_runtime_windows::BurstEvent::QueueCancelled, 2);
            self.image_sequence = ImageSequence::default();
            return;
        }
        // Initial pixels can arrive before Shell order. Keep accepting bounded
        // direction input until that same source's order request finishes.
        if self.folder_snapshot.is_none()
            && matches!(&self.pending_folder, Some((_, FolderIntent::Refresh(path)))
                if self.path.as_ref() == Some(path))
        {
            return;
        }
        if self.coalesce_image_repeats(Instant::now()) {
            return;
        }
        self.image_sequence.awaiting = None;
        if let Some(step) = self.image_sequence.steps.pop_front() {
            let forward = step.forward;
            #[cfg(feature = "presentation-verification")]
            self.trace_burst(
                towavue_runtime_windows::BurstEvent::Dequeued,
                u64::from(forward),
            );
            // Only this continuation preserves queued directions across load/guard cancellation.
            let remaining = std::mem::take(&mut self.image_sequence.steps);
            self.navigate(forward, true);
            if self.image_sequence.awaiting.is_some() {
                self.image_sequence.steps = remaining;
                #[cfg(feature = "presentation-verification")]
                self.trace_burst(towavue_runtime_windows::BurstEvent::QueueRestored, 0);
            }
        } else {
            #[cfg(feature = "presentation-verification")]
            self.trace_burst(
                towavue_runtime_windows::BurstEvent::SequenceSettled,
                self.image_generation,
            );
        }
    }
}
