use crate::*;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    /// Whole status fields, highest priority first. Only retained/cached data is read.
    pub(super) fn status_details(&self) -> Vec<String> {
        let mut details = Vec::new();
        let held = self.image_handoff.as_ref();
        let image = held.map(|held| &held.image).or(self.image.as_ref());
        let file = held.map_or_else(
            || self.status_file_details.get(self.status_file_source()),
            |held| held.file_details.as_ref(),
        );
        if matches!(self.media_kind, Some(MediaKind::Image | MediaKind::Video)) {
            details.push(
                match held.map_or(self.image_view.zoom, |held| held.view.zoom) {
                    ZoomMode::Fit => "Fit".into(),
                    ZoomMode::Cover => "Cover".into(),
                    ZoomMode::Actual => "100%".into(),
                    ZoomMode::Custom(scale) => {
                        format!("{:.*}%", if scale < 0.1 { 2 } else { 0 }, scale * 100.0)
                    }
                },
            );
        }
        if image.is_some_and(|image| image.decoded.is_animated()) {
            details.push("1.00×".into());
        } else if image.is_none() && self.session.is_some() {
            details.push(if self.held_speed.is_some() {
                "2× while held".into()
            } else {
                format!("{:.2}×", self.edit_state().rate)
            });
        }
        if self
            .tabs
            .active()
            .is_some_and(|tab| self.edits.get(&tab.id).is_some_and(EditHistory::is_dirty))
        {
            details.push("Unsaved".into());
        }
        if let Some(file) = file {
            details.push(format_size(file.bytes));
        }
        if let Some(extension) = self
            .displayed_image_path()
            .and_then(|path| path.extension())
            .and_then(|extension| extension.to_str())
        {
            details.push(extension.to_uppercase());
        } else if let Some(image) = image {
            details.push(image.decoded.format.to_uppercase());
        }
        if let Some(image) = image {
            let (width, height) = image.dimensions();
            details.push(format!("{width}×{height}"));
        } else {
            if let Some((width, height, _)) = self
                .session
                .as_ref()
                .and_then(PlaybackSession::video_geometry)
            {
                details.push(format!("{width}×{height}"));
            }
        }
        if self.media_kind == Some(MediaKind::Image)
            && self.reading_mode
            && self.reading_drag.is_none()
        {
            details.push(self.reading_status());
        }
        if let Some(path) = self.displayed_image_path()
            && let Some(snapshot) = &self.folder_snapshot
        {
            let position = if self.reading_mode && self.media_kind == Some(MediaKind::Image) {
                let mut count = 0;
                let mut position = None;
                for item in snapshot.items_of_kind(MediaKind::Image) {
                    if Some(&item.path) == self.reading_focus_path() {
                        position = Some(count);
                    }
                    count += 1;
                }
                position.map(|index| (index, count))
            } else {
                snapshot
                    .item_index(path)
                    .map(|index| (index, snapshot.items.len()))
            };
            if let Some((index, count)) = position {
                details.push(format!("{} / {}", index + 1, count));
            }
        }
        if let Some(image) = image {
            if image.decoded.is_animated() {
                details.push(format!("{} frames", image.decoded.frames.len()));
            }
            details.push(
                if self.nearest_images {
                    "Nearest"
                } else {
                    "Smooth"
                }
                .into(),
            );
        } else if self.session.is_some() {
            let edit = self.edit_state();
            if edit.trim_start.is_some() || edit.trim_end.is_some() {
                details.push("Trim (T)".into());
            }
        }
        if let Some(modified) = file.and_then(|file| file.modified_local.as_ref()) {
            details.push(format!("Modified (local): {modified}"));
        }
        details
    }
}

/// Choose a prefix so widening only adds fields; never elide or split a field.
pub(super) fn fitting_text(ui: &egui::Ui, details: &[String], width: f32) -> String {
    let mut text = String::new();
    for detail in details {
        let candidate = if text.is_empty() {
            detail.clone()
        } else {
            format!("{text}   {detail}")
        };
        let galley = ui.painter().layout_no_wrap(
            candidate.clone(),
            egui::FontId::proportional(12.0),
            chrome::MUTED,
        );
        if galley.size().x > width {
            break;
        }
        text = candidate;
    }
    text
}
