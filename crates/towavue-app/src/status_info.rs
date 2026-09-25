use crate::localization::{Language, Text};
use crate::*;
use towavue_core::localization::formatted;

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
    language: Language,
    pub fields: Vec<String>,
    help: Vec<(Group, String)>,
}

impl StatusInfo {
    fn push(&mut self, group: Group, compact: impl Into<String>, help: impl Into<String>) {
        self.fields.push(compact.into());
        self.help.push((group, help.into()));
    }

    pub fn tooltip(&self) -> String {
        let language = self.language;
        [
            (Group::File, Text::MenuFile.in_language(language)),
            (Group::Display, Text::StatusDisplay.in_language(language)),
            (Group::Playback, Text::StatusPlayback.in_language(language)),
            (Group::Document, Text::StatusEdits.in_language(language)),
            (Group::Folder, Text::StatusFolder.in_language(language)),
            (Group::Modified, Text::StatusModified.in_language(language)),
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
        let language = self.language();
        let mut details = StatusInfo {
            language,
            ..Default::default()
        };
        let held = self.image_handoff.as_ref();
        let image = held.map(|held| &held.image).or(self.image.as_ref());
        let file = held.map_or_else(
            || self.status_file_details.get(self.status_file_source()),
            |held| held.file_details.as_ref(),
        );
        if matches!(self.media_kind, Some(MediaKind::Image | MediaKind::Video)) {
            let (short, help) = match held.map_or(self.image_view.zoom, |held| held.view.zoom) {
                ZoomMode::Fit => (
                    Text::StatusFit.in_language(language).into(),
                    Text::StatusFitHelp.in_language(language).into(),
                ),
                ZoomMode::Cover => (
                    Text::StatusCover.in_language(language).into(),
                    Text::StatusCoverHelp.in_language(language).into(),
                ),
                ZoomMode::Actual => (
                    "100%".into(),
                    Text::StatusActualZoom.in_language(language).into(),
                ),
                ZoomMode::Custom(scale) => {
                    let value = format!("{:.*}%", if scale < 0.1 { 2 } else { 0 }, scale * 100.0);
                    (value.clone(), formatted::status_zoom(language, &value))
                }
            };
            let reduction = if self.high_quality_minification {
                Text::StatusHighQualityReduction.in_language(language)
            } else {
                Text::StatusFastReduction.in_language(language)
            };
            details.push(
                Group::Display,
                short,
                formatted::status_reduction(language, &help, reduction),
            );
        }
        if image.is_some_and(|image| image.decoded.is_animated()) {
            details.push(
                Group::Playback,
                "1.00\u{00d7}",
                Text::StatusAnimationSpeed.in_language(language),
            );
        } else if image.is_none() && self.session.is_some() {
            let value = if self.held_speed.is_some() {
                Text::StatusHeldSpeed.in_language(language).into()
            } else {
                format!("{:.2}\u{00d7}", self.edit_state().rate)
            };
            details.push(
                Group::Playback,
                &value,
                formatted::status_speed(language, &value),
            );
        }
        if self.media_kind == Some(MediaKind::Video)
            && let Some(fps) = self
                .session
                .as_ref()
                .and_then(PlaybackSession::source_video_frame_rate)
        {
            let value = frame_rate_text(fps);
            details.push(
                Group::File,
                &value,
                formatted::source_frame_rate(language, &value),
            );
        }
        if self
            .tabs
            .active()
            .is_some_and(|tab| self.edits.get(&tab.id).is_some_and(EditHistory::is_dirty))
        {
            details.push(
                Group::Document,
                Text::StatusUnsaved.in_language(language),
                Text::StatusUnsavedHelp.in_language(language),
            );
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
        } else if self.current_document_untitled() {
            details.push(
                Group::Document,
                "PNG",
                Text::StatusPasted.in_language(language),
            );
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
            details.push(
                Group::File,
                &value,
                formatted::status_pixels(language, &value),
            );
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
                    Text::StatusImage.in_language(language)
                } else {
                    Text::StatusItem.in_language(language)
                };
                details.push(Group::Folder, &value, format!("{kind} {value}"));
            }
        }
        if let Some(image) = image {
            if image.decoded.is_animated() {
                let value = formatted::status_frames(language, image.decoded.frames.len());
                details.push(
                    Group::Playback,
                    &value,
                    formatted::status_animation_frames(language, image.decoded.frames.len()),
                );
            }
            let (short, help) = if self.nearest_images {
                (
                    Text::StatusNearest.in_language(language),
                    Text::StatusNearestHelp.in_language(language),
                )
            } else {
                (
                    Text::StatusSmooth.in_language(language),
                    Text::StatusSmoothHelp.in_language(language),
                )
            };
            details.push(Group::Display, short, help);
        } else if self.session.is_some() {
            let edit = self.edit_state();
            if edit.trim_start.is_some() || edit.trim_end.is_some() {
                details.push(
                    Group::Document,
                    Text::StatusTrim.in_language(language),
                    Text::StatusTrimHelp.in_language(language),
                );
            }
        }
        if let Some(modified) = file.and_then(|file| file.modified_local.as_ref()) {
            details.push(
                Group::Modified,
                formatted::status_modified(language, modified),
                modified,
            );
        }
        if let Some(snapshot) = &self.folder_snapshot {
            details.help.push((
                Group::Folder,
                snapshot_source(language, snapshot.source).into(),
            ));
        }
        details
    }
}

/// Match the path separator to the spacing between complete status fields.
pub(super) fn field_gap(ui: &egui::Ui) -> f32 {
    ui.painter()
        .layout_no_wrap(
            "   ".into(),
            egui::FontId::proportional(12.0),
            chrome::MUTED,
        )
        .size()
        .x
}

/// Reuse the measured right-aligned galley so fitting and painting share identical metrics.
pub(super) fn fitting_text(
    ui: &egui::Ui,
    details: &[String],
    width: f32,
) -> Option<std::sync::Arc<egui::Galley>> {
    let mut text = String::new();
    let mut fitted = None;
    for detail in details {
        let candidate = if text.is_empty() {
            detail.clone()
        } else {
            format!("{text}   {detail}")
        };
        let mut job = egui::text::LayoutJob::simple_singleline(
            candidate.clone(),
            egui::FontId::proportional(12.0),
            chrome::MUTED,
        );
        job.halign = egui::Align::RIGHT;
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        if galley.size().x > width {
            break;
        }
        fitted = Some(galley);
        text = candidate;
    }
    fitted
}

fn frame_rate_text(fps: f64) -> String {
    // Very sparse streams must not round down to an impossible zero FPS.
    let number = if fps < 0.001 {
        fps.to_string()
    } else {
        format!("{fps:.3}")
    };
    format!("{} fps", number.trim_end_matches('0').trim_end_matches('.'))
}

#[cfg(test)]
mod tests {
    #[test]
    fn source_frame_rate_keeps_useful_fractional_precision() {
        for (fps, expected) in [
            (60.0, "60 fps"),
            (30000.0 / 1001.0, "29.97 fps"),
            (24000.0 / 1001.0, "23.976 fps"),
            (0.5, "0.5 fps"),
            (0.0001, "0.0001 fps"),
        ] {
            assert_eq!(super::frame_rate_text(fps), expected);
        }
    }
}
