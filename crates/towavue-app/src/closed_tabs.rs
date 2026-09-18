use crate::*;

pub(super) enum ClosedTab {
    Media(PathBuf),
    Gallery {
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
        match self.closed_tabs.pop_back() {
            Some(ClosedTab::Media(path)) => self.open_external(path, true),
            Some(ClosedTab::Gallery { search, filter }) => {
                // Open Gallery retains foreground media and reuses the window's
                // one Gallery, including an automatically created last-tab fallback.
                self.dispatch(CommandId::OpenGallery);
                self.gallery_search = search;
                self.gallery_filter = filter;
                self.request_redraw();
            }
            None => self.set_status("No closed tabs to reopen.".into()),
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
