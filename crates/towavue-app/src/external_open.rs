use super::*;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn reusable_external_tab(&self, path: &Path) -> Option<TabId> {
        let kind = MediaKind::from_path(path)?;
        self.tabs
            .tabs()
            .iter()
            .filter(|tab| {
                let matches = match &tab.target {
                    TabTarget::Media {
                        path: current,
                        kind: current_kind,
                    } => *current_kind == kind && current == path,
                    TabTarget::AudioFolder { folder, .. } => {
                        kind == MediaKind::Audio && Some(folder.as_path()) == path.parent()
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
            .min_by_key(|tab| (Some(tab.id) != self.tabs.active_id(), tab.id))
            .map(|tab| tab.id)
    }

    pub(super) fn reuse_external_tab(&mut self, path: &Path) -> bool {
        let Some(id) = self.reusable_external_tab(path) else {
            return false;
        };
        self.activate_tab(id);
        if self.path.as_deref() != Some(path) {
            // A clean audio folder changes tracks through the existing navigation route.
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
}
