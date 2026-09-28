use super::*;
use towavue_core::AudioTrackSelection;

pub(super) fn submenu(
    ui: &mut egui::Ui,
    context: CommandContext,
    shortcuts: &ShortcutBindings,
    requested: Option<egui::Id>,
    data: &mut MenuData<'_>,
    ancestor: Option<egui::Rect>,
) -> (egui::Response, Option<CommandId>) {
    let root = egui::containers::menu::find_menu_root(ui);
    let parent = ui.ctx().read_response(root.id).expect("parent menu").rect;
    let screen = ui.ctx().content_rect();
    let right = screen.right() - parent.right();
    let left = parent.left() - screen.left();
    let side = if ancestor.is_some_and(|root| root.right() <= parent.left()) {
        right
    } else {
        right.max(left)
    };
    let frame = egui::Frame::popup(ui.style()).total_margin().sum().x;
    let width = (side - frame - 2.0 - 1.0 / ui.ctx().pixels_per_point()).max(1.0);
    let enabled = !context.playback_blocked
        && data
            .audio_tracks
            .is_some_and(|catalog| !catalog.tracks.is_empty());
    let menu = ui
        .add_enabled_ui(enabled, |ui| {
            let category = ui.next_auto_id();
            if requested == Some(category) || crate::logo_menu::drag::submenu(ui, category) {
                let id = egui::containers::menu::SubMenu::id_from_widget_id(category);
                egui::containers::menu::MenuState::mark_shown(ui.ctx(), id);
                egui::containers::menu::MenuState::from_ui(ui, |state, _| {
                    state.open_item = Some(id)
                });
            }
            let button = egui::Button::new(text(ui.ctx(), Text::AudioTracks)).right_text((
                shortcut_text(ui, shortcuts.label(CycleAudioTrack, context), enabled),
                egui::containers::menu::SubMenuButton::RIGHT_ARROW,
            ));
            egui::containers::menu::SubMenuButton::from_button(button).ui(ui, |ui| {
                let keyboard = MenuKeyboard::begin(ui);
                let back = keyboard.left;
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                let mut items = Vec::new();
                let mut command = None;
                let mut rows = vec![
                    (
                        AudioTrackSelection::Default,
                        text(ui.ctx(), Text::AudioTrackDefault).into(),
                    ),
                    (
                        AudioTrackSelection::All,
                        text(ui.ctx(), Text::AudioTrackAll).into(),
                    ),
                ];
                if let Some(catalog) = data.audio_tracks {
                    rows.extend(catalog.tracks.iter().enumerate().map(|(index, track)| {
                        (
                            AudioTrackSelection::Track(track.id),
                            crate::audio_preview::track_label(language(ui.ctx()), index, track),
                        )
                    }));
                }
                let natural = rows
                    .iter()
                    .map(|(_, label)| sizing::text_width(ui, label))
                    .chain(std::iter::once(sizing::text_width(
                        ui,
                        text(ui.ctx(), Text::CommandCycleAudioTrack),
                    )))
                    .fold(0.0, f32::max)
                    + ui.spacing().icon_width
                    + ui.spacing().icon_spacing
                    + 2.0 * ui.spacing().button_padding.x
                    + ui.spacing().scroll.bar_width;
                ui.set_width(natural.min(width));
                let height = sizing::height(ui);
                crate::logo_menu::drag::scroll(ui, egui::Id::new("audio-tracks"), height, |ui| {
                    for (selection, label) in rows {
                        let mut selected = selection == data.audio_selection;
                        let response = ui
                            .checkbox(&mut selected, label.clone())
                            .on_hover_text(label);
                        let response = if selection == AudioTrackSelection::All {
                            response.on_hover_text(text(ui.ctx(), Text::AudioTrackAllWaveform))
                        } else {
                            response
                        };
                        items.push(response.id);
                        if response.gained_focus() {
                            response.scroll_to_me(None);
                        }
                        if crate::logo_menu::drag::clicked(&response) {
                            data.audio_action = Some(selection);
                            ui.close();
                        }
                    }
                    crate::chrome::separator(ui);
                    let response = ui.button(text(ui.ctx(), Text::CommandCycleAudioTrack));
                    items.push(response.id);
                    if crate::logo_menu::drag::clicked(&response) {
                        command = Some(CycleAudioTrack);
                        ui.close();
                    }
                });
                keyboard.finish(ui, items);
                (command, back)
            })
        })
        .inner;
    let (response, contents) = menu;
    let (chosen, back) = contents.map(|inner| inner.inner).unwrap_or_default();
    if back {
        egui::containers::menu::MenuState::from_ui(ui, |state, _| state.open_item = None);
        response.request_focus();
    }
    (response, chosen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::{Action, ActionRequest, TreeId};
    use towavue_core::{AudioTrack, AudioTrackCatalog, AudioTrackId, MediaKind};

    #[test]
    fn audio_track_menu_selects_tracks_and_all_with_bounded_localized_rows() {
        for density in [1.0, 1.25, 2.0] {
            for width in [480.0, 1200.0] {
                for japanese in [false, true] {
                    for choose_all in [false, true] {
                        let context = if japanese {
                            crate::localization::test_ui::japanese_context(density)
                        } else {
                            crate::fonts::test_context()
                        };
                        context.set_pixels_per_point(density);
                        context.enable_accesskit();
                        context.global_style_mut(crate::chrome::style);
                        let track = AudioTrack {
                            id: AudioTrackId::from_index(3),
                            title: Some("Long commentary description ".repeat(8)),
                            language: Some("eng".into()),
                        };
                        let catalog = AudioTrackCatalog {
                            tracks: vec![track.clone()],
                            preferred: Some(track.id),
                        };
                        let chosen = std::cell::Cell::new(None);
                        let parent = std::cell::Cell::new(egui::Rect::NOTHING);
                        let frame = |events| {
                            let mut data = MenuData {
                                audio_tracks: Some(&catalog),
                                ..Default::default()
                            };
                            let output = context.run_ui(
                                egui::RawInput {
                                    screen_rect: Some(egui::Rect::from_min_size(
                                        egui::Pos2::ZERO,
                                        egui::vec2(width, 700.0),
                                    )),
                                    events,
                                    ..Default::default()
                                },
                                |ui| {
                                    ui.menu_button("Menu", |ui| {
                                        let root = egui::containers::menu::find_menu_root(ui);
                                        parent.set(
                                            ui.ctx().read_response(root.id).expect("parent").rect,
                                        );
                                        show_items(
                                            ui,
                                            Text::MenuView,
                                            CommandContext {
                                                media_kind: Some(MediaKind::Video),
                                                ..Default::default()
                                            },
                                            &crate::shortcuts::defaults(),
                                            &mut data,
                                            None,
                                        );
                                    });
                                },
                            );
                            if data.audio_action.is_some() {
                                chosen.set(data.audio_action);
                            }
                            output
                        };
                        let click = |id| {
                            egui::Event::AccessKitActionRequest(ActionRequest {
                                action: Action::Click,
                                target_tree: TreeId::ROOT,
                                target_node: id,
                                data: None,
                            })
                        };
                        // egui concatenates the caption, shortcut and submenu arrow
                        // in the accessibility label, retaining all button atoms.
                        for label in [
                            "Menu".to_owned(),
                            format!("{} Alt+A", text(&context, Text::AudioTracks)),
                        ] {
                            for _ in 0..4 {
                                frame(vec![]);
                            }
                            let output = frame(vec![]);
                            let tree = output.platform_output.accesskit_update.expect("tree");
                            let id = tree
                                .nodes
                                .iter()
                                .find(|(_, node)| {
                                    node.label().is_some_and(|value| {
                                        value.trim_end_matches('⏵').trim() == label
                                    })
                                })
                                .unwrap_or_else(|| {
                                    panic!(
                                        "missing {label}: {:?}",
                                        tree.nodes
                                            .iter()
                                            .filter_map(|(_, node)| node.label())
                                            .collect::<Vec<_>>()
                                    )
                                })
                                .0;
                            frame(vec![click(id)]);
                        }
                        for _ in 0..4 {
                            frame(vec![]);
                        }
                        let output = frame(vec![]);
                        let tree = output.platform_output.accesskit_update.expect("track menu");
                        let label =
                            crate::audio_preview::track_label(language(&context), 0, &track);
                        let row = tree
                            .nodes
                            .iter()
                            .find(|(_, node)| node.label() == Some(label.as_str()))
                            .expect("full track label retained for accessibility");
                        let bounds = row.1.bounds().expect("row bounds");
                        assert!(bounds.x0 >= 0.0 && bounds.x1 <= f64::from(width) + 1.0);
                        assert!(
                            bounds.x0 >= f64::from(parent.get().right())
                                || bounds.x1 <= f64::from(parent.get().left()),
                            "track submenu overlaps parent: {bounds:?} {:?}",
                            parent.get()
                        );
                        let id = if choose_all {
                            tree.nodes
                                .iter()
                                .find(|(_, node)| {
                                    node.label() == Some(text(&context, Text::AudioTrackAll))
                                })
                                .expect("all row")
                                .0
                        } else {
                            row.0
                        };
                        frame(vec![click(id)]);
                        assert_eq!(
                            chosen.get(),
                            Some(if choose_all {
                                AudioTrackSelection::All
                            } else {
                                AudioTrackSelection::Track(track.id)
                            })
                        );
                    }
                }
            }
        }
    }
}
