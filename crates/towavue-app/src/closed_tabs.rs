use crate::*;

pub(super) enum ClosedTab {
    Media(PathBuf, usize),
    KeyboardSettings(keyboard_settings::KeyboardSettings, usize),
    Gallery {
        position: usize,
        search: String,
        filter: Option<MediaKind>,
    },
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn remember_closed_tab(&mut self, tab: ClosedTab) {
        if self.closed_tabs.len() == 32 {
            self.closed_tabs.pop_front();
        }
        self.closed_tabs.push_back(tab);
    }

    pub(super) fn reopen_closed_tab(&mut self) {
        if self.modal_input_blocked() {
            return;
        }
        let restored = match self.closed_tabs.pop_back() {
            Some(ClosedTab::KeyboardSettings(state, position)) => {
                self.dispatch(CommandId::OpenKeyboardSettings);
                self.keyboard_settings = state;
                self.tabs.keyboard_settings().map(|id| (id, position))
            }
            Some(ClosedTab::Media(path, position)) => {
                let previous = self.tabs.active_id();
                self.open_external(path, true);
                self.tabs
                    .active_id()
                    .filter(|id| Some(*id) != previous)
                    .map(|id| (id, position))
            }
            Some(ClosedTab::Gallery {
                search,
                filter,
                position,
            }) => {
                // Reuse the window's singleton, including an automatic last-tab fallback.
                self.dispatch(CommandId::OpenGallery);
                self.gallery_search = search;
                self.gallery_filter = filter;
                self.tabs.gallery().map(|id| (id, position))
            }
            None => {
                self.set_status("No closed tabs to reopen.".into());
                None
            }
        };
        if let Some((id, position)) = restored {
            let from = self
                .tabs
                .tab_ids()
                .position(|tab| tab == id)
                .expect("restored tab");
            let to = position.min(self.tabs.len().saturating_sub(1));
            // reorder accepts a gap in the pre-removal order, not a final index.
            self.tabs.reorder(id, to + usize::from(from < to));
            self.request_redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_and_media_reopen_in_close_order_with_query_and_guard_preserved() {
        let Some(root) = crate::tests::isolated_test_root(
            "closed_tabs::tests::gallery_and_media_reopen_in_close_order_with_query_and_guard_preserved",
        ) else {
            return;
        };
        let path = root.join("media.wav");
        std::fs::write(&path, b"owned undecodable media fixture").expect("fixture");
        let mut app = Application::new(None, |_| {}).expect("app");
        let gallery = app.tabs.gallery().expect("Gallery");
        app.dispatch(CommandId::CloseTab);
        assert!(
            app.closed_tabs.is_empty(),
            "protected last tab is not a close"
        );
        let keep = app.tabs.open_new(path.clone(), MediaKind::Audio);
        app.edits
            .entry(keep)
            .or_default()
            .push(EditOperation::SetVolume(0.5), MediaKind::Audio);
        let history = app.edits[&keep].clone();
        let media = app.tabs.open_new(path.clone(), MediaKind::Audio);
        app.gallery_search = "saved query".into();
        app.gallery_filter = Some(MediaKind::Image);
        app.dispatch_tab_command(gallery, CommandId::CloseTab);
        assert_eq!(app.tabs.active_id(), Some(media));
        assert!(app.tabs.gallery().is_none());
        assert_eq!(app.closed_tabs.len(), 1);
        app.dispatch(CommandId::CloseTab);
        assert_eq!(app.closed_tabs.len(), 2);
        app.dispatch(CommandId::ReopenClosedTab);
        let reopened_media = app.tabs.active_id().expect("reopened media");
        assert_ne!(reopened_media, media);
        assert_eq!(app.path.as_ref(), Some(&path));
        assert!(
            app.tabs.gallery().is_none(),
            "most recently closed media reopens first"
        );
        app.pending_guard = Some(GuardedAction::CloseTab(keep));
        app.reopen_closed_tab();
        assert_eq!(
            app.closed_tabs.len(),
            1,
            "modal must not consume Gallery history"
        );
        app.resolve_guard(GuardDecision::Cancel);
        app.dispatch(CommandId::ReopenClosedTab);
        let reopened_gallery = app.tabs.gallery().expect("reopened Gallery");
        assert_ne!(reopened_gallery, gallery);
        assert_eq!(app.tabs.active_id(), Some(reopened_gallery));
        assert_eq!(app.gallery_search, "saved query");
        assert_eq!(app.gallery_filter, Some(MediaKind::Image));
        assert_eq!(
            app.edits[&keep], history,
            "Gallery restoration retains dirty media edits"
        );
        assert!(app.path.is_none() && app.pending_guard.is_none());
        assert!(app.closed_tabs.is_empty());
        app.dispatch(CommandId::CloseTab);
        app.dispatch(CommandId::OpenGallery);
        let existing = app.tabs.gallery().expect("new Gallery");
        app.gallery_search = "new query".into();
        app.dispatch(CommandId::ReopenClosedTab);
        assert_eq!(app.tabs.gallery(), Some(existing), "one Gallery per window");
        assert_eq!(app.gallery_search, "saved query");
        assert_eq!(app.gallery_filter, Some(MediaKind::Image));
        assert_eq!(app.tabs.len(), 3);
        app.remove_tab(existing, false);
        assert!(
            app.closed_tabs.is_empty(),
            "transfer removal is not a user close"
        );
    }

    #[test]
    fn reopening_restores_positions_for_media_and_utilities_and_clamps_shorter_strips() {
        let Some(root) = crate::tests::isolated_test_root(
            "closed_tabs::tests::reopening_restores_positions_for_media_and_utilities_and_clamps_shorter_strips",
        ) else {
            return;
        };
        let path = root.join("source.wav");
        std::fs::write(&path, b"owned undecodable fixture").expect("fixture");
        for position in 0..4 {
            let mut app = Application::new(None, |_| {}).expect("app");
            let gallery = app.tabs.gallery().expect("Gallery");
            let image = app.tabs.open_new(path.clone(), MediaKind::Audio);
            let settings = app.tabs.open_keyboard_settings();
            let last = app.tabs.open_new(path.clone(), MediaKind::Audio);
            let mut expected = vec![gallery, image, settings, last];
            app.remove_tab(expected[position], true);
            app.reopen_closed_tab();
            let restored = app.tabs.active_id().expect("restored tab");
            assert_ne!(restored, expected[position]);
            expected[position] = restored;
            assert_eq!(app.tabs.tab_ids().collect::<Vec<_>>(), expected);
        }
        let mut app = Application::new(None, |_| {}).expect("app");
        let gallery = app.tabs.gallery().expect("Gallery");
        let first = app.tabs.open_new(path.clone(), MediaKind::Audio);
        let second = app.tabs.open_new(path.clone(), MediaKind::Audio);
        let last = app.tabs.open_new(path.clone(), MediaKind::Audio);
        app.remove_tab(last, true);
        app.remove_tab(first, false);
        app.remove_tab(second, false);
        app.reopen_closed_tab();
        let restored = app.tabs.active_id().expect("restored media");
        assert_eq!(app.tabs.tab_ids().collect::<Vec<_>>(), [gallery, restored]);
        // A utility opened since the close is reused, then moved in either direction.
        app.tabs.reorder(gallery, app.tabs.len());
        app.remove_tab(gallery, true);
        app.dispatch(CommandId::OpenGallery);
        let existing = app.tabs.gallery().expect("existing Gallery");
        app.tabs.reorder(existing, 0);
        app.reopen_closed_tab();
        assert_eq!(app.tabs.tab_ids().collect::<Vec<_>>(), [restored, existing]);
        let order = app.tabs.tab_ids().collect::<Vec<_>>();
        app.remember_closed_tab(ClosedTab::Media(root.join("missing.wav"), 0));
        app.reopen_closed_tab();
        assert_eq!(
            app.tabs.tab_ids().collect::<Vec<_>>(),
            order,
            "failed opens cannot move an unrelated active tab"
        );
    }

    #[test]
    fn gallery_closes_share_the_bounded_media_history() {
        let Some(_) = crate::tests::isolated_test_root(
            "closed_tabs::tests::gallery_closes_share_the_bounded_media_history",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("app");
        app.tabs.open_new("unopened.wav".into(), MediaKind::Audio);
        for index in 0..35 {
            app.dispatch(CommandId::OpenGallery);
            app.gallery_search = index.to_string();
            app.dispatch(CommandId::CloseTab);
        }
        assert_eq!(app.closed_tabs.len(), 32);
        assert!(
            matches!(app.closed_tabs.front(), Some(ClosedTab::Gallery { search, .. }) if search == "3")
        );
        for index in (3..35).rev() {
            app.dispatch(CommandId::ReopenClosedTab);
            assert_eq!(app.gallery_search, index.to_string());
            assert_eq!(app.tabs.len(), 2);
        }
        assert!(app.closed_tabs.is_empty());
    }
}
