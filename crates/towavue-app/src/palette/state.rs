use super::*;

impl<N: Fn(crate::AppEvent) + Send + Sync + 'static> crate::Application<N> {
    pub(crate) fn update_palette_states(&mut self) {
        use CommandId::*;
        let language = self.language();
        let on_off = |enabled| {
            if enabled {
                Text::ReuseFolderTabOn
            } else {
                Text::ReuseFolderTabOff
            }
        };
        let mut values = std::collections::BTreeMap::new();
        for (command, enabled) in [
            (ToggleTabIcons, self.shows_tab_icons()),
            (ToggleReuseFolderTab, self.reuses_folder_tabs()),
            (ToggleFilmstrip, self.filmstrip_open),
            (ToggleReadingMode, self.reading_mode),
            (ReverseReadingOrder, self.reading_settings.reversed),
            (ToggleTimeline, self.timeline_is_visible()),
            (ToggleHardwareEncode, self.prefer_hardware_encode),
            (ToggleFullscreen, self.fullscreen),
            (ToggleSubtitles, self.subtitle_settings().visible),
            (ToggleVideoRepeat, self.video_repeat),
            (ToggleAudioShuffle, self.audio_mode().1),
            (ToggleMute, self.playback_volume() == 0.0),
        ] {
            values.insert(command, on_off(!enabled).in_language(language).to_owned());
        }
        for (command, label) in [
            (
                ToggleImageInterpolation,
                if self.nearest_images {
                    Text::StatusSmooth
                } else {
                    Text::StatusNearest
                },
            ),
            (
                ToggleImageMinification,
                if self.high_quality_minification {
                    Text::MinificationFast
                } else {
                    Text::QualityHigh
                },
            ),
            (
                ToggleReadingAxis,
                if self.reading_settings.axis == towavue_core::ReadingAxis::Vertical {
                    Text::PaletteHorizontal
                } else {
                    Text::PaletteVertical
                },
            ),
        ] {
            values.insert(command, label.in_language(language).to_owned());
        }
        if let Some(label) = match self.state {
            towavue_core::PlaybackState::Playing => Some(Text::StatePaused),
            towavue_core::PlaybackState::Paused => Some(Text::StatePlaying),
            _ => None,
        } {
            values.insert(TogglePause, label.in_language(language).to_owned());
        }
        self.palette.next_values = values;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_values_refresh_without_changing_toggle_dispatch() {
        let mut app = crate::Application::new(None, |_| {}).expect("app");
        app.nearest_images = false;
        app.video_repeat = false;
        app.state = towavue_core::PlaybackState::Paused;
        app.update_palette_states();
        let language = app.language();
        assert_eq!(
            app.palette.next_values[&CommandId::ToggleImageInterpolation],
            Text::StatusNearest.in_language(language)
        );
        assert_eq!(
            app.palette.next_values[&CommandId::ToggleVideoRepeat],
            Text::ReuseFolderTabOn.in_language(language)
        );
        assert_eq!(
            app.palette.next_values[&CommandId::TogglePause],
            Text::StatePlaying.in_language(language)
        );
        app.nearest_images = true;
        app.video_repeat = true;
        app.state = towavue_core::PlaybackState::Playing;
        app.update_palette_states();
        assert_eq!(
            app.palette.next_values[&CommandId::ToggleImageInterpolation],
            Text::StatusSmooth.in_language(language)
        );
        assert_eq!(
            app.palette.next_values[&CommandId::ToggleVideoRepeat],
            Text::ReuseFolderTabOff.in_language(language)
        );
        assert_eq!(
            app.palette.next_values[&CommandId::TogglePause],
            Text::StatePaused.in_language(language)
        );
        assert!(!options::group(CommandId::ToggleImageInterpolation));
        assert!(!options::group(CommandId::ToggleVideoRepeat));
        assert!(
            search::ranked("interpolation", localization::Language::English, &[])
                .0
                .iter()
                .any(|command| command.id == CommandId::ToggleImageInterpolation)
        );
    }
}
