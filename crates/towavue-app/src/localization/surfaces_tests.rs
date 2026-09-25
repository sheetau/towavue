use super::{Language, Text, set_language, test_ui};
use crate::*;

#[derive(Clone, Copy)]
enum Surface {
    Guard,
    Window,
    Toolbar,
    Status,
}

fn paint(
    app: &mut Application<impl Fn(AppEvent) + Send + Sync + 'static>,
    surface: Surface,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Vec<UiAction>) {
    let context = app.ui_context.clone().expect("context");
    let mut actions = Vec::new();
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            focused: true,
            events,
            ..Default::default()
        },
        |ui| match surface {
            Surface::Guard => app.draw_unsaved_guard(&context, &mut actions),
            Surface::Window => app.draw_ui(ui, &mut actions),
            Surface::Toolbar => app.draw_top_bar(ui, &mut actions),
            Surface::Status => {
                app.draw_status_bar(ui, &mut actions, &mut Vec::new());
            }
        },
    );
    (output, actions)
}

fn settle(
    app: &mut Application<impl Fn(AppEvent) + Send + Sync + 'static>,
    surface: Surface,
    size: egui::Vec2,
) -> egui::FullOutput {
    let mut output = egui::FullOutput::default();
    for _ in 0..4 {
        let result = paint(app, surface, size, vec![]);
        assert!(result.1.is_empty(), "painting cannot dispatch an action");
        output = result.0;
    }
    output
}

fn painted(output: &egui::FullOutput, label: &str) {
    assert!(
        output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.text() == label && !text.galley.elided)),
        "untruncated {label}"
    );
}

#[test]
fn japanese_fallback_guard_keeps_decisions_and_export_disable_state() {
    let Some(root) = crate::tests::isolated_test_root(
        "localization::surfaces_tests::japanese_fallback_guard_keeps_decisions_and_export_disable_state",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for width in [360.0, 800.0] {
            let size = egui::vec2(width, 600.0);
            let mut app = Application::new(None, |_| {}).expect("app");
            app.ui_context = Some(test_ui::japanese_context(density));
            let path = root.join("日本語{original}.png");
            let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
            app.path = Some(path.clone());
            app.media_kind = Some(MediaKind::Image);
            app.pending_guard = Some(GuardedAction::CloseTab(tab));
            app.edits
                .entry(tab)
                .or_default()
                .push(EditOperation::RotateClockwise, MediaKind::Image);
            let history = app.edits.clone();
            for (label, expected) in [
                ("保存して続行", GuardDecision::Save),
                ("編集を破棄", GuardDecision::Discard),
                ("キャンセル", GuardDecision::Cancel),
            ] {
                let output = settle(&mut app, Surface::Guard, size);
                assert_eq!(output.pixels_per_point, density);
                test_ui::visible_button(&output, label, size, true);
                painted(&output, "元のファイルに上書き保存しますか？");
                painted(&output, "日本語{original}.png");
                let event = test_ui::action(&output, label, None);
                let (_, actions) = paint(&mut app, Surface::Guard, size, vec![event]);
                assert!(
                    matches!(actions.as_slice(), [UiAction::ResolveGuard(decision)] if *decision == expected)
                );
                assert_eq!(app.edits, history, "UI dispatch alone cannot discard edits");
                assert!(
                    matches!(app.pending_guard, Some(GuardedAction::CloseTab(id)) if id == tab)
                );
            }
            let request = ExportRequest {
                source: path.clone(),
                target: path,
                kind: MediaKind::Image,
                operations: Vec::new(),
                hardware_encode: false,
            };
            let options = ExportOptions::default();
            app.active_export = Some(ActiveExport {
                progress: export_progress::ExportProgress::new(&request, &options, None),
                // Same-source refusal produces no media reads or writes.
                job: ExportJob::start(request.clone(), |_| {})
                    .expect("fixture worker")
                    .into(),
                tab,
                request,
                options,
                encoded: Duration::ZERO,
                analyzing_audio: false,
                cancelling: false,
                continuation: None,
            });
            let output = settle(&mut app, Surface::Guard, size);
            test_ui::visible_button(&output, "保存して続行", size, false);
            test_ui::visible_button(&output, "実行中の書き出しをキャンセル", size, true);
            let event = test_ui::action(&output, "実行中の書き出しをキャンセル", None);
            let (_, actions) = paint(&mut app, Surface::Guard, size, vec![event]);
            assert!(matches!(actions.as_slice(), [UiAction::CancelExport]));
            assert_eq!(app.edits, history);
        }
    }
}

