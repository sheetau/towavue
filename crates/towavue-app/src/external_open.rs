use super::*;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn reuses_folder_tabs(&self) -> bool {
        self.view_preferences
            .as_ref()
            .map_or(self.reuse_folder_tab, |store| {
                store.value(towavue_runtime_windows::ViewPreference::ReuseFolderTab) != 0
            })
    }

    pub(super) fn reusable_external_tab(&self, path: &Path) -> Option<TabId> {
        let kind = MediaKind::from_path(path)?;
        let reuse_folder = self.reuses_folder_tabs();
        self.tabs
            .tabs()
            .iter()
            .filter(|tab| {
                let matches = match &tab.target {
                    TabTarget::Media {
                        path: current,
                        kind: current_kind,
                    } => {
                        (*current_kind == kind && current == path)
                            || (reuse_folder
                                && current.parent().is_some()
                                && current.parent() == path.parent())
                    }
                    TabTarget::AudioFolder { folder, .. } => {
                        (kind == MediaKind::Audio || reuse_folder)
                            && Some(folder.as_path()) == path.parent()
                    }
                    TabTarget::UntitledImage => false,
                };
                matches
                    && !self.edits.get(&tab.id).is_some_and(EditHistory::is_dirty)
                    && !self.deleted_sources.contains_key(&tab.id)
                    && !self
                        .active_export
                        .as_ref()
                        .is_some_and(|export| export.tab == tab.id)
            })
            .min_by_key(|tab| {
                (
                    tab.target.current_path() != Some(path),
                    Some(tab.id) != self.tabs.active_id(),
                    tab.id,
                )
            })
            .map(|tab| tab.id)
    }

    pub(super) fn reuse_external_tab(&mut self, path: &Path) -> bool {
        let Some(id) = self.reusable_external_tab(path) else {
            return false;
        };
        self.activate_tab(id);
        if self.path.as_deref() != Some(path) {
            // Change the source through the existing mixed-media navigation route.
            self.navigate_to_unchecked(path.to_owned());
        }
        // Reusing an existing tab is activation, not a new explicit history visit.
        self.request_redraw();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_reuse_preserves_clean_view_and_excludes_dirty_and_deleted_documents() {
        let Some(root) = crate::tests::isolated_test_root(
            "external_open::tests::external_reuse_preserves_clean_view_and_excludes_dirty_and_deleted_documents",
        ) else {
            return;
        };
        let path = root.join("existing.bmp");
        tab_transfer::tests::bitmap(&path);
        let mut app = Application::new(None, |_| {}).expect("app");
        app.ui_context = Some(fonts::test_context());
        let id = tab_transfer::tests::install(
            &mut app,
            path.clone(),
            tab_transfer::tests::decoded(false),
        );
        app.image_view.zoom = ZoomMode::Custom(2.0);
        app.image_view.pan = (12.0, -5.0);
        let generation = app.media_generation;
        assert!(app.reuse_external_tab(&path));
        assert_eq!(app.tabs.active_id(), Some(id));
        assert_eq!(app.media_generation, generation);
        assert_eq!(app.image_view.zoom, ZoomMode::Custom(2.0));
        assert_eq!(app.image_view.pan, (12.0, -5.0));
        app.push_edit(EditOperation::FlipHorizontal);
        assert!(!app.reuse_external_tab(&path));
        app.open_external(path.clone(), true);
        let fresh = app.tabs.active_id().expect("new clean tab");
        assert_ne!(fresh, id);
        assert!(app.edits[&id].is_dirty());
        assert_eq!(app.reusable_external_tab(&path), Some(fresh));
    }

    #[test]
    fn external_audio_reuses_clean_folder_but_not_a_dirty_playlist() {
        let Some(root) = crate::tests::isolated_test_root(
            "external_open::tests::external_audio_reuses_clean_folder_but_not_a_dirty_playlist",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("app");
        let first = root.join("a.wav");
        let next = root.join("b.wav");
        let id = app.tabs.open_new(first, MediaKind::Audio);
        assert_eq!(app.reusable_external_tab(&next), Some(id));
        assert!(
            app.reusable_external_tab(&root.join("another/a.wav"))
                .is_none()
        );
        app.edits
            .entry(id)
            .or_default()
            .push(EditOperation::SetVolume(0.5), MediaKind::Audio);
        assert!(app.reusable_external_tab(&next).is_none());
    }
    #[test]
    fn folder_reuse_is_opt_in_cross_kind_and_preserves_dirty_documents() {
        let Some(root) = crate::tests::isolated_test_root(
            "external_open::tests::folder_reuse_is_opt_in_cross_kind_and_preserves_dirty_documents",
        ) else {
            return;
        };
        let first = root.join("first.bmp");
        let next = root.join("next.bmp");
        tab_transfer::tests::bitmap(&first);
        tab_transfer::tests::bitmap(&next);
        let mut app = Application::new(None, |_| {}).expect("app");
        app.ui_context = Some(fonts::test_context());
        let id = tab_transfer::tests::install(
            &mut app,
            first.clone(),
            tab_transfer::tests::decoded(false),
        );
        assert!(!app.reuses_folder_tabs());
        assert!(app.reusable_external_tab(&next).is_none());
        app.dispatch(CommandId::ToggleReuseFolderTab);
        assert_eq!(app.reusable_external_tab(&next), Some(id));
        assert_eq!(app.reusable_external_tab(&root.join("video.mp4")), Some(id));
        assert_eq!(app.reusable_external_tab(&root.join("audio.wav")), Some(id));
        assert!(
            app.reusable_external_tab(&root.join("other/next.bmp"))
                .is_none()
        );
        let exact = app.tabs.open_new(next.clone(), MediaKind::Image);
        app.tabs.activate(id);
        assert_eq!(app.reusable_external_tab(&next), Some(exact));
        app.tabs.close(exact);
        assert!(app.reuse_external_tab(&next));
        assert_eq!(app.tabs.active_id(), Some(id));
        assert_eq!(app.tabs.tabs().len(), 1);
        assert_eq!(app.path.as_deref(), Some(next.as_path()));
        app.edits
            .entry(id)
            .or_default()
            .push(EditOperation::FlipHorizontal, MediaKind::Image);
        assert!(app.reusable_external_tab(&first).is_none());
        app.open_external(first, true);
        assert_ne!(app.tabs.active_id(), Some(id));
        assert!(app.edits[&id].is_dirty());
        app.dispatch(CommandId::ToggleReuseFolderTab);
        assert!(!app.reuses_folder_tabs());
    }
}
