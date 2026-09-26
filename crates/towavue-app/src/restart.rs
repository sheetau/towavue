use crate::localization::Language;

// The event loop has already drained Shell/update workers. Dropping the host
// flushes preferences and releases remaining application ownership; dropping
// LaunchServer joins its receiver before releasing the single-instance marker.
// Only then may a replacement process participate in ordinary launch election.
pub(super) fn retire_then_restart<A, S>(
    application: A,
    launch_server: S,
    language: Option<Language>,
    launch: impl FnOnce(Language) -> std::io::Result<()>,
) -> std::io::Result<()> {
    drop(application);
    drop(launch_server);
    if let Some(language) = language {
        launch(language)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn exit_guards_show_complete_all_tab_discard_labels_at_supported_densities() {
        use crate::{Application, GuardedAction, MediaKind, localization};
        let Some(root) = crate::tests::isolated_test_root(
            "restart::tests::exit_guards_show_complete_all_tab_discard_labels_at_supported_densities",
        ) else {
            return;
        };
        for language in [Language::English, Language::Japanese] {
            for density in [1.0, 1.25, 2.0] {
                let context = crate::fonts::test_context();
                localization::test_ui::configure_japanese(&context, density);
                localization::set_language(&context, language);
                let mut app = Application::new(None, |_| {}).expect("app");
                app.language_settings.display = language;
                let path = root.join("source.png");
                let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
                app.path = Some(path);
                for action in [
                    GuardedAction::Exit,
                    GuardedAction::CoordinatedExit(1),
                    GuardedAction::CloseTab(tab),
                ] {
                    let all = !matches!(action, GuardedAction::CloseTab(_));
                    let expected = if all {
                        localization::Text::NativeDiscardAll
                    } else {
                        localization::Text::NativeDiscard
                    }
                    .in_language(language);
                    app.pending_guard = Some(action);
                    let mut output = egui::FullOutput::default();
                    let mut actions = Vec::new();
                    for _ in 0..4 {
                        output = context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(420.0, 300.0),
                                )),
                                ..Default::default()
                            },
                            |_| app.draw_unsaved_guard(&context, &mut actions),
                        );
                    }
                    assert_eq!(output.pixels_per_point, density);
                    assert!(output.shapes.iter().any(|shape| {
                        matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == expected
                            && !text.galley.elided
                            && shape.clip_rect.contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size())))
                    }), "complete discard scope: {expected} at {density}");
                    assert!(
                        actions.is_empty() && app.pending_guard.is_some() && !app.exit_requested
                    );
                }
            }
        }
    }

    #[test]
    fn restarted_process_becomes_primary_and_reads_the_saved_language() {
        use towavue_runtime_windows::{LanguagePreferences, LaunchRole, LaunchServer};
        const NAME: &str =
            "restart::tests::restarted_process_becomes_primary_and_reads_the_saved_language";
        let Some(root) = crate::tests::isolated_test_root(NAME) else {
            return;
        };
        let path = root.join("config/towavue/language.conf");
        let marker = root.join("restarted-process.txt");
        let LaunchRole::Primary(server) =
            LaunchServer::start_or_forward_target(None, false, |request| {
                request.acknowledge(false);
            })
            .expect("launch election")
        else {
            panic!("replacement must not forward to the retiring process");
        };
        if std::env::var_os("TOWAVUE_RESTART_HANDOFF_CHILD").is_some() {
            let store = LanguagePreferences::open(path, |_| {}).expect("reopen preference");
            assert_eq!(store.initial(), Language::Japanese);
            std::fs::write(marker, std::process::id().to_string()).expect("child completion");
            drop(server);
            return;
        }
        let (send, receive) = std::sync::mpsc::channel();
        let store = LanguagePreferences::open(path, move |result| {
            send.send(result).expect("saved response");
        })
        .expect("language writer");
        store.remember(Language::Japanese).expect("save choice");
        assert_eq!(
            receive
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("save completion")
                .expect("saved"),
            Language::Japanese
        );
        retire_then_restart(store, server, Some(Language::English), |_| {
            let output = crate::tests::hidden_command(std::env::current_exe()?)
                .args(["--exact", NAME, "--nocapture"])
                .env("TOWAVUE_RESTART_HANDOFF_CHILD", "1")
                .output()?;
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            Ok(())
        })
        .expect("relaunch handoff");
        let child: u32 = std::fs::read_to_string(marker)
            .expect("child ran")
            .parse()
            .expect("child pid");
        assert_ne!(
            child,
            std::process::id(),
            "a fresh process read the preference"
        );
    }

    #[test]
    fn relaunch_waits_for_both_owners_and_propagates_failure_without_retry() {
        struct Owner<'a>(&'a RefCell<Vec<&'static str>>, &'static str);
        impl Drop for Owner<'_> {
            fn drop(&mut self) {
                self.0.borrow_mut().push(self.1);
            }
        }
        for restart in [false, true] {
            let events = RefCell::new(Vec::new());
            let result = retire_then_restart(
                Owner(&events, "host"),
                Owner(&events, "launch server"),
                restart.then_some(Language::Japanese),
                |language| {
                    assert_eq!(language, Language::Japanese);
                    assert_eq!(*events.borrow(), ["host", "launch server"]);
                    events.borrow_mut().push("launch");
                    Err(std::io::Error::other("relaunch failure"))
                },
            );
            assert_eq!(result.is_err(), restart);
            assert_eq!(events.borrow().len(), if restart { 3 } else { 2 });
        }
    }
}
