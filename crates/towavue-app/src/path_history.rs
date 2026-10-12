use super::*;

const LIMIT: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Target {
    File(PathBuf),
    Folder(PathBuf),
}

/// Navigation retains paths only. Decoder, preview and undo ownership is unchanged.
#[derive(Default)]
pub(super) struct History {
    entries: Vec<Target>,
    cursor: usize,
    revision: u64,
    current: Option<PathBuf>,
    at_cursor: bool,
    restored: Option<PathBuf>,
}

impl History {
    pub fn begin(&mut self, path: &Path) {
        if self.current.as_deref() != Some(path) {
            self.current = Some(path.to_owned());
            self.restored = None;
            self.at_cursor = self.entries.get(self.cursor) == Some(&Target::File(path.to_owned()));
        }
    }

    pub fn qualify(&mut self, path: &Path) {
        self.begin(path);
        if self.restored.as_deref() != Some(path) {
            self.record(Target::File(path.to_owned()));
        }
    }

    pub fn record(&mut self, target: Target) {
        if self.entries.get(self.cursor) != Some(&target) {
            self.entries.truncate(self.cursor + 1);
            self.entries.push(target);
            if self.entries.len() > LIMIT {
                self.entries.remove(0);
            }
            self.cursor = self.entries.len() - 1;
            self.revision = self.revision.wrapping_add(1);
        }
        self.at_cursor = true;
    }

    fn index(&self, forward: bool) -> Option<usize> {
        if self.entries.is_empty() {
            return None;
        }
        if forward {
            (self.cursor + 1 < self.entries.len()).then_some(self.cursor + 1)
        } else if !self.at_cursor {
            Some(self.cursor)
        } else {
            self.cursor.checked_sub(1)
        }
    }

    fn commit(&mut self, request: &Request, path: &Path) {
        self.cursor = request.index;
        self.current = Some(path.to_owned());
        self.restored = Some(path.to_owned());
        self.at_cursor = true;
        self.revision = self.revision.wrapping_add(1);
    }
}

#[derive(Clone)]
pub(super) struct Request {
    tab: TabId,
    generation: u64,
    revision: u64,
    index: usize,
    target: Target,
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn history_available(&self, forward: bool) -> bool {
        self.tabs
            .active_id()
            .and_then(|id| self.path_history.get(&id))
            .and_then(|history| history.index(forward))
            .is_some()
    }

    pub(super) fn navigate_history(&mut self, forward: bool) {
        if self.modal_input_blocked() || !self.can_replace_active_tab() {
            return;
        }
        let Some(tab) = self.tabs.active_id() else {
            return;
        };
        let Some(history) = self.path_history.get(&tab) else {
            return;
        };
        let Some(index) = history.index(forward) else {
            return;
        };
        let request = Request {
            tab,
            generation: self.media_generation,
            revision: history.revision,
            index,
            target: history.entries[index].clone(),
        };
        match &request.target {
            Target::File(path) => {
                // Resolve through the same Shell validation as an ordinary open.
                match towavue_runtime_windows::canonical_shell_path(path) {
                    Ok(path) => self.request_guarded(GuardedAction::NavigateHistory(request, path)),
                    Err(error) => {
                        self.set_status(towavue_core::localization::formatted::open_failed(
                            self.language(),
                            &path.display().to_string(),
                            &error.to_string(),
                        ))
                    }
                }
            }
            Target::Folder(folder) => {
                self.open_folder_path(folder.clone());
                if let Some((_, intent)) = &mut self.pending_folder {
                    *intent = FolderIntent::History(request);
                }
            }
        }
    }

    pub(super) fn history_request_valid(&self, request: &Request) -> bool {
        self.tabs.active_id() == Some(request.tab)
            && self.media_generation == request.generation
            && self.path_history.get(&request.tab).is_some_and(|history| {
                history.revision == request.revision
                    && history.entries.get(request.index) == Some(&request.target)
            })
    }

    pub(super) fn finish_history_navigation(&mut self, request: Request, path: PathBuf) {
        // A successful Save may replace the media instance while the modal owns
        // this continuation. The tab and history revision remain its identity.
        if self.tabs.active_id() != Some(request.tab)
            || self.path_history.get(&request.tab).is_none_or(|history| {
                history.revision != request.revision
                    || history.entries.get(request.index) != Some(&request.target)
            })
        {
            return;
        }
        self.navigate_to_unchecked(path.clone());
        if self.path.as_ref() == Some(&path) {
            self.path_history
                .entry(request.tab)
                .or_default()
                .commit(&request, &path);
            self.begin_history_visit(request.tab, &path);
        }
    }

