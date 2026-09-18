//! Plain list keys belong to the visible list, never to text editors or popups.

#[derive(Clone, Copy, Default)]
pub(crate) struct Navigation {
    pub steps: isize,
    pub pages: isize,
}

impl Navigation {
    pub fn read(input: &mut egui::InputState) -> Self {
        let mut navigation = Self::default();
        input.events.retain(|event| {
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                return true;
            };
            if *modifiers != egui::Modifiers::NONE {
                return true;
            }
            match key {
                egui::Key::ArrowUp => navigation.steps -= 1,
                egui::Key::ArrowDown => navigation.steps += 1,
                egui::Key::PageUp => navigation.pages -= 1,
                egui::Key::PageDown => navigation.pages += 1,
                _ => return true,
            }
            false
        });
        navigation
    }

    pub fn for_list(ui: &egui::Ui) -> Self {
        if !available(ui) {
            return Self::default();
        }
        let navigation = ui.input_mut(Self::read);
        if navigation.moved() {
            // egui has already scheduled directional focus movement for this pass.
            ui.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
        }
        navigation
    }

    pub fn moved(self) -> bool {
        self.steps != 0 || self.pages != 0
    }

    pub fn rows(self, height: f32, row: f32) -> isize {
        self.steps + self.pages * (height / row).floor().max(1.0) as isize
    }

    pub fn destination(self, current: usize, count: usize, height: f32, row: f32) -> usize {
        current
            .saturating_add_signed(self.rows(height, row))
            .min(count.saturating_sub(1))
    }
}

/// Keyboard-only suppression keeps uncovered pointer targets usable under a picker.
/// Tie the flag to the current frame so an absent owner cannot leave a stale lock.
pub(crate) fn block_for_frame(context: &egui::Context, blocked: bool) {
    // Context data callbacks hold its lock; read the frame before entering one.
    let frame = context.cumulative_frame_nr();
    context.data_mut(|data| {
        data.insert_temp(egui::Id::new("list-keyboard-blocked"), (frame, blocked))
    });
}

pub(crate) fn available(ui: &egui::Ui) -> bool {
    let blocked = ui
        .ctx()
        .data(|data| data.get_temp::<(u64, bool)>(egui::Id::new("list-keyboard-blocked")))
        .is_some_and(|(frame, blocked)| frame == ui.ctx().cumulative_frame_nr() && blocked);
    !(blocked
        || !ui.is_enabled()
        || ui.ctx().text_edit_focused()
        || egui::Popup::is_any_open(ui.ctx())
        || ui.input(|input| {
            !input.focused
                || input.events.contains(&egui::Event::WindowFocused(false))
                || input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Ime(_)))
        }))
}

