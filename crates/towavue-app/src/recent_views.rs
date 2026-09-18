use super::*;
use std::collections::HashMap;

const DWELL: Duration = Duration::from_secs(2);

struct Visit {
    path: PathBuf,
    presented: bool,
    since: Option<Instant>,
    recorded: bool,
    observed: bool,
    forgotten: bool,
}

#[derive(Default)]
pub(super) struct Views {
    visits: HashMap<TabId, Visit>,
    pending: Vec<PathBuf>,
}

impl Views {
    fn visit(&mut self, id: TabId, path: &Path) -> &mut Visit {
        let visit = self.visits.entry(id).or_insert_with(|| Visit {
            path: path.to_owned(),
            presented: false,
            since: None,
            recorded: false,
            observed: false,
            forgotten: false,
        });
        if visit.path != path {
            *visit = Visit {
                path: path.to_owned(),
                presented: false,
                since: None,
                recorded: false,
                observed: false,
                forgotten: false,
            };
        }
        visit
    }

    pub fn forget(&mut self, path: Option<&Path>) {
        // Removing history suppresses passive re-addition, not later explicit intent.
        for visit in self
            .visits
            .values_mut()
            .filter(|visit| path.is_none_or(|path| visit.path == path))
        {
            visit.recorded = true;
            visit.forgotten = true;
            visit.since = None;
        }
        self.pending
            .retain(|pending| path.is_some_and(|path| pending != path));
    }

