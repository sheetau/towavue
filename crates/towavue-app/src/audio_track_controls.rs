use crate::*;

mod icon;
use towavue_core::{AudioTrackSelection, AudioTrackTarget};

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn toggle_audio_track_output(&mut self) {
        if self.modal_input_blocked()
            || !self.timeline_is_visible()
            || self.media_kind != Some(MediaKind::Video)
        {
            return;
        }
        let Some(catalog) = self.session.as_ref().map(PlaybackSession::audio_tracks) else {
            return;
        };
        if catalog.tracks.is_empty() {
            return;
        }
        let output = self.edit_state().audio_output;
        let target = match self.audio_selection() {
            AudioTrackSelection::All => AudioTrackTarget::All,
            AudioTrackSelection::Track(id) => AudioTrackTarget::Track(id),
            AudioTrackSelection::Default => {
                let Some(id) = catalog.preferred else { return };
                AudioTrackTarget::Track(id)
            }
        };
        let enabled = match target {
            AudioTrackTarget::All => catalog
                .tracks
                .iter()
                .all(|track| !output.is_enabled(track.id)),
            AudioTrackTarget::Track(id) => !output.is_enabled(id),
        };
        self.push_edit(EditOperation::SetAudioTrackOutput(target, enabled));
    }

    pub(super) fn waveform_track_muted(&self) -> bool {
        if self.media_kind != Some(MediaKind::Video) {
            return false;
        }
        let track = self
            .waveform_audio_track()
            .or_else(|| self.session.as_ref()?.audio_tracks().preferred);
        track.is_some_and(|id| !self.edit_state().audio_output.is_enabled(id))
    }

    pub(super) fn draw_waveform_track_hint(
        &self,
        ui: &egui::Ui,
        rect: egui::Rect,
        result: &time_selection::Output,
    ) {
        if self.audio_selection() != AudioTrackSelection::All
            || self.waveform_loading
            || self.waveform_detail.is_pending()
            || result.display_selection.is_some()
            || result.gain_preview.is_some()
            || result.stretch_preview.is_some()
            || result.scrub.is_some()
            || timeline_input::is_active(ui.ctx())
            || seekbar::has_value_focus(ui.ctx())
            || egui::Popup::is_any_open(ui.ctx())
            || ui.input(|input| input.pointer.any_down())
        {
            return;
        }
        let Some(track) = self.waveform_audio_track() else {
            return;
        };
        let Some(index) = self.session.as_ref().and_then(|session| {
            session
                .audio_tracks()
                .tracks
                .iter()
                .position(|entry| entry.id == track)
        }) else {
            return;
        };
        let label =
            towavue_core::localization::formatted::shown_waveform_track(self.language(), index + 1);
        ui.painter().with_clip_rect(rect).text(
            rect.left_bottom() + egui::vec2(time_selection::LABEL_INSET, -10.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(time_selection::LABEL_SIZE),
            chrome::FOREGROUND,
        );
    }

    pub(super) fn draw_audio_track_controls(&self, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        if !self.timeline_is_visible() || self.media_kind != Some(MediaKind::Video) {
            return;
        }
        let Some(catalog) = self
            .session
            .as_ref()
            .map(PlaybackSession::audio_tracks)
            .filter(|catalog| !catalog.tracks.is_empty())
        else {
            return;
        };
        let output = self.edit_state().audio_output;
        let _ = Controls {
            catalog,
            output: &output,
            selection: self.audio_selection(),
            owner: (self.displayed_tab, self.media_generation),
            language: self.language(),
        }
        .show(ui, actions);
    }
}

struct Controls<'a> {
    catalog: &'a towavue_core::AudioTrackCatalog,
    output: &'a towavue_core::AudioTrackOutput,
    selection: AudioTrackSelection,
    owner: (Option<TabId>, u64),
    language: localization::Language,
}

