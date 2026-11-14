use super::*;
use crate::hover_help::HoverHelp;

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
            .video_tracks
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
            let button = egui::Button::new(text(ui.ctx(), Text::VideoTracks)).right_text((
                shortcut_text(ui, shortcuts.label(CycleVideoTrack, context), enabled),
                crate::chrome::chevron(),
            ));
            crate::chrome::submenu_button(ui, button, |ui| {
                let keyboard = MenuKeyboard::begin(ui);
                let back = keyboard.left;
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                let mut items = Vec::new();
                let mut command = None;
                let rows: Vec<_> = data
                    .video_tracks
                    .into_iter()
                    .flat_map(|catalog| catalog.tracks.iter().enumerate())
                    .map(|(index, track)| {
                        (
                            track.id,
                            crate::video_tracks::track_label(language(ui.ctx()), index, track),
                        )
                    })
                    .collect();
                let natural = rows
                    .iter()
                    .map(|(_, label)| sizing::text_width(ui, label))
                    .chain(std::iter::once(sizing::text_width(
                        ui,
                        text(ui.ctx(), Text::CommandCycleVideoTrack),
                    )))
                    .fold(0.0, f32::max)
                    + ui.spacing().interact_size.y
                    + 2.0 * ui.spacing().button_padding.x;
                let natural = crate::logo_menu::drag::scroll_width(
                    ui,
                    egui::Id::new("video-tracks"),
                    natural,
                );
                ui.set_width(natural.min(width));
                let height = sizing::height(ui);
                crate::logo_menu::drag::scroll(ui, egui::Id::new("video-tracks"), height, |ui| {
                    for (selection, label) in rows {
                        let mut selected = Some(selection)
                            == data.video_selection.or_else(|| {
                                data.video_tracks.and_then(|catalog| catalog.preferred)
                            });
                        let response =
                            crate::chrome::menu_checkbox(ui, &mut selected, label.clone())
                                .help_text(label);
                        items.push((response.id, response.enabled()));
                        if response.gained_focus() {
                            response.scroll_to_me(None);
                        }
                        if crate::logo_menu::drag::clicked(&response) {
                            data.video_action = Some(selection);
                        }
                    }
                    crate::chrome::separator(ui);
                    let response = ui.button(text(ui.ctx(), Text::CommandCycleVideoTrack));
                    items.push((response.id, response.enabled()));
                    if crate::logo_menu::drag::clicked(&response) {
                        command = Some(CycleVideoTrack);
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
#[path = "video_tracks_tests.rs"]
mod tests;
