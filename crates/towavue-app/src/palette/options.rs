use super::*;
use towavue_core::{AudioTrackSelection, TabId};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    Command(CommandId),
    Audio(AudioTrackSelection),
    Video(towavue_core::VideoTrackId),
    Subtitle(crate::subtitles::Action),
}

pub(crate) struct OptionRow {
    pub label: String,
    pub value: Value,
    pub current: bool,
}

pub(super) struct Step {
    command: CommandId,
    pub(super) title: String,
    owner: (Option<TabId>, u64),
    rows: Vec<OptionRow>,
    selected: Option<usize>,
    previous_query: String,
}

pub(crate) fn group(command: CommandId) -> bool {
    matches!(
        command,
        CommandId::FilmstripGrid
            | CommandId::ToggleReuseFolderTab
            | CommandId::CycleVolumeStep
            | CommandId::CycleAudioRepeat
            | CommandId::ResizeViewFit
            | CommandId::ExportQualityHigh
            | CommandId::CycleAudioTrack
            | CommandId::CycleVideoTrack
            | CommandId::LoadSubtitles
    )
}

pub(super) fn child(command: CommandId) -> bool {
    matches!(
        command,
        CommandId::FilmstripRow
            | CommandId::ReuseFolderTabOff
            | CommandId::ReuseFolderTabAll
            | CommandId::ReuseFolderTabSameKind
            | CommandId::VolumeStepTwo
            | CommandId::VolumeStepFive
            | CommandId::VolumeStepTen
            | CommandId::AudioRepeatOff
            | CommandId::AudioRepeatAll
            | CommandId::AudioRepeatOne
            | CommandId::ResizeViewKeepArea
            | CommandId::ResizeViewKeepScale
            | CommandId::ExportQualityBalanced
            | CommandId::ExportQualitySmaller
    )
}

pub(super) fn title(command: CommandId, language: localization::Language) -> Option<&'static str> {
    Some(
        match command {
            CommandId::FilmstripGrid => Text::FilmstripLayout,
            CommandId::ToggleReuseFolderTab => Text::CommandToggleReuseFolderTab,
            CommandId::CycleVolumeStep => Text::ListeningVolumeStep,
            CommandId::CycleAudioRepeat => Text::AudioRepeat,
            CommandId::ResizeViewFit => Text::ViewResizeBehavior,
            CommandId::ExportQualityHigh => Text::ExportQuality,
            CommandId::CycleAudioTrack => Text::AudioTracks,
            CommandId::CycleVideoTrack => Text::VideoTracks,
            CommandId::LoadSubtitles => Text::Subtitles,
            _ => return None,
        }
        .in_language(language),
    )
}

impl CommandPalette {
    pub(crate) fn options_command(&self) -> Option<CommandId> {
        self.step.as_ref().map(|step| step.command)
    }
    pub(crate) fn begin_options(
        &mut self,
        command: CommandId,
        language: localization::Language,
        owner: (Option<TabId>, u64),
        rows: Vec<OptionRow>,
    ) {
        let selected = rows
            .iter()
            .position(|row| row.current)
            .or((!rows.is_empty()).then_some(0));
        self.step = Some(Step {
            command,
            title: title(command, language).unwrap_or_default().into(),
            owner,
            rows,
            selected,
            previous_query: std::mem::take(&mut self.query),
        });
        self.fresh = true;
        self.clear_preview();
    }

    pub(crate) fn options_owner_matches(&self, owner: (Option<TabId>, u64)) -> bool {
        self.step.as_ref().is_none_or(|step| step.owner == owner)
    }

    pub(super) fn back(&mut self) {
        if let Some(step) = self.step.take() {
            self.query = step.previous_query;
            self.selected_command = Some(step.command);
            self.fresh = true;
        }
    }

