use towavue_core::{CommandId, ShortcutBindings, TabId, TabSet, command_definitions};

use CommandId::*;

#[cfg(test)]
pub(crate) mod keyboard_tests;

pub(crate) fn context_key_event(
    key: &winit::keyboard::Key,
    modifiers: winit::keyboard::ModifiersState,
    pressed: bool,
    repeat: bool,
) -> Option<egui::Event> {
    (key == &winit::keyboard::Key::Named(winit::keyboard::NamedKey::ContextMenu)
        && (!pressed || (modifiers.is_empty() && !repeat)))
        .then_some(egui::Event::Key {
            key: egui::Key::F10,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::SHIFT,
        })
}

pub(crate) fn popup(
    ui: &egui::Ui,
    response: &egui::Response,
    close: &egui::Response,
    contents: impl FnOnce(&mut egui::Ui) -> Option<CommandId>,
) -> Option<CommandId> {
    let context = ui.ctx();
    let popup_id = egui::Popup::default_response_id(response);
    let was_open = egui::Popup::is_id_open(context, popup_id);
    let eligible = response.enabled()
        && context.dragged_id().is_none()
        && (!egui::Popup::is_any_open(context) || was_open)
        && ui.input(|input| {
            input.focused
                && !input.pointer.any_down()
                && !input.events.contains(&egui::Event::WindowFocused(false))
        });
    for widget in [response, close] {
        context.accesskit_node_builder(widget.id, |node| {
            node.add_action(egui::accesskit::Action::ShowContextMenu);
        });
    }
    let origin = [response, close].into_iter().find(|widget| {
        let focused = widget.has_focus();
        eligible
            && ui.input(|input| {
                input.has_accesskit_action_request(
                    widget.id,
                    egui::accesskit::Action::ShowContextMenu,
                ) || (focused
                    && input.events.iter().any(|event| {
                        matches!(event, egui::Event::Key {
                            key: egui::Key::F10, pressed: true, repeat: false, modifiers, ..
                        } if *modifiers == egui::Modifiers::SHIFT)
                    }))
            })
    });
    let anchor_id = popup_id.with("keyboard-anchor");
    if let Some(origin) = origin {
        ui.input_mut(|input| {
            input.consume_key(egui::Modifiers::SHIFT, egui::Key::F10);
            input.consume_accesskit_action_requests(origin.id, |request| {
                request.action == egui::accesskit::Action::ShowContextMenu
            });
        });
        context.data_mut(|data| data.insert_temp(anchor_id, origin.id));
    } else if response.secondary_clicked() {
        context.data_mut(|data| data.remove::<egui::Id>(anchor_id));
    }
    let keyboard_origin = context.data(|data| data.get_temp::<egui::Id>(anchor_id));
    let mut popup = egui::Popup::context_menu(response);
    if origin.is_some() {
        popup = popup.open_memory(egui::SetOpenCommand::Bool(true));
    }
    if keyboard_origin.is_some() {
        popup = popup.at_position(
            response
                .rect
                .union(close.rect)
                .intersect(ui.clip_rect())
                .left_bottom(),
        );
    }
    let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
    let chosen = popup.show(contents).and_then(|inner| inner.inner);
    if (was_open || origin.is_some()) && !egui::Popup::is_id_open(context, popup_id) {
        if (escape || chosen.is_some()) && !egui::Popup::is_any_open(context) {
            context
                .memory_mut(|memory| memory.request_focus(keyboard_origin.unwrap_or(response.id)));
        }
        context.data_mut(|data| data.remove::<egui::Id>(anchor_id));
    }
    chosen
}

pub fn close_targets(tabs: &TabSet, target: TabId, command: CommandId) -> Vec<TabId> {
    let Some(index) = tabs.tab_ids().position(|tab| tab == target) else {
        return Vec::new();
    };
    tabs.tab_ids()
        .enumerate()
        .filter(|(position, tab)| match command {
            CloseTab => *tab == target && tabs.can_close(*tab),
            CloseOtherTabs => *tab != target,
            CloseTabsLeft => *position < index,
            CloseTabsRight => *position > index,
            CloseAllTabs => true,
            _ => false,
        })
        .map(|(_, tab)| tab)
        .collect()
}

