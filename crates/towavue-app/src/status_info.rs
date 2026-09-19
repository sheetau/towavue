use crate::*;

#[derive(Clone, Copy, PartialEq)]
enum Group {
    File,
    Display,
    Playback,
    Document,
    Folder,
    Modified,
}

#[derive(Default)]
pub(super) struct StatusInfo {
    pub fields: Vec<String>,
    help: Vec<(Group, String)>,
}

impl StatusInfo {
    fn push(&mut self, group: Group, compact: impl Into<String>, help: impl Into<String>) {
        self.fields.push(compact.into());
        self.help.push((group, help.into()));
    }

    pub fn tooltip(&self) -> String {
        [
            (Group::File, "File"),
            (Group::Display, "Display"),
            (Group::Playback, "Playback"),
            (Group::Document, "Edits"),
            (Group::Folder, "Folder"),
            (Group::Modified, "Modified (local)"),
        ]
        .into_iter()
        .filter_map(|(group, label)| {
            let values: Vec<_> = self
                .help
                .iter()
                .filter(|(kind, _)| *kind == group)
                .map(|(_, value)| value.as_str())
                .collect();
            (!values.is_empty()).then(|| format!("\u{2022} {label}: {}", values.join(" \u{00b7} ")))
        })
        .collect::<Vec<_>>()
        .join("\n")
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    #[cfg(test)]
    pub(super) fn status_details(&self) -> Vec<String> {
        self.status_info().fields
    }

    /// Compact fields and grouped explanations share the same cached snapshot.
    /// Drawing performs no filesystem work, including during image handoff.
    pub(super) fn status_info(&self) -> StatusInfo {
        let mut details = StatusInfo::default();
        let held = self.image_handoff.as_ref();
        let image = held.map(|held| &held.image).or(self.image.as_ref());
        let file = held.map_or_else(
            || self.status_file_details.get(self.status_file_source()),
            |held| held.file_details.as_ref(),
        );
        if matches!(self.media_kind, Some(MediaKind::Image | MediaKind::Video)) {
            let (short, help) = match held.map_or(self.image_view.zoom, |held| held.view.zoom) {
                ZoomMode::Fit => ("Fit".into(), "Fit within the window".into()),
                ZoomMode::Cover => (
                    "Cover".into(),
                    "Fill the window; edges may extend outside the view".into(),
                ),
                ZoomMode::Actual => ("100%".into(), "100% zoom".into()),
                ZoomMode::Custom(scale) => {
                    let value = format!("{:.*}%", if scale < 0.1 { 2 } else { 0 }, scale * 100.0);
                    (value.clone(), format!("{value} zoom"))
                }
            };
            details.push(Group::Display, short, help);
        }
        if image.is_some_and(|image| image.decoded.is_animated()) {
            details.push(
                Group::Playback,
                "1.00\u{00d7}",
                "1.00\u{00d7} animation speed",
            );
        } else if image.is_none() && self.session.is_some() {
            let value = if self.held_speed.is_some() {
                "2\u{00d7} while held".into()
            } else {
                format!("{:.2}\u{00d7}", self.edit_state().rate)
            };
            details.push(Group::Playback, &value, format!("Speed {value}"));
        }
        if self
            .tabs
            .active()
            .is_some_and(|tab| self.edits.get(&tab.id).is_some_and(EditHistory::is_dirty))
        {
            details.push(Group::Document, "Unsaved", "Unsaved changes");
        }
        if let Some(file) = file {
            let value = format_size(file.bytes);
            details.push(Group::File, &value, &value);
        }
        if let Some(extension) = self
            .displayed_image_path()
            .and_then(|path| path.extension())
            .and_then(|extension| extension.to_str())
        {
            let value = extension.to_uppercase();
            details.push(Group::File, &value, &value);
        } else if let Some(image) = image {
            let value = image.decoded.format.to_uppercase();
            details.push(Group::File, &value, &value);
        }
        let dimensions = image.map(|image| image.dimensions()).or_else(|| {
            self.session
                .as_ref()
                .and_then(PlaybackSession::video_geometry)
                .map(|(width, height, _)| (width, height))
        });
        if let Some((width, height)) = dimensions {
            let value = format!("{width}\u{00d7}{height}");
            details.push(Group::File, &value, format!("{value} pixels"));
        }
        if self.media_kind == Some(MediaKind::Image)
            && self.reading_mode
            && self.reading_drag.is_none()
        {
            let value = self.reading_status();
            details.push(Group::Display, &value, &value);
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
                let value = format!("{} / {}", index + 1, count);
                let kind = if self.reading_mode && self.media_kind == Some(MediaKind::Image) {
                    "Image"
                } else {
                    "Item"
                };
                details.push(Group::Folder, &value, format!("{kind} {value}"));
            }
        }
        if let Some(image) = image {
            if image.decoded.is_animated() {
                let value = format!("{} frames", image.decoded.frames.len());
                details.push(
                    Group::Playback,
                    &value,
                    format!("{} animation frames", image.decoded.frames.len()),
                );
            }
            let (short, help) = if self.nearest_images {
                (
                    "Nearest",
                    "Nearest-neighbor image scaling (sharp pixel edges)",
                )
            } else {
                ("Smooth", "Smooth image scaling (filtered)")
            };
            details.push(Group::Display, short, help);
        } else if self.session.is_some() {
            let edit = self.edit_state();
            if edit.trim_start.is_some() || edit.trim_end.is_some() {
                details.push(
                    Group::Document,
                    "Trim (T)",
                    "Trimmed playback range; open the timeline to adjust",
                );
            }
        }
        if let Some(modified) = file.and_then(|file| file.modified_local.as_ref()) {
            details.push(
                Group::Modified,
                format!("Modified (local): {modified}"),
                modified,
            );
        }
        if let Some(snapshot) = &self.folder_snapshot {
            details
                .help
                .push((Group::Folder, snapshot_source(snapshot.source).into()));
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
