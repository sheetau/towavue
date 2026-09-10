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
    let Some(index) = tabs.tabs().iter().position(|tab| tab.id == target) else {
        return Vec::new();
    };
    tabs.tabs()
        .iter()
        .enumerate()
        .filter(|(position, tab)| match command {
            CloseTab => tab.id == target,
            CloseOtherTabs => tab.id != target,
            CloseTabsLeft => *position < index,
            CloseTabsRight => *position > index,
            CloseAllTabs => true,
            _ => false,
        })
        .map(|(_, tab)| tab.id)
        .collect()
}

pub fn show(
    ui: &mut egui::Ui,
    tabs: &TabSet,
    target: TabId,
    can_reopen: bool,
    shortcuts: &ShortcutBindings,
) -> Option<CommandId> {
    let keyboard = crate::menu::MenuKeyboard::begin(ui);
    let mut items = Vec::new();
    let mut chosen = None;
    for group in [
        &[
            CloseTab,
            CloseOtherTabs,
            CloseTabsLeft,
            CloseTabsRight,
            CloseAllTabs,
        ][..],
        &[ReopenClosedTab],
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
                CopyFilePath | RevealFile => tabs.tabs().iter().any(|tab| tab.id == target),
                _ => !close_targets(tabs, target, *command).is_empty(),
            };
            let response = ui.add_enabled(
                enabled,
                egui::Button::new(definition.title).shortcut_text(
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
                ),
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
    fn group_save_continues_after_each_export_and_preserves_sources() {
        let root =
            std::env::temp_dir().join(format!("towavue-group-export-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("owned fixture directory");
        let (sent, events) = std::sync::mpsc::channel();
        let mut app = Application::new(None, move |event| {
            let _ = sent.send(event);
        })
        .expect("app");
        let bytes = b"P6\n2 1\n255\n\xff\x00\x00\x00\xff\x00";
        let mut tabs = Vec::new();
        for index in 0..2 {
            let path = root.join(format!("{index}.ppm"));
            std::fs::write(&path, bytes).expect("owned source");
            let id = app.tabs.open_new(path, MediaKind::Image);
            app.edits
                .entry(id)
                .or_default()
                .push(EditOperation::RotateClockwise, MediaKind::Image);
            app.export_paths
                .insert(id, root.join(format!("{index}.png")));
            tabs.push(id);
        }
        app.dispatch_tab_command(tabs[1], CloseAllTabs);
        for expected in &tabs {
            assert_eq!(app.tabs.active().expect("dirty tab").id, *expected);
            app.resolve_guard(GuardDecision::Save);
            assert!(app.active_export.is_some());
            app.request_guarded(GuardedAction::CloseTabs(tabs.clone()));
            assert_eq!(
                app.tabs.tabs().len(),
                2,
                "active export blocks the whole close request"
            );
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while app.active_export.is_some() {
                let event = events
                    .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                    .expect("export event");
                if let crate::AppEvent::Export(event) = event {
                    app.handle_export_event(event);
                }
            }
            assert!(app.export_error.is_none(), "{:?}", app.export_error);
        }
        assert!(app.tabs.tabs().is_empty() && app.pending_guard.is_none());
        assert_eq!(app.closed_tabs.len(), 2);
        drop(app);
        for index in 0..2 {
            let source = root.join(format!("{index}.ppm"));
            let output = root.join(format!("{index}.png"));
            assert_eq!(std::fs::read(&source).expect("source"), bytes);
            assert!(
                std::fs::read(&output)
                    .expect("exported PNG")
                    .starts_with(b"\x89PNG")
            );
            std::fs::remove_file(source).expect("remove owned source");
            std::fs::remove_file(output).expect("remove owned output");
        }
        std::fs::remove_dir(root).expect("remove owned directory");
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
        assert_eq!(app.closed_tabs.len(), 3);
    }

    #[test]
    fn closed_history_is_bounded_path_only_and_reopen_forces_a_new_tab() {
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
        app.closed_tabs.push_back(root.join("missing.png"));
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
        for _ in 0..5 {
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
