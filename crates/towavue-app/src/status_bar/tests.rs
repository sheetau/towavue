use super::*;

type App = Application<fn(AppEvent)>;

fn frame(app: &mut App, events: Vec<egui::Event>) -> (egui::FullOutput, Vec<UiAction>) {
    let context = app.ui_context.clone().expect("context");
    let mut actions = Vec::new();
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(960.0, 400.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            app.draw_status_bar(ui, &mut actions, &mut Vec::new());
        },
    );
    (output, actions)
}

fn target(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    let node = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree")
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label))
        .expect("status control");
    let bounds = node.1.bounds().expect("bounds");
    assert!((bounds.height() - 24.0).abs() < 0.01);
    egui::pos2(
        ((bounds.x0 + bounds.x1) * 0.5) as f32,
        ((bounds.y0 + bounds.y1) * 0.5) as f32,
    )
}

fn click(app: &mut App, pos: egui::Pos2) -> (egui::FullOutput, Vec<UiAction>) {
    click_button(app, pos, egui::PointerButton::Primary)
}

fn click_button(
    app: &mut App,
    pos: egui::Pos2,
    button: egui::PointerButton,
) -> (egui::FullOutput, Vec<UiAction>) {
    frame(app, vec![egui::Event::PointerMoved(pos)]);
    frame(
        app,
        vec![egui::Event::PointerButton {
            pos,
            button,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    frame(
        app,
        vec![egui::Event::PointerButton {
            pos,
            button,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    )
}

#[test]
fn status_access_uses_commands_and_path_menu_ignores_notifications() {
    let Some(root) = crate::tests::isolated_test_root(
        "status_bar::tests::status_access_uses_commands_and_path_menu_ignores_notifications",
    ) else {
        return;
    };
    let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
    let context = fonts::test_context();
    context.global_style_mut(chrome::style);
    context.enable_accesskit();
    app.ui_context = Some(context.clone());
    let path = root.join("Pictures").join("photo.png");
    app.displayed_tab = Some(app.tabs.open_new(path.clone(), MediaKind::Image));
    app.image = Some(
        ImagePresentation::from_decoded(&context, &path, tab_transfer::tests::decoded(false))
            .expect("pixels"),
    );
    app.nearest_images = false;
    app.path = Some(path);
    app.media_kind = Some(MediaKind::Image);
    app.state = PlaybackState::Paused;
    for _ in 0..3 {
        frame(&mut app, vec![]);
    }
    let output = frame(&mut app, vec![]).0;
    let fit = target(&output, "Fit");
    assert!(
        click(&mut app, fit)
            .1
            .iter()
            .any(|action| matches!(action, UiAction::Command(CommandId::CoverWindow)))
    );
    let output = frame(&mut app, vec![]).0;
    let smooth = target(&output, "Smooth");
    assert!(click(&mut app, smooth).1.iter().any(|action| matches!(
        action,
        UiAction::Command(CommandId::ToggleImageInterpolation)
    )));
    let format = target(&output, "PNG 2×2");
    assert!(
        click(&mut app, format)
            .1
            .iter()
            .any(|action| matches!(action, UiAction::StatusPalette("resize / resample")))
    );
    let path = target(&output, "Pictures\\photo.png");
    assert!(
        click(&mut app, path)
            .1
            .iter()
            .any(|action| matches!(action, UiAction::Command(CommandId::SearchCurrentFiles)))
    );
    assert!(!egui::Popup::is_any_open(&context));
    click_button(&mut app, path, egui::PointerButton::Secondary);
    let output = frame(&mut app, vec![]).0;
    assert!(
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("tree")
            .nodes
            .iter()
            .any(|(_, node)| node.label() == Some("Copy file path"))
    );
    egui::Popup::close_all(&context);
    app.set_status("A temporary notice".into());
    frame(&mut app, vec![]);
    assert!(click(&mut app, path).1.is_empty());
    click_button(&mut app, path, egui::PointerButton::Secondary);
    assert!(!egui::Popup::is_any_open(&context));
    app.status_message = None;
    let mut decoded = (*tab_transfer::tests::decoded(false)).clone();
    decoded.frames.push(decoded.frames[0].clone());
    app.image = Some(
        ImagePresentation::from_decoded(
            &context,
            app.path.as_ref().expect("path"),
            Arc::new(decoded),
        )
        .expect("animation"),
    );
    let output = frame(&mut app, vec![]).0;
    let frames = app
        .status_info()
        .fields
        .into_iter()
        .find(|field| field.contains("frames"))
        .expect("frame count");
    let point = target(&output, &frames);
    assert!(
        click(&mut app, point)
            .1
            .iter()
            .any(|action| matches!(action, UiAction::Command(CommandId::ToggleTimeline)))
    );
}

#[test]
fn gallery_search_replaces_status_content_without_a_media_header() {
    let Some(_root) = crate::tests::isolated_test_root(
        "status_bar::tests::gallery_search_replaces_status_content_without_a_media_header",
    ) else {
        return;
    };
    let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
    let context = fonts::test_context();
    context.global_style_mut(chrome::style);
    context.enable_accesskit();
    app.ui_context = Some(context.clone());
    app.status_message = None;
    for _ in 0..3 {
        frame(&mut app, vec![]);
    }
    let output = frame(&mut app, vec![]).0;
    let search = target(
        &output,
        localization::Text::FilmstripSearch.in_language(app.language()),
    );
    assert!(search.y > 370.0);
    click(&mut app, search);
    frame(&mut app, vec![egui::Event::Text("photo".into())]);
    assert_eq!(app.gallery_search, "photo");
    let output = frame(&mut app, vec![]).0;
    let close = target(
        &output,
        localization::Text::Cancel.in_language(app.language()),
    );
    click(&mut app, close);
    assert!(app.gallery_search.is_empty());
}

#[test]
fn status_hover_keeps_an_antialias_margin_outside_both_edges() {
    for density in [1.0, 1.25, 2.0] {
        let context = fonts::test_context();
        context.set_pixels_per_point(density);
        context.global_style_mut(chrome::style);
        let rect = egui::Rect::from_min_size(egui::pos2(10.3, 10.0), egui::vec2(70.3, 24.0));
        let draw = || {
            context.run_ui(
                egui::RawInput {
                    events: vec![egui::Event::PointerMoved(rect.center())],
                    ..Default::default()
                },
                |ui| {
                    item_at(ui, rect, "", true);
                },
            )
        };
        draw();
        let output = draw();
        let shape = output
            .shapes
            .iter()
            .find(|shape| matches!(&shape.shape,egui::Shape::Rect(r) if r.fill==chrome::ACTIVE_TAB))
            .expect("hover fill");
        assert!(shape.clip_rect.right() >= rect.right() + 0.99 / density);
        assert!(shape.clip_rect.left() <= rect.left() - 0.99 / density);
    }
}
