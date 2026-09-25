//! Per-window display language; command IDs and persisted shortcuts stay stable.
pub(crate) use towavue_core::localization::{Language, Text};
mod native_prompt;

#[derive(Clone, Copy, Default)]
pub(crate) struct Settings {
    pub display: Language,
    pub next: Language,
    pub saving: bool,
}

#[cfg(test)]
pub(crate) mod test_ui;

pub(crate) fn language(context: &egui::Context) -> Language {
    context
        .data(|data| data.get_temp(egui::Id::new("display-language")))
        .unwrap_or_default()
}

pub(crate) fn text(context: &egui::Context, key: Text) -> &'static str {
    key.in_language(language(context))
}

impl<N: Fn(crate::AppEvent) + Send + Sync + 'static> crate::Application<N> {
    pub(crate) fn language(&self) -> Language {
        self.ui_context
            .as_ref()
            .map_or(self.language_settings.display, language)
    }
}

pub(crate) fn set_language(context: &egui::Context, language: Language) {
    context.data_mut(|data| data.insert_temp(egui::Id::new("display-language"), language));
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::{Action, ActionRequest, TreeId};
    use towavue_core::{CommandContext, CommandId, MediaKind};

    #[test]
    fn japanese_window_titles_and_save_overlay_keep_user_names_and_dirty_state() {
        use crate::{Application, EditHistory, EditOperation, PlaybackState};
        let Some(root) = crate::tests::isolated_test_root(
            "localization::tests::japanese_window_titles_and_save_overlay_keep_user_names_and_dirty_state",
        ) else {
            return;
        };
        let context = test_ui::japanese_context(1.0);
        let mut app = Application::new(None, |_| {}).expect("app");
        app.ui_context = Some(context.clone());
        app.state = PlaybackState::Paused;
        assert_eq!(app.title(), "ギャラリー — towavue (一時停止)");
        let path = root.join("日本語{original}.png");
        let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
        app.path = Some(path);
        app.media_kind = Some(MediaKind::Image);
        app.edits
            .entry(tab)
            .or_insert_with(EditHistory::default)
            .push(EditOperation::RotateClockwise, MediaKind::Image);
        for (state, english, japanese) in [
            (PlaybackState::Loading, "Loading", "読み込み中"),
            (PlaybackState::Playing, "Playing", "再生中"),
            (PlaybackState::Paused, "Paused", "一時停止"),
            (PlaybackState::Ended, "Ended", "再生終了"),
            (PlaybackState::Faulted, "Faulted", "エラー"),
        ] {
            app.state = state;
            for (language, label) in [(Language::English, english), (Language::Japanese, japanese)]
            {
                set_language(&context, language);
                assert_eq!(
                    app.title(),
                    format!("日本語{{original}}.png * — towavue ({label})")
                );
            }
        }
        app.reading_settings.page_count = 2;
        app.reading_settings.first_page_count = 1;
        assert_eq!(app.reading_status(), "読書 2ページ · 先頭 1ページ");
        app.source_save.frozen = true;
        let output = context.run_ui(Default::default(), |ui| app.draw_ui(ui, &mut Vec::new()));
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text() == "ファイルを保存中…")));
        assert!(app.edits[&tab].is_dirty());
    }

    #[test]
    fn japanese_menu_cascade_preserves_command_identity_and_readable_choices() {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            if !crate::fonts::install(&context) {
                eprintln!(
                    "SKIP Japanese glyph qualification: no installed Japanese UI font; command routing remains checked"
                );
            }
            context.enable_accesskit();
            context.set_pixels_per_point(density);
            context.global_style_mut(crate::chrome::style);
            set_language(&context, Language::Japanese);
            let frame = |events| {
                let mut chosen = None;
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(480.0, 1100.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        ui.menu_button("Fixture", |ui| {
                            chosen = crate::menu::show_section(
                                ui,
                                CommandContext {
                                    media_kind: Some(MediaKind::Audio),
                                    ..Default::default()
                                },
                                &crate::shortcuts::defaults(),
                                Some(crate::menu::Section::View),
                            );
                        });
                    },
                );
                (output, chosen)
            };
            for title in ["Fixture", "音声のリピート", "全曲"] {
                for _ in 0..4 {
                    assert_eq!(frame(vec![]).1, None);
                }
                let output = frame(vec![]).0;
                let tree = output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("tree");
                let (id, node) = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| {
                        node.label()
                            .is_some_and(|label| label.trim_end_matches('\u{23f5}').trim() == title)
                    })
                    .unwrap_or_else(|| panic!("missing {title}"));
                assert!(!node.is_disabled(), "translated command stays enabled");
                if title == "全曲" {
                    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                        egui::Shape::Text(text) if text.galley.text() == title && !text.galley.elided)));
                    let bounds = node.bounds().expect("choice bounds");
                    assert!(bounds.x0 >= 0.0 && bounds.x1 <= 480.0);
                }
                let chosen = frame(vec![egui::Event::AccessKitActionRequest(ActionRequest {
                    action: Action::Click,
                    target_tree: TreeId::ROOT,
                    target_node: *id,
                    data: None,
                })])
                .1;
                assert_eq!(
                    chosen,
                    (title == "全曲").then_some(CommandId::AudioRepeatAll)
                );
            }
            assert_eq!(
                language(&egui::Context::default()),
                Language::English,
                "language is isolated between UI contexts"
            );
        }
    }
}