#[test]
fn japanese_fallback_export_error_preserves_detail_and_dismisses_only_the_error() {
    let Some(_root) = crate::tests::isolated_test_root(
        "localization::surfaces_tests::japanese_fallback_export_error_preserves_detail_and_dismisses_only_the_error",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        let size = egui::vec2(800.0, 600.0);
        let mut app = Application::new(None, |_| {}).expect("app");
        app.ui_context = Some(test_ui::japanese_context(density));
        app.export_error = Some("Windows detail: 日本語{file}.png".into());
        app.pending_guard = Some(GuardedAction::Exit);
        let output = settle(&mut app, Surface::Window, size);
        painted(&output, "書き出しに失敗しました");
        painted(&output, "編集内容と既存のファイルは保持されています。");
        painted(&output, "Windows detail: 日本語{file}.png");
        test_ui::visible_button(&output, "OK", size, true);
        let event = test_ui::action(&output, "OK", None);
        let (_, actions) = paint(&mut app, Surface::Window, size, vec![event]);
        assert!(matches!(actions.as_slice(), [UiAction::DismissExportError]));
        assert!(matches!(app.pending_guard, Some(GuardedAction::Exit)));
        assert!(!app.exit_requested);
    }
}

#[test]
fn japanese_toolbar_and_status_keep_tab_and_playback_action_identity() {
    let Some(root) = crate::tests::isolated_test_root(
        "localization::surfaces_tests::japanese_toolbar_and_status_keep_tab_and_playback_action_identity",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        let size = egui::vec2(800.0, 600.0);
        let mut app = Application::new(None, |_| {}).expect("app");
        let context = test_ui::japanese_context(density);
        app.ui_context = Some(context.clone());
        let path = root.join("日本語{original}.mp4");
        let tab = app.tabs.open_new(path.clone(), MediaKind::Video);
        app.path = Some(path.clone());
        app.media_kind = Some(MediaKind::Video);
        app.state = PlaybackState::Paused;
        app.media_duration = Some(Duration::from_secs(120));
        app.edits
            .entry(tab)
            .or_default()
            .push(EditOperation::RotateClockwise, MediaKind::Video);
        let history = app.edits.clone();
        let output = settle(&mut app, Surface::Toolbar, size);
        let tree = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("tree");
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.label() == Some("towavue メニュー"))
        );
        let close = "タブを閉じる: 日本語{original}.mp4";
        let node = &tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(close))
            .expect("close tab")
            .1;
        assert_eq!(
            node.description(),
            Some(format!("{} — 未保存の変更があります", path.display()).as_str())
        );
        let event = test_ui::action(&output, close, None);
        let (_, actions) = paint(&mut app, Surface::Toolbar, size, vec![event]);
        assert!(matches!(actions.as_slice(), [UiAction::CloseTab(id)] if *id == tab));
        for (state, title) in [
            (PlaybackState::Paused, Text::PlayReplay),
            (PlaybackState::Playing, Text::Pause),
        ] {
            app.state = state;
            for width in [240.0, 800.0] {
                let size = egui::vec2(width, 600.0);
                let output = settle(&mut app, Surface::Status, size);
                let label = app.command_hint(
                    CommandId::TogglePause,
                    title.in_language(Language::Japanese),
                );
                let event = test_ui::action(&output, &label, None);
                let (_, actions) = paint(&mut app, Surface::Status, size, vec![event]);
                assert!(matches!(
                    actions.as_slice(),
                    [UiAction::Command(CommandId::TogglePause)]
                ));
                painted(&output, "50%");
            }
        }
        assert_eq!(app.edits, history);
        assert_eq!(app.tabs.active_id(), Some(tab));
        set_language(&context, Language::English);
        let output = settle(&mut app, Surface::Toolbar, size);
        assert!(
            output
                .platform_output
                .accesskit_update
                .expect("tree")
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Close tab: 日本語{original}.mp4"))
        );
    }
}