/// Scroll an unfocused list without selecting or activating its items.
pub(crate) fn unfocused_scroll(
    ui: &egui::Ui,
    salt: impl egui::AsIdSalt,
    row_height: f32,
) -> Option<f32> {
    if ui.memory(|memory| memory.focused().is_some()) {
        return None;
    }
    let navigation = Navigation::for_list(ui);
    if !navigation.moved() {
        return None;
    }
    let offset =
        egui::scroll_area::State::load(ui.ctx(), ui.make_persistent_id(egui::IdSalt::new(salt)))
            .map_or(0.0, |state| state.offset.y);
    Some(
        (offset
            + navigation.steps as f32 * row_height
            + navigation.pages as f32 * ui.available_height())
        .max(0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scroll_style::ScrollAreaStyle;

    #[test]
    fn list_navigation_scrolls_unfocused_content_and_respects_disabled_and_text_focus() {
        for density in [1.0, 1.25, 2.0] {
            let context = egui::Context::default();
            context.set_pixels_per_point(density);
            let mut text = String::new();
            let mut frame = |key: Option<egui::Key>, disabled, edit_focus| {
                let mut offset = 0.0;
                let _ = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(400.0, 300.0),
                        )),
                        events: key
                            .into_iter()
                            .map(|key| egui::Event::Key {
                                key,
                                physical_key: None,
                                pressed: true,
                                repeat: false,
                                modifiers: egui::Modifiers::NONE,
                            })
                            .collect(),
                        ..Default::default()
                    },
                    |ui| {
                        if disabled {
                            ui.disable();
                        }
                        if edit_focus {
                            ui.text_edit_singleline(&mut text).request_focus();
                        }
                        let mut scroll = egui::ScrollArea::vertical()
                            .id_salt("test-list")
                            .auto_shrink([false, false]);
                        if let Some(offset) = unfocused_scroll(ui, "test-list", 24.0) {
                            scroll = scroll.vertical_scroll_offset(offset);
                        }
                        offset = scroll
                            .show_rows_styled(ui, 24.0, 1000, |ui, _| {
                                ui.add_space(24000.0);
                            })
                            .state
                            .offset
                            .y;
                    },
                );
                offset
            };
            frame(None::<egui::Key>, false, false);
            let step = frame(Some(egui::Key::ArrowDown), false, false);
            assert_eq!(step, 24.0);
            let page = frame(Some(egui::Key::PageDown), false, false);
            assert!(page - step >= 280.0 && page - step <= 300.0);
            assert_eq!(frame(Some(egui::Key::PageDown), true, false), page);
            frame(None, false, true);
            assert_eq!(frame(Some(egui::Key::ArrowDown), false, true), page);
        }
    }

    #[test]
    fn list_navigation_keyboard_block_keeps_pointer_targets_and_expires_with_frame() {
        let context = egui::Context::default();
        for blocked in [true, false] {
            let _ = context.run_ui(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key: egui::Key::PageDown,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..Default::default()
                },
                |ui| {
                    if blocked {
                        block_for_frame(&context, true);
                    }
                    assert!(ui.button("Uncovered pointer target").enabled());
                    assert_eq!(Navigation::for_list(ui).moved(), !blocked);
                    assert_eq!(
                        ui.input(|input| input.key_pressed(egui::Key::PageDown)),
                        blocked
                    );
                },
            );
        }
    }

    #[test]
    fn list_navigation_native_ownership_preserves_media_and_overlay_routes() {
        let Some(_root) = crate::tests::isolated_test_root(
            "list_navigation::tests::list_navigation_native_ownership_preserves_media_and_overlay_routes",
        ) else {
            return;
        };
        let mut app = crate::Application::new(None, |_| {}).expect("app");
        app.ui_context = Some(crate::fonts::test_context());
        let key = "Down".parse::<towavue_core::KeyStroke>().expect("key");
        assert!(app.list_owns_key(&key));
        assert!(!app.list_owns_key(&"Ctrl+Down".parse().expect("key")));
        app.shortcuts.set(
            towavue_core::CommandId::OpenFile,
            "Alt+Y Down".parse().expect("chord"),
        );
        app.process_shortcut("Alt+Y".parse().expect("prefix"));
        assert!(app.prefix_started.is_some());
        assert!(
            app.owns_focused_shortcut(&key),
            "chord suffix must route before egui list input"
        );
        app.cancel_shortcut_prefix();
        app.palette_open = true;
        assert!(!app.list_owns_key(&key));
        app.palette_open = false;
        app.native_ime_composing = true;
        assert!(!app.list_owns_key(&key));
        app.native_ime_composing = false;
        app.dispatch(towavue_core::CommandId::OpenKeyboardSettings);
        assert!(app.list_owns_key(&key));
        app.tabs.open_new(
            std::path::PathBuf::from("owned.wav"),
            towavue_core::MediaKind::Audio,
        );
        app.media_kind = Some(towavue_core::MediaKind::Audio);
        assert!(app.list_owns_key(&key));
        app.media_kind = Some(towavue_core::MediaKind::Video);
        assert!(!app.list_owns_key(&key));
    }
}