impl Controls<'_> {
    fn show(&self, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) -> egui::Response {
        let &Self {
            catalog,
            output,
            selection,
            owner,
            language,
        } = self;
        let selected = match selection {
            AudioTrackSelection::Default => catalog.preferred,
            AudioTrackSelection::Track(id) => Some(id),
            AudioTrackSelection::All => None,
        };
        let label = if selection == AudioTrackSelection::All {
            "All".to_owned()
        } else {
            format!(
                "A{}",
                catalog
                    .tracks
                    .iter()
                    .position(|track| Some(track.id) == selected)
                    .unwrap_or(0)
                    + 1
            )
        };
        let multiple = catalog.tracks.len() > 1;
        let muted = if selection == AudioTrackSelection::All {
            catalog
                .tracks
                .iter()
                .all(|track| !output.is_enabled(track.id))
        } else {
            selected.is_some_and(|id| !output.is_enabled(id))
        };
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(5, 0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = if multiple { 2.0 } else { 5.0 };
                    let font = egui::FontId::proportional(chrome::TEXT_LG);
                    let text_width = ui
                        .painter()
                        .layout_no_wrap(label.clone(), font.clone(), chrome::MUTED)
                        .size()
                        .x;
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(text_width + if multiple { 18.0 } else { 0.0 }, 24.0),
                        egui::Sense::hover(),
                    );
                    if multiple {
                        let popup_id = ui.id().with(("audio-track-popup", owner));
                        let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
                        let response = crate::browser_controls::text_button_color(
                            ui,
                            rect,
                            ui.id().with(("audio-track", owner)),
                            &label,
                            false,
                            if open {
                                chrome::FOREGROUND
                            } else {
                                chrome::MUTED
                            },
                        );
                        ui.painter().text(
                            egui::pos2(rect.right() - 8.0, rect.center().y),
                            egui::Align2::CENTER_CENTER,
                            if open { "\u{eab7}" } else { "\u{eab4}" },
                            crate::fonts::icon_font(),
                            if open || response.hovered() || response.has_focus() {
                                chrome::FOREGROUND
                            } else {
                                chrome::MUTED
                            },
                        );
                        chrome::menu_popup(&response, popup_id)
                            .align(egui::RectAlign::TOP_START)
                            .gap(2.0)
                            .style(chrome::menu_style)
                            .show(|ui| {
                                let all_muted = catalog
                                    .tracks
                                    .iter()
                                    .all(|track| !output.is_enabled(track.id));
                                let all = localization::Text::AudioTrackAll.in_language(language);
                                let mut choices = vec![(
                                    AudioTrackSelection::All,
                                    if all_muted {
                                        format!("{all} (Mute)")
                                    } else {
                                        all.to_owned()
                                    },
                                    selection == AudioTrackSelection::All,
                                )];
                                choices.extend(catalog.tracks.iter().enumerate().map(
                                    |(index, track)| {
                                        let mut label =
                                            audio_preview::track_label(language, index, track);
                                        if !output.is_enabled(track.id) {
                                            label.push_str(" (Mute)");
                                        }
                                        (
                                            AudioTrackSelection::Track(track.id),
                                            label,
                                            selected == Some(track.id),
                                        )
                                    },
                                ));
                                let font = egui::TextStyle::Button.resolve(ui.style());
                                let width = choices
                                    .iter()
                                    .map(|(_, label, _)| {
                                        ui.painter()
                                            .layout_no_wrap(
                                                label.clone(),
                                                font.clone(),
                                                chrome::FOREGROUND,
                                            )
                                            .size()
                                            .x
                                    })
                                    .fold(0.0, f32::max)
                                    + ui.spacing().interact_size.y
                                    + 2.0 * ui.spacing().button_padding.x;
                                ui.set_width(width.ceil().min(ui.available_width()));
                                egui::ScrollArea::vertical()
                                    .max_height(
                                        (rect.top() - ui.ctx().content_rect().top() - 12.0)
                                            .max(22.0),
                                    )
                                    .show(ui, |ui| {
                                        for (value, label, mut checked) in choices {
                                            if chrome::menu_checkbox(ui, &mut checked, label)
                                                .clicked()
                                            {
                                                actions.push(UiAction::AudioTrack(
                                                    owner.0, owner.1, value,
                                                ));
                                            }
                                        }
                                    });
                            });
                    } else {
                        let response = ui.interact(
                            rect,
                            ui.id().with("audio-track-label"),
                            egui::Sense::hover(),
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Label,
                                ui.is_enabled(),
                                &label,
                            )
                        });
                        ui.painter().text(
                            rect.left_center(),
                            egui::Align2::LEFT_CENTER,
                            &label,
                            font,
                            chrome::MUTED,
                        );
                    }
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(16.0, 24.0), egui::Sense::click());
                    let title = localization::Text::MuteAudioTrack.in_language(language);
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Button,
                            response.enabled(),
                            muted,
                            title,
                        )
                    });
                    paint_mute(ui, rect, muted, response.hovered() || response.has_focus());
                    if response
                        .help_text(title)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        actions.push(UiAction::Command(CommandId::MuteAudioTrack));
                    }
                });
            })
            .response
    }
}

fn paint_mute(ui: &egui::Ui, rect: egui::Rect, muted: bool, hot: bool) {
    // Geometry matches the centered 7-by-7 polyline in assets/mute-track.svg.
    let icon = egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0));
    let ink = if muted || hot {
        chrome::FOREGROUND
    } else {
        chrome::FAINT
    };
    ui.painter().rect_filled(
        icon,
        3,
        if muted {
            chrome::FAINT
        } else {
            chrome::SURFACE
        },
    );
    icon::paint(ui, icon, ink);
}