    pub fn take_pending(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.pending)
    }

    pub fn begin(&mut self, id: TabId, path: &Path) {
        self.visit(id, path);
    }

    pub fn qualify(&mut self, id: TabId, path: &Path) {
        let visit = self.visit(id, path);
        if !visit.recorded || visit.forgotten {
            visit.recorded = true;
            visit.forgotten = false;
            visit.since = None;
            self.pending.push(path.to_owned());
        }
    }

    fn observe(&mut self, id: TabId, path: &Path, visible: bool, playing: bool, now: Instant) {
        let visit = self.visit(id, path);
        visit.observed = true;
        if visit.recorded {
            return;
        }
        if !playing && !(visible && visit.presented) {
            visit.since = None;
            return;
        }
        let since = *visit.since.get_or_insert(now);
        if now.saturating_duration_since(since) >= DWELL {
            self.qualify(id, path);
        }
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.visits
            .values()
            .filter_map(|visit| visit.since.map(|since| since + DWELL))
            .min()
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn qualify_history(&mut self, id: TabId, path: &Path) {
        // A new explicit open also restores an item the owner removed from history.
        self.viewed_media.visits.remove(&id);
        self.viewed_media.qualify(id, path);
        self.flush_viewed_history();
    }

    pub(super) fn qualify_current_history(&mut self) {
        if let (Some(id), Some(path)) = (self.displayed_tab, self.path.as_deref())
            && self.tabs.active_id() == Some(id)
        {
            self.viewed_media.qualify(id, path);
            self.flush_viewed_history();
        }
    }

    fn flush_viewed_history(&mut self) {
        for path in self.viewed_media.pending.drain(..) {
            if let Some(recent) = &self.recent_files {
                recent.record(path);
            }
        }
    }

    pub(super) fn history_presented(&mut self, video_drawn: bool) {
        if let (Some(id), Some(path)) = (self.displayed_tab, self.path.as_deref())
            && self.tabs.active_id() == Some(id)
            && !self.image_loading
            && self.image_handoff.is_none()
            && self.image_error.is_none()
            && (video_drawn
                || self.media_kind == Some(MediaKind::Audio)
                || (self.image.is_some() && self.reading_source_visible()))
        {
            self.viewed_media.visit(id, path).presented = true;
        }
    }

    pub(super) fn observe_viewed_history(&mut self, now: Instant) {
        let visible = self.image_animation_visible()
            && !self.modal_input_blocked()
            && !self.palette_open
            && !self.grid_open
            && !self.filmstrip_open;
        let active = self.tabs.active_id();
        // Mark/sweep keeps retirement linear in the number of live visits/tabs.
        for visit in self.viewed_media.visits.values_mut() {
            visit.observed = false;
        }
        for tab in self.tabs.tabs() {
            if self.tabs.is_utility(tab.id) {
                continue;
            }
            let path = tab.target.current_path();
            let (shown, playing) = if Some(tab.id) == self.displayed_tab {
                let ready = self.path.as_deref() == Some(path)
                    && !matches!(self.state, PlaybackState::Loading | PlaybackState::Faulted)
                    && !self.image_loading
                    && self.image_error.is_none()
                    && self.playback_error.is_none();
                (
                    visible && active == Some(tab.id) && ready,
                    ready
                        && self.state == PlaybackState::Playing
                        && self.clock.is_some()
                        && matches!(self.media_kind, Some(MediaKind::Audio | MediaKind::Video)),
                )
            } else if let Some(saved) = self.retained_playback.get(&tab.id) {
                (
                    false,
                    saved.path == path
                        && !saved.prepared_only
                        && saved.error.is_none()
                        && saved.state == PlaybackState::Playing
                        && saved.clock.is_some(),
                )
            } else {
                (false, false)
            };
            self.viewed_media.observe(tab.id, path, shown, playing, now);
        }
        self.viewed_media.visits.retain(|_, visit| visit.observed);
        self.flush_viewed_history();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dwell_requires_current_presented_content_and_resets_across_interruptions() {
        let mut tabs = TabSet::default();
        let id = tabs.open_new("first.png".into(), MediaKind::Image);
        let mut views = Views::default();
        let now = Instant::now();
        let first = Path::new("first.png");
        views.observe(id, first, true, false, now);
        views.observe(id, first, true, false, now + DWELL * 20);
        assert!(views.pending.is_empty() && views.deadline().is_none());
        for index in 0..100 {
            let path = PathBuf::from(format!("{index}.png"));
            views.visit(id, &path).presented = true;
            views.observe(id, &path, true, false, now);
            views.observe(id, &path, true, false, now + Duration::from_millis(30));
        }
        assert!(
            views.pending.is_empty(),
            "transient presented images are not visits"
        );
        let last = Path::new("99.png");
        views.observe(id, last, false, false, now + DWELL);
        assert!(
            views.deadline().is_none(),
            "hidden/covered/paused background resets dwell"
        );
        views.observe(id, last, true, false, now + DWELL * 2);
        assert_eq!(views.deadline(), Some(now + DWELL * 3));
        views.observe(id, last, true, false, now + DWELL * 3);
        assert_eq!(views.pending, [last.to_owned()]);
        views.observe(id, last, true, false, now + DWELL * 4);
        assert_eq!(views.pending.len(), 1, "one record per qualified visit");
        assert!(views.deadline().is_none());
        let background = Path::new("background.wav");
        views.observe(id, background, false, true, now);
        views.observe(id, background, false, true, now + DWELL);
        assert_eq!(views.pending.last().map(PathBuf::as_path), Some(background));
    }

    fn ready(app: &mut Application<fn(AppEvent)>) {
        app.image_generation = app.image_loader.request(Vec::new());
        app.image_loading = false;
        app.image_error = None;
        app.image_handoff = None;
        app.image = Some(
            ImagePresentation::from_decoded(
                app.ui_context.as_ref().expect("context"),
                app.path.as_ref().expect("path"),
                tab_transfer::tests::decoded(false),
            )
            .expect("pixels"),
        );
        app.state = PlaybackState::Paused;
    }

    #[test]
    fn explicit_guarded_opens_dwell_zoom_and_edits_persist_only_qualified_files() {
        let Some(root) = crate::tests::isolated_test_root(
            "recent_views::tests::explicit_guarded_opens_dwell_zoom_and_edits_persist_only_qualified_files",
        ) else {
            return;
        };
        let paths: Vec<_> = (0..9)
            .map(|index| root.join(format!("image-{index}.bmp")))
            .collect();
        for path in &paths {
            tab_transfer::tests::bitmap(path);
        }
        let history = root.join("viewed.txt");
        let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
        app.ui_context = Some(fonts::test_context());
        app.recent_files = Some(
            towavue_runtime_windows::RecentFiles::new(history.clone(), || {}).expect("history"),
        );
        app.open_external(paths[0].clone(), false);
        let now = Instant::now();
        for (index, path) in paths.iter().enumerate().take(4).skip(1) {
            app.navigate_to_unchecked(path.clone());
            ready(&mut app);
            app.history_presented(false);
            app.observe_viewed_history(now + Duration::from_millis(index as u64 * 30));
        }
        app.observe_viewed_history(now + DWELL + Duration::from_millis(100));
        app.navigate_to_unchecked(paths[4].clone());
        ready(&mut app);
        app.observe_viewed_history(now + DWELL * 3);
        app.observe_viewed_history(now + DWELL * 4);
        assert!(
            !app.viewed_media.visits[&app.displayed_tab.expect("tab")].recorded,
            "decoded but never presented content does not qualify"
        );
        app.dispatch(CommandId::ActualSize);
        app.navigate_to_unchecked(paths[5].clone());
        ready(&mut app);
        app.push_edit(EditOperation::FlipHorizontal);
        app.handle_ui_action(UiAction::OpenMedia(paths[6].clone(), false));
        assert!(matches!(app.pending_guard, Some(GuardedAction::View(_))));
        app.resolve_guard(GuardDecision::Cancel);
        assert_eq!(app.path.as_ref(), Some(&paths[5]));
        app.handle_ui_action(UiAction::OpenMedia(paths[7].clone(), false));
        app.resolve_guard(GuardDecision::Discard);
        assert_eq!(app.path.as_ref(), Some(&paths[7]));
        let id = app.displayed_tab.expect("tab");
        app.tabs.close(id);
        app.observe_viewed_history(now + DWELL * 10);
        assert!(
            !app.viewed_media.visits.contains_key(&id),
            "closed owners retire timers"
        );
        drop(app.recent_files.take());
        let persisted = std::fs::read_to_string(history).expect("persisted visits");
        for (index, path) in paths.iter().enumerate() {
            assert_eq!(
                persisted.contains(path.to_str().expect("path")),
                matches!(index, 0 | 3 | 4 | 5 | 7),
                "file {index}"
            );
        }
    }

    #[test]
    fn existing_tab_open_and_new_interaction_restore_history_after_explicit_removal() {
        let Some(root) = crate::tests::isolated_test_root(
            "recent_views::tests::existing_tab_open_and_new_interaction_restore_history_after_explicit_removal",
        ) else {
            return;
        };
        let path = root.join("existing.bmp");
        tab_transfer::tests::bitmap(&path);
        let history = root.join("viewed.txt");
        let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
        app.ui_context = Some(fonts::test_context());
        app.recent_files = Some(
            towavue_runtime_windows::RecentFiles::new(history.clone(), || {}).expect("history"),
        );
        let id = tab_transfer::tests::install(
            &mut app,
            path.clone(),
            tab_transfer::tests::decoded(false),
        );
        app.handle_recent_action(menu::RecentAction::Open(
            path.clone(),
            towavue_runtime_windows::RecentKind::File,
            menu::OpenTarget::Tab,
        ));
        assert!(app.viewed_media.visits[&id].recorded);
        app.handle_recent_action(menu::RecentAction::Clear);
        let now = Instant::now();
        app.history_presented(false);
        app.observe_viewed_history(now);
        app.observe_viewed_history(now + DWELL * 10);
        assert!(
            app.viewed_media.visits[&id].forgotten,
            "passive dwell cannot undo Clear"
        );
        assert!(app.viewed_media.deadline().is_none());
        app.dispatch(CommandId::ActualSize);
        assert!(
            !app.viewed_media.visits[&id].forgotten,
            "new interaction qualifies again"
        );
        app.recent_paths = vec![path.clone()];
        app.handle_recent_action(menu::RecentAction::Remove(
            path.clone(),
            towavue_runtime_windows::RecentKind::File,
        ));
        assert!(app.viewed_media.visits[&id].forgotten);
        app.handle_recent_action(menu::RecentAction::Open(
            path.clone(),
            towavue_runtime_windows::RecentKind::File,
            menu::OpenTarget::Tab,
        ));
        assert!(!app.viewed_media.visits[&id].forgotten);
        drop(app.recent_files.take());
        let persisted = std::fs::read_to_string(history).expect("history");
        assert!(persisted.contains(path.to_str().expect("path")));
    }

    #[test]
    fn background_card_navigation_is_excluded_but_background_playback_qualifies() {
        let Some(root) = crate::tests::isolated_test_root(
            "recent_views::tests::background_card_navigation_is_excluded_but_background_playback_qualifies",
        ) else {
            return;
        };
        let paths: Vec<_> = (0..3)
            .map(|index| root.join(format!("card-{index}.bmp")))
            .collect();
        for path in &paths {
            tab_transfer::tests::bitmap(path);
        }
        let history = root.join("viewed.txt");
        let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
        app.ui_context = Some(fonts::test_context());
        app.recent_files = Some(
            towavue_runtime_windows::RecentFiles::new(history.clone(), || {}).expect("history"),
        );
        let background = tab_transfer::tests::install(
            &mut app,
            paths[0].clone(),
            tab_transfer::tests::decoded(false),
        );
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: paths
                .iter()
                .map(|path| towavue_core::FolderMediaItem {
                    identity: towavue_core::ShellIdentity::new(vec![]),
                    path: path.clone(),
                    kind: MediaKind::Image,
                })
                .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: std::time::SystemTime::UNIX_EPOCH,
        });
        let foreground = tab_transfer::tests::install(
            &mut app,
            paths[2].clone(),
            tab_transfer::tests::decoded(false),
        );
        let instance = app.retained_images[&background].instance;
        app.navigate_image_tab(background, instance, paths[0].clone(), paths[1].clone());
        assert_eq!(app.retained_images[&background].path, paths[1]);
        assert_eq!(app.displayed_tab, Some(foreground));
        let now = Instant::now();
        app.observe_viewed_history(now);
        app.observe_viewed_history(now + DWELL * 10);
        assert!(!app.viewed_media.visits[&background].recorded);
        for (index, kind) in [MediaKind::Audio, MediaKind::Video].into_iter().enumerate() {
            let path = root.join(if index == 0 {
                "background.wav"
            } else {
                "background.mp4"
            });
            std::fs::write(&path, b"owned metadata failure fixture").expect("fixture");
            let id = app.tabs.open_new(path.clone(), kind);
            app.tabs.activate(foreground);
            app.prepare_playback_tab(id);
            app.observe_viewed_history(now);
            app.observe_viewed_history(now + DWELL * 3);
            assert!(
                !app.viewed_media.visits[&id].recorded,
                "metadata-only tab does not qualify"
            );
            let saved = app.retained_playback.get_mut(&id).expect("prepared tab");
            saved.prepared_only = false;
            saved.state = PlaybackState::Playing;
            saved.clock = Some(PlaybackClock::new(MediaTime::ZERO, 1.0));
            app.observe_viewed_history(now + DWELL * 4);
            app.observe_viewed_history(now + DWELL * 5);
            assert!(
                app.viewed_media.visits[&id].recorded,
                "playing background qualifies without activation"
            );
        }
        drop(app.recent_files.take());
        let persisted = std::fs::read_to_string(history).expect("history");
        assert!(!persisted.contains("card-"));
        assert!(persisted.contains("background.wav") && persisted.contains("background.mp4"));
    }
}