    pub(super) fn show_options(
        &mut self,
        ui: &mut egui::Ui,
        navigation: Navigation,
        enter: bool,
        query_changed: bool,
    ) -> Option<Choice> {
        let step = self.step.as_mut()?;
        let query = self.query.trim().to_lowercase();
        let matches: Vec<_> = step
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.label.to_lowercase().contains(&query))
            .collect();
        let current = step
            .selected
            .and_then(|selected| matches.iter().position(|(index, _)| *index == selected));
        let enabled = vec![true; matches.len()];
        let selected = navigate_enabled(
            current.or((!matches.is_empty()).then_some(0)),
            &enabled,
            navigation,
            self.list_height,
        );
        step.selected = selected.map(|row| matches[row].0);
        let mut chosen = enter
            .then_some(selected)
            .flatten()
            .map(|index| matches[index].1.value.clone());
        overlay_scroll_bar(ui);
        let height = (ui.ctx().content_rect().height() - 120.0).clamp(40.0, 264.0);
        let output = egui::ScrollArea::vertical()
            .id_salt("palette-options")
            .auto_shrink([false, true])
            .max_height(height)
            .show_popup_styled(ui, crate::chrome::PADDING as f32, |ui| {
                ui.spacing_mut().item_spacing.y = crate::chrome::ITEM_GAP;
                if matches.is_empty() {
                    row::heading(ui, localization::text(ui.ctx(), Text::NoMatchingCommands));
                }
                for (index, (original, row)) in matches.iter().enumerate() {
                    let (_, rect) = ui.allocate_space(egui::vec2(
                        ui.available_width(),
                        crate::chrome::ROW_HEIGHT,
                    ));
                    let mut row_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt(("option-row", *original))
                            .max_rect(rect),
                    );
                    let ui = &mut row_ui;
                    let selected = selected == Some(index);
                    let atoms = egui::Atoms::new((
                        highlight::label(
                            &row.label,
                            &query,
                            crate::chrome::TEXT_MD,
                            egui::Color32::PLACEHOLDER,
                        ),
                        egui::Atom::grow(),
                        egui::Atom::default().atom_size(egui::Vec2::splat(20.0)),
                    ));
                    let background = ui.painter().add(egui::Shape::Noop);
                    let response = ui
                        .push_id(("palette-option", *original), |ui| {
                            row_button(
                                ui,
                                rect.with_max_x(rect.right() - crate::chrome::PADDING as f32),
                                selected,
                                atoms,
                            )
                        })
                        .inner;
                    ui.painter()
                        .set(background, row_background(ui, rect, &response, selected));
                    if row.current {
                        crate::chrome::check_mark(
                            ui,
                            egui::pos2(
                                rect.right() - crate::chrome::PADDING as f32 - 10.0,
                                rect.center().y,
                            ),
                            if selected {
                                ui.visuals().selection.stroke
                            } else {
                                ui.visuals().widgets.inactive.fg_stroke
                            },
                        );
                    }
                    if selected && (navigation.moved() || query_changed) {
                        response.scroll_to_me(None);
                    }
                    if response.clicked() && !query_changed {
                        chosen = Some(row.value.clone());
                    }
                }
            });
        self.list_height = output.inner_rect.height();
        chosen.map(|value| Choice::Option(step.owner, value))
    }
}

