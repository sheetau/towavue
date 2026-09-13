use super::*;

#[derive(Clone, Copy)]
pub(super) struct PlaybackVolume {
    level: f32,
    unmuted: f32,
}

impl Default for PlaybackVolume {
    fn default() -> Self {
        Self {
            level: 1.0,
            unmuted: 1.0,
        }
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn playback_volume(&self) -> f32 {
        self.tabs
            .active()
            .and_then(|tab| self.playback_volumes.get(&tab.id))
            .copied()
            .unwrap_or_default()
            .level
    }

    pub(super) fn set_playback_volume(&mut self, level: f32) {
        if !level.is_finite()
            || !matches!(self.media_kind, Some(MediaKind::Audio | MediaKind::Video))
        {
            return;
        }
        let Some(id) = self.tabs.active().map(|tab| tab.id) else {
            return;
        };
        let level = level.clamp(0.0, 2.0);
        if level == self.playback_volume() {
            return;
        }
        let volume = self.playback_volumes.entry(id).or_default();
        volume.level = level;
        if level > 0.0 {
            volume.unmuted = level;
        }
        // Existing saved gain remains independent; changing the listening level
        // never re-decodes the timeline, modifies history, or changes export.
        let gain = self.edit_state().volume * level;
        if let Some(session) = &mut self.session {
            session.set_volume(gain);
        }
        self.volume_hud
            .changed(id, self.media_generation, Instant::now());
        self.request_redraw();
    }

    pub(super) fn toggle_playback_mute(&mut self) {
        let volume = self
            .tabs
            .active()
            .and_then(|tab| self.playback_volumes.get(&tab.id))
            .copied()
            .unwrap_or_default();
        self.set_playback_volume(if volume.level == 0.0 {
            volume.unmuted
        } else {
            0.0
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_volume_commands_do_not_change_export_history() {
        let Some(root) = crate::tests::isolated_test_root(
            "playback_volume::tests::playback_volume_commands_do_not_change_export_history",
        ) else {
            return;
        };
        for kind in [MediaKind::Audio, MediaKind::Video] {
            let mut app = Application::new(None, |_| {}).expect("app");
            let source = root.join(if kind == MediaKind::Audio {
                "audio.wav"
            } else {
                "video.mp4"
            });
            let tab = app.tabs.open_new(source.clone(), kind);
            app.path = Some(source.clone());
            app.media_kind = Some(kind);
            app.state = PlaybackState::Paused;
            app.media_duration = Some(Duration::from_secs(2));
            app.handle_ui_action(UiAction::Volume(tab, 0.4));
            assert_eq!(
                app.edit_state().volume,
                1.0,
                "wheel must not change export gain"
            );
            assert!(!app.command_context().has_unsaved_edits);
            app.dispatch(CommandId::VolumeUp);
            app.dispatch(CommandId::VolumeDown);
            app.dispatch(CommandId::ToggleMute);
            assert_eq!(app.edit_state().volume, 1.0, "mute must not mute export");
            assert!(!app.command_context().has_unsaved_edits);
            assert_eq!(app.playback_volume(), 0.0);
            let selection =
                towavue_core::TimeRange::new(MediaTime::ZERO, media_time(Duration::from_secs(1)))
                    .expect("range");
            app.push_edit(EditOperation::Timeline(
                towavue_core::TimelineEdit::SetVolume(selection, 0.5),
            ));
            let history = app.edits[&tab].clone();
            app.dispatch(CommandId::VolumeUp);
            app.dispatch(CommandId::ToggleMute);
            assert_eq!(app.edits[&tab], history, "saved gain remains independent");
            // Same-source rejection prevents this request-capture job writing a file.
            for output in [ExportOutput::Media, ExportOutput::AudioOnly] {
                assert!(app.start_export(tab, source.clone(), kind, source.clone(), None, output));
                app.set_playback_volume(1.7);
                app.toggle_playback_mute();
                let export = app.active_export.as_ref().expect("export request");
                assert_eq!(export.request.operations, history.operations());
                assert_eq!(
                    export.options,
                    ExportOptions {
                        output,
                        ..ExportOptions::default()
                    }
                );
                app.active_export.take();
            }
        }
    }
}