#[cfg(test)]
mod tests {
    use super::*;

    use towavue_core::{AudioTrack, AudioTrackCatalog, AudioTrackId, AudioTrackOutput};

    #[test]
    fn controls_match_group_insets_hit_boxes_and_checkbox_popup() {
        for density in [1.0, 1.25, 2.0] {
            let context = fonts::test_context();
            context.set_pixels_per_point(density);
            context.enable_accesskit();
            context.global_style_mut(chrome::style);
            let track = AudioTrackId::from_index(1);
            let catalog = AudioTrackCatalog {
                tracks: vec![
                    AudioTrack {
                        id: track,
                        title: None,
                        language: None,
                    },
                    AudioTrack {
                        id: AudioTrackId::from_index(2),
                        title: None,
                        language: None,
                    },
                ],
                preferred: Some(track),
            };
            let mut output = AudioTrackOutput::default();
            output.set_enabled(AudioTrackTarget::Track(track), false);
            let controls = Controls {
                catalog: &catalog,
                output: &output,
                selection: AudioTrackSelection::Default,
                owner: (None, 0),
                language: localization::Language::English,
            };
            let mut time = 0.0;
            let mut draw = |events| {
                time += 1.0 / 60.0;
                let mut bounds = egui::Rect::NOTHING;
                let result = context.run_ui(
                    egui::RawInput {
                        time: Some(time),
                        events,
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(400.0, 240.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        crate::overlay_input::dismiss_menu_on_outside_press(ui.ctx());
                        ui.add_space(190.0);
                        bounds = controls.show(ui, &mut Vec::new()).rect;
                    },
                );
                (result, bounds)
            };
            draw(vec![]);
            let (result, group) = draw(vec![]);
            let rect = |result: &egui::FullOutput, label| {
                let bounds = result
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("tree")
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some(label))
                    .expect("control")
                    .1
                    .bounds()
                    .expect("bounds");
                egui::Rect::from_min_max(
                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                    egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                )
            };
            let selector = rect(&result, "A1");
            let mute = rect(&result, "Mute track");
            assert_eq!(mute.size(), egui::vec2(16.0, 24.0));
            assert_eq!(selector.left() - group.left(), 5.0);
            assert_eq!(group.right() - mute.right(), 5.0);
            assert_eq!(mute.left() - selector.right(), 2.0);
            let background = result
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Rect(shape)
                        if shape.fill == chrome::FAINT && mute.contains_rect(shape.rect) =>
                    {
                        Some(shape.rect)
                    }
                    _ => None,
                })
                .expect("muted background");
            let glyph = result
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.calc_bounds() == background => Some(mesh),
                    _ => None,
                })
                .expect("single rasterized M");
            assert_eq!(glyph.calc_bounds().center(), background.center());
            let point = selector.center();
            let button = |pressed| egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            draw(vec![egui::Event::PointerMoved(point), button(true)]);
            draw(vec![button(false)]);
            draw(vec![egui::Event::PointerMoved(egui::pos2(390.0, 230.0))]);
            let (result, _) = draw(vec![]);
            let nodes = &result
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("menu tree")
                .nodes;
            let choices: Vec<_> = nodes
                .iter()
                .filter(|(_, node)| node.role() == egui::accesskit::Role::CheckBox)
                .collect();
            assert_eq!(choices.len(), 3);
            assert!(
                choices
                    .iter()
                    .any(|(_, node)| node.label().is_some_and(|label| label.ends_with("(Mute)")))
            );
            assert!(
                choices
                    .iter()
                    .all(|(_, node)| node.bounds().expect("row").width() < 210.0),
                "menu hugs its short labels"
            );
            for label in ["A1", "\u{eab7}"] {
                let text = result
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text() == label => Some(text),
                        _ => None,
                    })
                    .expect("open selector");
                assert!(
                    text.galley
                        .job
                        .sections
                        .iter()
                        .all(|section| section.format.color == chrome::FOREGROUND)
                        || text.fallback_color == chrome::FOREGROUND
                );
            }
            draw(vec![egui::Event::PointerMoved(point), button(true)]);
            assert!(
                !egui::Popup::is_any_open(&context),
                "host prepass leaves repress closure to the opener"
            );
            draw(vec![button(false)]);
            assert!(
                !egui::Popup::is_any_open(&context),
                "release does not reopen track selector"
            );
            draw(vec![button(true)]);
            draw(vec![button(false)]);
            assert!(
                egui::Popup::is_any_open(&context),
                "a fresh click opens again"
            );
        }
    }
}
