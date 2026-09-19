use crate::*;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Owner {
    pub tab: TabId,
    pub instance: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    Gallery,
    Filmstrip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Open,
    Window,
    Tab,
    Copy,
    Reveal,
    RemoveHistory,
    File(file_operations::Kind),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Intent {
    pub owner: Option<Owner>,
    pub scope: Scope,
    pub path: PathBuf,
    pub action: Action,
}

pub(crate) fn show(
    ui: &egui::Ui,
    response: &egui::Response,
    path: &Path,
    scope: Scope,
    owner: Option<Owner>,
    actions: &mut Vec<UiAction>,
) {
    let popup = egui::Popup::default_response_id(response);
    let owner_id = popup.with("thumbnail-owner");
    if egui::Popup::is_id_open(ui.ctx(), popup)
        && ui
            .ctx()
            .data(|data| data.get_temp::<Option<Owner>>(owner_id))
            != Some(owner)
    {
        egui::Popup::close_id(ui.ctx(), popup);
        return;
    }
    let chosen = tab_menu::popup(ui, response, response, |ui| {
        chrome::flat_buttons(ui);
        ui.set_min_width(170.0);
        let keyboard = menu::MenuKeyboard::begin(ui);
        let mut items = Vec::new();
        let mut chosen = None;
        for (label, action) in [
            ("Open", Action::Open),
            ("Open in new window", Action::Window),
            ("Open in new tab", Action::Tab),
        ] {
            let response = ui.button(label);
            items.push(response.id);
            if response.clicked() {
                chosen = Some(action);
            }
        }
        chrome::separator(ui);
        for (label, action) in [
            ("Copy file path", Action::Copy),
            ("Reveal in File Explorer", Action::Reveal),
        ] {
            let response = ui.button(label);
            items.push(response.id);
            if response.clicked() {
                chosen = Some(action);
            }
        }
        chrome::separator(ui);
        match scope {
            Scope::Gallery => {
                let response = ui.button("Remove from history");
                items.push(response.id);
                if response.clicked() {
                    chosen = Some(Action::RemoveHistory);
                }
            }
            Scope::Filmstrip => {
                for (label, kind) in [
                    ("Rename file…", file_operations::Kind::Rename),
                    ("Move file…", file_operations::Kind::Move),
                    ("Delete file…", file_operations::Kind::Delete),
                ] {
                    let response = ui.button(label);
                    items.push(response.id);
                    if response.clicked() {
                        chosen = Some(Action::File(kind));
                    }
                }
            }
        }
        keyboard.finish(ui, items);
        if chosen.is_some() {
            ui.close();
        }
        chosen
    });
    if egui::Popup::is_id_open(ui.ctx(), popup) {
        ui.ctx().data_mut(|data| data.insert_temp(owner_id, owner));
    } else {
        ui.ctx()
            .data_mut(|data| data.remove::<Option<Owner>>(owner_id));
    }
    if let Some((action, _)) = chosen {
        actions.push(UiAction::ThumbnailMenu(Intent {
            owner,
            scope,
            path: path.to_owned(),
            action,
        }));
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn handle_thumbnail_menu(&mut self, intent: Intent) {
        let Some(owner) = intent.owner else {
            return;
        };
        if self.tabs.active_id() != Some(owner.tab)
            || self.media_generation != owner.instance
            || self.modal_input_blocked()
            || self.palette_open
            || self.grid_open
            || self
                .ui_context
                .as_ref()
                .is_some_and(egui::Popup::is_any_open)
        {
            return;
        }
        let path = intent.path;
        match intent.scope {
            Scope::Gallery => {
                if self.tabs.gallery() != Some(owner.tab)
                    || !self.recent_paths.contains(&path)
                    || self.gallery_missing_files.contains(&path)
                    || !welcome::matches(&path, &self.gallery_search)
                    || self
                        .gallery_filter
                        .is_some_and(|kind| MediaKind::from_path(&path) != Some(kind))
                {
                    return;
                }
            }
            Scope::Filmstrip if self.filmstrip_target(&path).is_none() => return,
            Scope::Filmstrip => {}
        }
        match intent.action {
            Action::Copy => {
                if let Some(context) = &self.ui_context {
                    context.copy_text(path.display().to_string());
                }
            }
            Action::Reveal => self.reveal_path(path),
            Action::RemoveHistory if intent.scope == Scope::Gallery => self.handle_recent_action(
                menu::RecentAction::Remove(path, towavue_runtime_windows::RecentKind::File),
            ),
            Action::File(kind) if intent.scope == Scope::Filmstrip => {
                self.begin_file_relocation_at(kind, path)
            }
            Action::Open if intent.scope == Scope::Filmstrip => {
                self.handle_ui_action(UiAction::OpenFilmstripMedia(path, false))
            }
            Action::Tab => self.handle_ui_action(if intent.scope == Scope::Gallery {
                UiAction::OpenGalleryBackground(path)
            } else {
                UiAction::OpenFilmstripMedia(path, true)
            }),
            Action::Open | Action::Window => self.handle_recent_action(menu::RecentAction::Open(
                path,
                towavue_runtime_windows::RecentKind::File,
                if intent.action == Action::Window {
                    menu::OpenTarget::Window
                } else {
                    menu::OpenTarget::Replace
                },
            )),
            Action::RemoveHistory | Action::File(_) => {}
        }
    }
}