pub fn show(
    ui: &mut egui::Ui,
    tabs: &TabSet,
    target: TabId,
    can_reopen: bool,
    shortcuts: &ShortcutBindings,
    muted: Option<bool>,
) -> Option<CommandId> {
    crate::chrome::flat_buttons(ui);
    let keyboard = crate::menu::MenuKeyboard::begin(ui);
    let mut items = Vec::new();
    let mut chosen = None;
    for group in [
        &[
            CloseTab,
            CloseOtherTabs,
            CloseAllTabs,
            CloseTabsLeft,
            CloseTabsRight,
        ][..],
        &[ReopenClosedTab],
        &[ToggleMute],
        &[CopyFilePath, RevealFile],
    ] {
        if !items.is_empty() {
            ui.separator();
        }
        for command in group {
            let definition = command_definitions()
                .iter()
                .find(|entry| entry.id == *command)
                .expect("registered tab command");
            let enabled = match command {
                ReopenClosedTab => can_reopen,
                ToggleMute => muted.is_some(),
                CopyFilePath | RevealFile => tabs.tabs().iter().any(|tab| tab.id == target),
                _ => !close_targets(tabs, target, *command).is_empty(),
            };
            let title = if *command == ToggleMute {
                if muted == Some(true) {
                    "Unmute tab"
                } else {
                    "Mute tab"
                }
            } else {
                definition.title
            };
            let response = ui.add_enabled(
                enabled,
                egui::Button::new(title).shortcut_text(crate::menu::shortcut_text(
                    ui,
                    shortcuts.label(
                        *command,
                        towavue_core::CommandContext {
                            media_kind: tabs
                                .tabs()
                                .iter()
                                .find(|tab| tab.id == target)
                                .map(|tab| tab.target.media_kind()),
                            ..Default::default()
                        },
                    ),
                    enabled,
                )),
            );
            if enabled {
                items.push(response.id);
            }
            if response.clicked() {
                chosen = Some(*command);
                ui.close();
            }
        }
    }
    keyboard.finish(ui, items);
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Application, GuardDecision, GuardedAction};
    use towavue_core::{EditOperation, MediaKind};

    #[test]
    fn close_groups_follow_target_identity_and_current_order() {
        let mut tabs = TabSet::default();
        let a = tabs.open_new("a.png".into(), MediaKind::Image);
        let b = tabs.open_new("b.png".into(), MediaKind::Image);
        let c = tabs.open_new("c.png".into(), MediaKind::Image);
        tabs.close_gallery(tabs.gallery().expect("media-only fixture"));
        assert_eq!(close_targets(&tabs, b, CloseTab), [b]);
        assert_eq!(close_targets(&tabs, b, CloseOtherTabs), [a, c]);
        assert_eq!(close_targets(&tabs, b, CloseTabsLeft), [a]);
        assert_eq!(close_targets(&tabs, b, CloseTabsRight), [c]);
        assert_eq!(close_targets(&tabs, b, CloseAllTabs), [a, b, c]);
        assert!(close_targets(&tabs, a, CloseTabsLeft).is_empty());
        tabs.reorder(a, 3);
        assert_eq!(close_targets(&tabs, b, CloseTabsRight), [c, a]);
        tabs.close(b);
        assert!(close_targets(&tabs, b, CloseAllTabs).is_empty());
    }

    #[test]
    fn group_save_continues_after_each_source_save_and_preserves_originals() {
        let Some(root) = crate::tests::isolated_test_root(
            "tab_menu::tests::group_save_continues_after_each_source_save_and_preserves_originals",
        ) else {
            return;
        };
        let (sent, events) = std::sync::mpsc::channel();
        let mut app = Application::new(None, move |event| {
            let _ = sent.send(event);
        })
        .expect("app");
        let mut tabs = Vec::new();
        let mut originals = Vec::new();
        for index in 0..2 {
            let path = root.join(format!("{index}.bmp"));
            crate::tab_transfer::tests::bitmap(&path);
            originals.push(std::fs::read(&path).expect("original"));
            let id = app.tabs.open_new(path.clone(), MediaKind::Image);
            crate::source_save::tests::loaded(&mut app, id, &path);
            app.edits
                .entry(id)
                .or_default()
                .push(EditOperation::RotateClockwise, MediaKind::Image);
            app.export_paths
                .insert(id, root.join(format!("{index}.png")));
            tabs.push(id);
        }
        app.dispatch_tab_command(tabs[1], CloseAllTabs);
        for (index, expected) in tabs.iter().enumerate() {
            assert_eq!(app.tabs.active().expect("dirty tab").id, *expected);
            // This headless fixture already owns the document identity; pixel
            // upload is independent of the group-close continuation control.
            app.image_loader.clear();
            app.image_loading = false;
            app.state = crate::PlaybackState::Paused;
            app.resolve_guard(GuardDecision::Save);
            assert!(app.active_export.is_some());
            app.request_guarded(GuardedAction::CloseTabs(tabs.clone()));
            assert_eq!(
                app.tabs.tabs().len(),
                2,
                "save blocks the whole close request"
            );
            crate::source_save::tests::finish(&mut app, &events);
            assert!(app.export_error.is_none(), "{:?}", app.export_error);
            let source = root.join(format!("{index}.bmp"));
            assert_ne!(
                std::fs::read(&source).expect("saved source"),
                originals[index]
            );
            if app.source_backings.contains_key(expected) {
                assert_eq!(
                    std::fs::read(app.media_input_for(Some(*expected), &source).path())
                        .expect("retained original"),
                    originals[index]
                );
            }
            assert!(!root.join(format!("{index}.png")).exists());
        }
        assert!(app.tabs.tabs().is_empty() && app.pending_guard.is_none());
        assert_eq!(app.closed_tabs.len(), 3, "includes the closed Gallery");
    }

    #[test]
    fn group_close_confirms_each_dirty_tab_and_cancel_stops_the_batch() {
        let mut app = Application::new(None, |_| {}).expect("app");
        let a = app.tabs.open_new("a.png".into(), MediaKind::Image);
        app.media_kind = Some(MediaKind::Image);
        app.push_edit(EditOperation::RotateClockwise);
        let b = app.tabs.open_new("b.png".into(), MediaKind::Image);
        app.push_edit(EditOperation::FlipHorizontal);
        let c = app.tabs.open_new("c.png".into(), MediaKind::Image);
        app.dispatch_tab_command(c, CloseAllTabs);
        assert_eq!(app.tabs.active().expect("first dirty").id, a);
        assert_eq!(app.tabs.tabs().len(), 3);
        app.resolve_guard(GuardDecision::Cancel);
        assert_eq!(app.tabs.tabs().len(), 3);
        assert!(app.closed_tabs.is_empty());
        app.dispatch_tab_command(c, CloseAllTabs);
        app.resolve_guard(GuardDecision::Discard);
        assert_eq!(app.tabs.active().expect("second dirty").id, b);
        assert_eq!(app.tabs.tabs().len(), 2);
        assert!(app.pending_guard.is_some());
        app.resolve_guard(GuardDecision::Cancel);
        assert!(app.edits[&b].is_dirty());
        assert_eq!(app.closed_tabs.len(), 1);
        app.dispatch_tab_command(c, CloseOtherTabs);
        assert!(matches!(
            app.pending_guard,
            Some(GuardedAction::CloseTabs(_))
        ));
        // Successful export marks only this source clean before resuming the same guard.
        let action = app.pending_guard.take().expect("guard");
        let operations = app.edits[&b].operations().to_vec();
        app.edits
            .get_mut(&b)
            .expect("history")
            .mark_exported(&operations);
        app.request_guarded(action);
        assert_eq!(app.tabs.tabs().len(), 1);
        assert_eq!(app.tabs.active().expect("kept target").id, c);
        assert!(app.pending_guard.is_none());
        app.dispatch_tab_command(c, CloseAllTabs);
        assert!(app.tabs.tabs().is_empty() && app.path.is_none());
        assert_eq!(app.closed_tabs.len(), 4, "includes the closed Gallery");
    }

    #[test]
    fn closed_media_history_is_bounded_path_only_and_reopen_forces_a_new_tab() {
        let root = std::env::temp_dir().join(format!("towavue-reopen-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("fixture directory");
        let path = root.join("source.wav");
        // Opening the tab identity does not require a successfully decoded waveform.
        std::fs::write(&path, b"owned invalid audio fixture").expect("fixture");
        let mut app = Application::new(None, |_| {}).expect("app");
        for _ in 0..34 {
            let id = app.tabs.open_new(path.clone(), MediaKind::Audio);
            app.close_tab_unchecked(id);
        }
        assert_eq!(app.closed_tabs.len(), 32);
        let keep = app.tabs.open_new(path.clone(), MediaKind::Audio);
        app.media_kind = Some(MediaKind::Audio);
        app.push_edit(EditOperation::SetVolume(0.5));
        app.dispatch(ReopenClosedTab);
        assert_eq!(
            app.tabs.tabs().len(),
            2,
            "same-folder audio must not replace its sibling"
        );
        assert_eq!(app.closed_tabs.len(), 31);
        assert!(app.edits[&keep].is_dirty());
        let reopened = app.tabs.active().expect("reopened").id;
        assert_ne!(reopened, keep);
        assert!(!app.edits[&reopened].is_dirty());
        assert!(!app.export_paths.contains_key(&reopened));
        app.remove_tab(reopened, false);
        assert_eq!(
            app.closed_tabs.len(),
            31,
            "detach removal is not a user close"
        );
        app.pending_guard = Some(GuardedAction::CloseTab(keep));
        app.reopen_closed_tab();
        assert_eq!(app.closed_tabs.len(), 31, "modal must not consume history");
        app.pending_guard = None;
        app.closed_tabs
            .push_back(crate::closed_tabs::ClosedTab::Media(
                root.join("missing.png"),
                0,
            ));
        app.reopen_closed_tab();
        assert_eq!(app.tabs.tabs().len(), 1);
        assert!(app.status_message.as_ref().is_some());
        drop(app);
        std::fs::remove_file(path).expect("remove fixture");
        std::fs::remove_dir(root).expect("remove owned directory");
    }

    #[test]
    fn copying_an_inactive_tab_uses_its_path_without_changing_edits() {
        let mut app = Application::new(None, |_| {}).expect("app");
        let context = crate::fonts::test_context();
        app.ui_context = Some(context.clone());
        let path = "folder with spaces/file & 日本語.png";
        let a = app.tabs.open_new(path.into(), MediaKind::Image);
        app.media_kind = Some(MediaKind::Image);
        app.push_edit(EditOperation::RotateClockwise);
        let b = app.tabs.open_new("current.png".into(), MediaKind::Image);
        let output = context.run_ui(Default::default(), |_| {
            app.dispatch_tab_command(a, CopyFilePath);
        });
        assert!(
            output.platform_output.commands.iter().any(
                |command| matches!(command, egui::OutputCommand::CopyText(text) if text == path)
            )
        );
        assert_eq!(app.tabs.active().expect("active").id, b);
        assert!(app.edits[&a].is_dirty());
        assert!(app.pending_guard.is_none());
        app.tabs.close(a);
        let output = context.run_ui(Default::default(), |_| {
            app.dispatch_tab_command(a, CopyFilePath);
        });
        assert!(
            output.platform_output.commands.is_empty(),
            "stale tab IDs are ignored"
        );
    }

    #[test]
    fn right_click_menu_keeps_its_tab_target_and_supports_keyboard_commands() {
        let mut app = Application::new(None, |_| {}).expect("app");
        app.tabs.open_new("a.png".into(), MediaKind::Image);
        let target = app.tabs.open_new("b.png".into(), MediaKind::Image);
        let active = app.tabs.open_new("c.png".into(), MediaKind::Image);
        app.tabs
            .close_gallery(app.tabs.gallery().expect("media-only fixture"));
        let context = crate::fonts::test_context();
        context.global_style_mut(crate::chrome::style);
        let mut time = 0.0;
        let mut frame = |events| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_top_bar(ui, &mut actions),
            );
            time += 0.1;
            (output, actions)
        };
        for _ in 0..3 {
            frame(vec![]);
        }
        let pointer = egui::pos2(250.0, 16.0);
        for pressed in [true, false] {
            let (_, actions) = frame(vec![
                egui::Event::PointerMoved(pointer),
                egui::Event::PointerButton {
                    pos: pointer,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            assert!(actions.is_empty(), "context menu must not activate the tab");
        }
        for _ in 0..3 {
            frame(vec![]);
        }
        assert!(egui::Popup::is_any_open(&context));
        frame(vec![egui::Event::PointerMoved(egui::pos2(800.0, 500.0))]);
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        // The first arrow enters keyboard navigation after pointer opening.
        for _ in 0..6 {
            frame(vec![key(egui::Key::ArrowDown)]);
        }
        frame(vec![]);
        let (_, actions) = frame(vec![key(egui::Key::Enter)]);
        assert!(
            actions == [crate::UiAction::TabCommand(target, CopyFilePath)],
            "keyboard targets the contextual tab: {:?}",
            actions
                .iter()
                .filter_map(|action| match action {
                    crate::UiAction::TabCommand(id, command) => Some((*id, *command)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        );
        assert_eq!(app.tabs.active().expect("unchanged active").id, active);
        assert!(!egui::Popup::is_any_open(&context));
    }
}