    pub(super) fn record_folder_navigation(&mut self, folder: PathBuf) {
        if let Some(id) = self.tabs.active_id() {
            self.path_history
                .entry(id)
                .or_default()
                .record(Target::Folder(folder));
        }
    }

    pub(super) fn open_background_folder_media(&mut self, snapshot: &FolderSnapshot) {
        let Some(first) = snapshot.items.first() else {
            return;
        };
        let Some(active) = self.tabs.active_id() else {
            return;
        };
        let added = self.tabs.open_new(first.path.clone(), first.kind);
        self.seed_playback_volume(added);
        self.begin_history_visit(added, &first.path);
        let history = self.path_history.entry(added).or_default();
        history.begin(&first.path);
        history.record(Target::Folder(snapshot.folder_path.clone()));
        self.edits.entry(added).or_default();
        self.tabs.activate(active);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_history_uses_dirty_guards_preserves_forward_and_discards_closed_owners() {
        let Some(root) = crate::tests::isolated_test_root(
            "path_history::tests::tab_history_uses_dirty_guards_preserves_forward_and_discards_closed_owners",
        ) else {
            return;
        };
        let paths: Vec<_> = ["a.bmp", "b.bmp", "c.bmp"]
            .into_iter()
            .map(|name| root.join(name))
            .collect();
        for path in &paths {
            tab_transfer::tests::bitmap(path);
        }
        let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
        app.ui_context = Some(fonts::test_context());
        let a = tab_transfer::tests::install(
            &mut app,
            paths[0].clone(),
            tab_transfer::tests::decoded(false),
        );
        app.qualify_current_history();
        app.navigate_to_unchecked(paths[1].clone());
        app.qualify_current_history();
        // Seed a dirty document independently of asynchronous image presentation.
        app.edits
            .entry(a)
            .or_default()
            .push(EditOperation::RotateClockwise, MediaKind::Image);
        let context = app.ui_context.as_ref().expect("context").clone();
        let mut search = String::from("filter");
        for _ in 0..2 {
            let _ = context.run_ui(egui::RawInput::default(), |ui| {
                ui.text_edit_singleline(&mut search).request_focus();
            });
        }
        assert!(context.text_edit_focused());
        let back = "Alt+Left".parse().expect("history key");
        assert!(app.owns_focused_shortcut(&back));
        app.process_shortcut(back);
        assert!(matches!(
            app.pending_guard,
            Some(GuardedAction::NavigateHistory(..))
        ));
        app.resolve_guard(GuardDecision::Cancel);
        assert_eq!(app.path.as_ref(), Some(&paths[1]));
        assert!(!app.history_available(true));
        app.navigate_history(false);
        app.resolve_guard(GuardDecision::Discard);
        assert_eq!(app.path.as_ref(), Some(&paths[0]));
        assert_eq!(app.tabs.active_id(), Some(a));
        assert!(app.history_available(true));
        app.qualify_current_history();
        assert!(app.history_available(true));
        let b = tab_transfer::tests::install(
            &mut app,
            paths[2].clone(),
            tab_transfer::tests::decoded(false),
        );
        app.qualify_current_history();
        assert!(!app.history_available(false));
        assert!(!app.history_available(true));
        app.activate_tab(a);
        assert!(app.history_available(true));
        app.navigate_history(true);
        assert_eq!(app.path.as_ref(), Some(&paths[1]));
        app.remove_tab(b, false);
        assert!(!app.path_history.contains_key(&b));
        app.remove_tab(a, false);
        assert!(app.path_history.is_empty());
    }

    #[test]
    fn folder_middle_open_is_background_only_and_empty_folders_do_nothing() {
        let Some(root) = crate::tests::isolated_test_root(
            "path_history::tests::folder_middle_open_is_background_only_and_empty_folders_do_nothing",
        ) else {
            return;
        };
        let path = root.join("current.bmp");
        tab_transfer::tests::bitmap(&path);
        let folder = root.join("other");
        std::fs::create_dir(&folder).expect("folder");
        tab_transfer::tests::bitmap(&folder.join("first.bmp"));
        let empty = root.join("empty");
        std::fs::create_dir(&empty).expect("empty");
        let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
        app.ui_context = Some(fonts::test_context());
        let owner = tab_transfer::tests::install(
            &mut app,
            path.clone(),
            tab_transfer::tests::decoded(false),
        );
        app.filmstrip_open = true;
        app.push_visual_edit(EditOperation::RotateClockwise);
        let generation = app.media_generation;
        let settle = |app: &mut Application<fn(AppEvent)>| {
            let deadline = Instant::now() + Duration::from_secs(30);
            while app.pending_folder.is_some() {
                app.finish_folder_load();
                assert!(Instant::now() < deadline, "folder enumeration");
                std::thread::sleep(Duration::from_millis(2));
            }
        };
        app.handle_ui_action(UiAction::FilmstripFolder(
            Some(owner),
            generation,
            empty,
            true,
        ));
        settle(&mut app);
        assert_eq!(app.tabs.tabs().len(), 1);
        assert_eq!(
            app.status_notice().as_deref(),
            Some(
                towavue_core::localization::formatted::no_folder_media(app.language(), "empty")
                    .as_str()
            )
        );
        app.handle_ui_action(UiAction::FilmstripFolder(
            Some(owner),
            generation,
            folder.clone(),
            true,
        ));
        settle(&mut app);
        assert_eq!(app.tabs.tabs().len(), 2);
        assert_eq!(app.tabs.active_id(), Some(owner));
        assert_eq!(app.displayed_tab, Some(owner));
        assert_eq!(app.path.as_ref(), Some(&path));
        assert_eq!(app.media_generation, generation);
        assert!(app.pending_guard.is_none());
        assert!(app.edits[&owner].is_dirty());
        assert!(app.retained_images.is_empty());
        assert!(app.retained_playback.is_empty());
        let added = app
            .tabs
            .tabs()
            .iter()
            .find(|tab| tab.id != owner)
            .expect("new tab")
            .id;
        assert_eq!(
            app.path_history[&added].entries,
            [Target::Folder(folder.clone())]
        );
        assert!(app.filmstrip_open);

        app.handle_ui_action(UiAction::FilmstripFolder(
            Some(owner),
            generation,
            folder.clone(),
            false,
        ));
        settle(&mut app);
        assert!(matches!(
            app.pending_guard,
            Some(GuardedAction::NavigateFromFolder(..))
        ));
        app.resolve_guard(GuardDecision::Discard);
        assert_eq!(
            app.path_history[&owner].entries,
            [Target::File(path.clone()), Target::Folder(folder.clone())]
        );
        app.navigate_history(false);
        assert_eq!(app.path.as_ref(), Some(&path));
        app.qualify_current_history();
        app.navigate_history(true);
        settle(&mut app);
        assert_eq!(app.path.as_ref(), Some(&folder.join("first.bmp")));
        app.qualify_current_history();
        assert_eq!(
            app.path_history[&owner].entries.len(),
            2,
            "folder traversal keeps its branch after qualification"
        );
    }

    #[test]
    fn transient_paths_do_not_fill_history_and_back_returns_last_qualified_visit() {
        let mut history = History::default();
        history.qualify(Path::new("a.png"));
        for i in 0..100 {
            history.begin(Path::new(&format!("transient-{i}.png")));
        }
        assert_eq!(history.entries.len(), 1);
        assert_eq!(history.index(false), Some(0));
        history.qualify(Path::new("b.png"));
        assert_eq!(history.index(false), Some(0));
    }

    #[test]
    fn history_is_bounded_and_restored_dwell_preserves_forward_entries() {
        let mut history = History::default();
        for i in 0..60 {
            history.qualify(Path::new(&format!("{i}.png")));
        }
        assert_eq!(history.entries.len(), 50);
        assert_eq!(history.entries[0], Target::File("10.png".into()));
        let tab = TabSet::default().open_new("58.png".into(), MediaKind::Image);
        let request = Request {
            tab,
            generation: 0,
            revision: history.revision,
            index: 48,
            target: history.entries[48].clone(),
        };
        history.commit(&request, Path::new("58.png"));
        history.qualify(Path::new("58.png"));
        assert_eq!(history.index(true), Some(49));
        history.qualify(Path::new("branch.png"));
        assert_eq!(history.index(true), None);
        assert_eq!(history.entries.len(), 50);
        assert_eq!(
            history.entries.last(),
            Some(&Target::File("branch.png".into()))
        );
    }
}
