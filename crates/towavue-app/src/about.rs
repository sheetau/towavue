use crate::{AppEvent, Application};
use towavue_runtime_windows::{AboutEvent, AboutResponse, show_about};

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn open_about(&mut self) {
        if self.modal_input_blocked() {
            return;
        }
        #[cfg(test)]
        if self
            .window
            .as_ref()
            .is_none_or(|window| window.is_visible() == Some(false))
        {
            // Hidden/headless controls inject events at the native callback boundary.
            self.about_open = true;
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        let notify = std::sync::Arc::clone(&self.notify);
        match show_about(
            self.language(),
            window,
            env!("CARGO_PKG_VERSION"),
            env!("CARGO_PKG_LICENSE"),
            move |event| notify(AppEvent::About(event)),
        ) {
            Ok(()) => self.about_open = true,
            Err(error) => self.set_status(towavue_core::localization::formatted::about_failed(
                self.language(),
                &error.message(self.language()),
            )),
        }
        self.request_redraw();
    }

    pub(super) fn handle_about(&mut self, event: AboutEvent) {
        if !self.about_open {
            return;
        }
        match event {
            AboutEvent::Link(link) => {
                if let Err(error) = link.open() {
                    self.set_status(towavue_core::localization::formatted::link_failed(
                        self.language(),
                        &error.to_string(),
                    ));
                }
            }
            AboutEvent::Closed(result) => {
                self.about_open = false;
                self.refresh_pointer_position();
                match result {
                    Ok(AboutResponse::Licenses) => self.show_licenses(),
                    Ok(AboutResponse::Close) => {}
                    Err(error) => {
                        self.set_status(towavue_core::localization::formatted::about_failed(
                            self.language(),
                            &error.message(self.language()),
                        ))
                    }
                }
                // A save can finish on the worker while About owns the window.
                // Re-evaluate its existing continuation after releasing the dialog.
                if self.active_export.is_none()
                    && self.pending_dialog.is_none()
                    && self.native_prompt.is_none()
                    && self.export_error.is_none()
                    && let Some(action) = self.pending_guard.take()
                {
                    self.request_guarded(action);
                }
            }
        }
        self.request_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GuardedAction;
    use towavue_core::{CommandId, MediaKind};
    use towavue_runtime_windows::{DialogError, FileDialogKind};

    #[test]
    fn native_about_blocks_leaving_and_releases_owner_on_close_or_failure() {
        let Some(root) = crate::tests::isolated_test_root(
            "about::tests::native_about_blocks_leaving_and_releases_owner_on_close_or_failure",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("app");
        let tab = app
            .tabs
            .open_new(root.join("retained.png"), MediaKind::Image);
        for fail in [false, true] {
            app.dispatch(CommandId::About);
            assert!(app.about_open && app.modal_input_blocked());
            app.dispatch(CommandId::OpenGallery);
            assert_eq!(app.tabs.active_id(), Some(tab));
            app.request_guarded(GuardedAction::Exit);
            assert!(!app.exit_requested);
            assert!(!app.begin_dialog(FileDialogKind::OpenFile, crate::DialogIntent::OpenFile));
            app.open_native_prompt(crate::FallbackPrompt::ExportBusy);
            assert!(app.native_prompt.is_none());
            app.handle_app_event(AppEvent::About(AboutEvent::Closed(if fail {
                Err(DialogError::ThreadStopped)
            } else {
                Ok(AboutResponse::Close)
            })));
            assert!(!app.about_open && !app.modal_input_blocked());
            assert_eq!(app.tabs.active_id(), Some(tab));
            assert!(!app.exit_requested);
            if fail {
                assert!(
                    app.status_message
                        .as_ref()
                        .expect("failure")
                        .0
                        .contains("Could not show About")
                );
            }
        }
        // A stale callback cannot launch the license guide or acquire a new modal.
        app.handle_app_event(AppEvent::About(AboutEvent::Closed(Ok(
            AboutResponse::Licenses,
        ))));
        assert!(!app.license_guide_pending);
    }

    #[test]
    fn native_about_defers_background_errors_and_save_continuations() {
        let Some(_root) = crate::tests::isolated_test_root(
            "about::tests::native_about_defers_background_errors_and_save_continuations",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("app");
        let context = crate::fonts::test_context();
        context.global_style_mut(crate::chrome::style);
        app.dispatch(CommandId::About);
        // Inject the same pending state produced by an export failure/completion.
        // Native About owns the window until its completion is consumed.
        app.export_error = Some("fixture export failure".into());
        app.pending_guard = Some(GuardedAction::Exit);
        for _ in 0..2 {
            let _ = context.run_ui(egui::RawInput::default(), |ui| {
                app.draw_ui(ui, &mut Vec::new());
            });
            assert!(context.memory(|memory| memory.top_modal_layer().is_none()));
        }
        app.handle_app_event(AppEvent::About(AboutEvent::Closed(Ok(
            AboutResponse::Close,
        ))));
        assert!(!app.exit_requested);
        assert!(app.pending_guard.is_some() && app.export_error.is_some());
        for _ in 0..2 {
            let _ = context.run_ui(egui::RawInput::default(), |ui| {
                app.draw_ui(ui, &mut Vec::new());
            });
        }
        assert!(context.memory(|memory| memory.top_modal_layer().is_some()));
        app.export_error = None;
        app.pending_guard = None;
        app.dispatch(CommandId::About);
        app.pending_guard = Some(GuardedAction::Exit);
        app.handle_app_event(AppEvent::About(AboutEvent::Closed(Ok(
            AboutResponse::Close,
        ))));
        assert!(app.exit_requested && app.pending_guard.is_none());
    }
}