impl<N: Fn(crate::AppEvent) + Send + Sync + 'static> crate::Application<N> {
    pub(crate) fn begin_palette_options(&mut self, command: CommandId) -> bool {
        use CommandId::*;
        if !group(command) {
            return false;
        }
        let language = self.language();
        let mut rows = Vec::new();
        if let Some((_, options)) = crate::menu::choices::options(command) {
            let selected = match command {
                FilmstripGrid => {
                    if self.filmstrip_is_grid() {
                        FilmstripGrid
                    } else {
                        FilmstripRow
                    }
                }
                CycleVolumeStep => match self.volume_step_percent {
                    5 => VolumeStepFive,
                    10 => VolumeStepTen,
                    _ => VolumeStepTwo,
                },
                CycleAudioRepeat => match self.audio_mode().0 {
                    towavue_core::RepeatMode::Off => AudioRepeatOff,
                    towavue_core::RepeatMode::All => AudioRepeatAll,
                    towavue_core::RepeatMode::One => AudioRepeatOne,
                },
                ToggleReuseFolderTab => match self.folder_tab_reuse_mode() {
                    1 => ReuseFolderTabAll,
                    2 => ReuseFolderTabSameKind,
                    _ => ReuseFolderTabOff,
                },
                ResizeViewFit => self.view_resize_mode.command(),
                ExportQualityHigh => match self.video_export_quality() {
                    towavue_runtime_windows::VideoExportQuality::High => ExportQualityHigh,
                    towavue_runtime_windows::VideoExportQuality::Balanced => ExportQualityBalanced,
                    towavue_runtime_windows::VideoExportQuality::Smaller => ExportQualitySmaller,
                },
                _ => command,
            };
            rows.extend(options.iter().map(|(command, label)| OptionRow {
                label: label.in_language(language).into(),
                value: Value::Command(*command),
                current: *command == selected,
            }));
        } else if command == CycleVideoTrack {
            if let Some(session) = &self.session {
                let selected = self.video_selection().or(session.video_tracks().preferred);
                rows.extend(session.video_tracks().tracks.iter().enumerate().map(
                    |(index, track)| OptionRow {
                        label: crate::video_tracks::track_label(language, index, track),
                        value: Value::Video(track.id),
                        current: Some(track.id) == selected,
                    },
                ));
            }
        } else if command == CycleAudioTrack {
            let selection = self.audio_selection();
            rows.extend(
                [
                    (AudioTrackSelection::Default, Text::AudioTrackDefault),
                    (AudioTrackSelection::All, Text::AudioTrackAll),
                ]
                .map(|(value, label)| OptionRow {
                    label: label.in_language(language).into(),
                    value: Value::Audio(value),
                    current: value == selection,
                }),
            );
            if let Some(session) = &self.session {
                rows.extend(session.audio_tracks().tracks.iter().enumerate().map(
                    |(index, track)| {
                        let value = AudioTrackSelection::Track(track.id);
                        OptionRow {
                            label: crate::audio_preview::track_label(language, index, track),
                            value: Value::Audio(value),
                            current: value == selection,
                        }
                    },
                ));
            }
        } else if command == LoadSubtitles {
            use crate::subtitles::{Action, Selection};
            let settings = self.subtitle_settings();
            let mut tracks = vec![(
                Selection::None,
                Text::SubtitleNone.in_language(language).to_owned(),
            )];
            if let Some(path) = &settings.external {
                tracks.push((
                    Selection::External,
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                ));
            }
            if let Some(session) = &self.session {
                tracks.extend(session.subtitle_tracks().iter().enumerate().map(
                    |(index, track)| {
                        (
                            Selection::Embedded(track.id),
                            crate::subtitles::track_label(language, index, track),
                        )
                    },
                ));
            }
            rows.extend(tracks.into_iter().map(|(selection, label)| OptionRow {
                label,
                value: Value::Subtitle(Action::Select(selection)),
                current: settings.selection == selection,
            }));
            rows.push(OptionRow {
                label: Text::CommandLoadSubtitles.in_language(language).into(),
                value: Value::Command(LoadSubtitles),
                current: false,
            });
        }
        self.palette.begin_options(
            command,
            language,
            (self.tabs.active_id(), self.media_generation),
            rows,
        );
        self.request_redraw();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filmstrip_layout_options_dispatch_without_shortcut_or_media() {
        let mut app = crate::Application::new(None, |_| {}).expect("app");
        for (command, grid) in [
            (CommandId::FilmstripGrid, true),
            (CommandId::FilmstripRow, false),
        ] {
            app.dispatch(command);
            assert_eq!(app.filmstrip_is_grid(), grid);
            assert!(app.begin_palette_options(CommandId::FilmstripGrid));
            let step = app.palette.step.as_ref().expect("layout choices");
            assert_eq!(step.rows.len(), 2);
            let selected = step
                .rows
                .iter()
                .filter(|row| row.current)
                .collect::<Vec<_>>();
            assert_eq!(selected.len(), 1);
            assert_eq!(selected[0].value, Value::Command(command));
        }
    }

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn options_select_current_filter_without_file_search_and_return_to_parent() {
        let context = crate::fonts::test_context();
        let mut palette = CommandPalette {
            query: ">volume".into(),
            ..Default::default()
        };
        palette.begin_options(
            CommandId::CycleVolumeStep,
            localization::Language::English,
            (None, 42),
            [
                CommandId::VolumeStepTwo,
                CommandId::VolumeStepFive,
                CommandId::VolumeStepTen,
            ]
            .into_iter()
            .enumerate()
            .map(|(index, command)| OptionRow {
                label: format!("Option {index}"),
                value: Value::Command(command),
                current: index == 1,
            })
            .collect(),
        );
        assert!(palette.file_query().is_none());
        assert!(!palette.options_owner_matches((None, 43)));
        let frame = |palette: &mut CommandPalette, events| {
            let mut result = None;
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |_| {
                    result = Some(palette.show_with_sources(
                        &context,
                        CommandContext::default(),
                        &crate::shortcuts::defaults(),
                        0.0,
                        OpenSources::default(),
                    ))
                },
            );
            result.expect("frame")
        };
        frame(&mut palette, vec![]);
        let (choice, closed) = frame(&mut palette, vec![key(egui::Key::Enter)]);
        assert!(!closed);
        assert_eq!(
            choice,
            Some(Choice::Option(
                (None, 42),
                Value::Command(CommandId::VolumeStepFive)
            ))
        );
        palette.query = "Option 2".into();
        frame(&mut palette, vec![]);
        assert_eq!(
            frame(&mut palette, vec![key(egui::Key::Enter)]).0,
            Some(Choice::Option(
                (None, 42),
                Value::Command(CommandId::VolumeStepTen)
            ))
        );
        assert!(!frame(&mut palette, vec![key(egui::Key::Escape)]).1);
        assert!(palette.step.is_none());
        assert_eq!(palette.query, ">volume");
        frame(&mut palette, vec![]);
        assert_eq!(palette.selected_command, Some(CommandId::CycleVolumeStep));
        assert!(frame(&mut palette, vec![key(egui::Key::Escape)]).1);
    }
    #[test]
    fn folder_reuse_palette_exposes_three_modes_and_restores_the_current_choice() {
        let mut app = crate::Application::new(None, |_| {}).expect("app");
        for mode in 0..3 {
            app.reuse_folder_tab = mode;
            assert!(app.begin_palette_options(CommandId::ToggleReuseFolderTab));
            let step = app.palette.step.as_ref().expect("three-way choice");
            assert_eq!(step.rows.len(), 3);
            assert_eq!(step.selected, Some(mode as usize));
            assert_eq!(step.rows.iter().filter(|row| row.current).count(), 1);
            let expected = [
                CommandId::ReuseFolderTabOff,
                CommandId::ReuseFolderTabAll,
                CommandId::ReuseFolderTabSameKind,
            ];
            for (row, command) in step.rows.iter().zip(expected) {
                assert_eq!(row.value, Value::Command(command));
            }
            app.palette.back();
        }
    }
}
