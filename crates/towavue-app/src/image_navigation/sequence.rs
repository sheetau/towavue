use super::*;

#[derive(Default)]
pub(crate) struct ImageSequence {
    pub awaiting: Option<u64>,
    pub steps: std::collections::VecDeque<bool>,
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
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
        if self.image_sequence.awaiting.is_none() {
            return false;
        }
        if self.image_sequence_blocked() {
            self.image_sequence = ImageSequence::default();
            return false;
        }
        if self.image_sequence.steps.len() == 256 {
            self.set_status(
                "Image navigation queue is full. Wait for accepted steps to finish.".into(),
            );
        } else {
            self.image_sequence.steps.push_back(forward);
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
        self.image_sequence.awaiting = None;
        if self.image_sequence_blocked() {
            self.image_sequence.steps.clear();
            return;
        }
        if let Some(forward) = self.image_sequence.steps.pop_front() {
            // Only this continuation preserves queued directions across load/guard cancellation.
            let remaining = std::mem::take(&mut self.image_sequence.steps);
            self.navigate(forward, true);
            if self.image_sequence.awaiting.is_some() {
                self.image_sequence.steps = remaining;
            }
        }
    }
}
