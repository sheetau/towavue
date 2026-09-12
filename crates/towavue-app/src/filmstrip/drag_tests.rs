use super::*;
use crate::*;
use std::time::SystemTime;
use towavue_core::{FolderMediaItem, FolderSnapshotSource, ShellIdentity};

fn snapshot(root: &Path) -> FolderSnapshot {
    FolderSnapshot {
        folder_identity: ShellIdentity::new(vec![]),
        folder_path: root.to_owned(),
        items: ["source.png", "other.png", "third.png"]
            .iter()
            .enumerate()
            .map(|(index, name)| FolderMediaItem {
                identity: ShellIdentity::new(vec![index as u8]),
                path: root.join(name),
                kind: MediaKind::Image,
            })
            .collect(),
        sort_columns: vec![],
        source: FolderSnapshotSource::LiveExplorerView,
        generation: 42,
        captured_at: SystemTime::now(),
    }
}

fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn frame(
    strip: &mut Filmstrip,
    context: &Context,
    snapshot: &FolderSnapshot,
    current: &Path,
    enabled: bool,
    input: egui::RawInput,
) -> (egui::FullOutput, Vec<UiAction>) {
    let mut actions = Vec::new();
    let output = context.run_ui(input, |_| {
        strip.show(
            context,
            context.content_rect(),
            Some(snapshot),
            Some(current),
            enabled,
            &mut actions,
        )
    });
    (output, actions)
}

fn input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(960.0, 576.0),
        )),
        events,
        ..Default::default()
    }
}

fn card(output: &egui::FullOutput, name: &str) -> Rect {
    let bounds = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree")
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(name))
        .expect("card")
        .1
        .bounds()
        .expect("bounds");
    Rect::from_min_max(
        egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
        egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
    )
}

#[test]
fn filmstrip_highlights_one_target_and_centers_two_line_names_below_the_preview() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_highlights_one_target_and_centers_two_line_names_below_the_preview",
    ) else {
        return;
    };
    let context = crate::fonts::test_context();
    context.enable_accesskit();
    let mut snapshot = snapshot(&root);
    snapshot.items[1].path = root.join(format!("{}.png", "長いファイル名と詳細な説明-".repeat(6)));
    let current = &snapshot.items[0].path;
    let name = display_name(&snapshot.items[1].path);
    let mut strip = Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
        .expect("filmstrip");
    for _ in 0..3 {
        frame(
            &mut strip,
            &context,
            &snapshot,
            current,
            true,
            input(vec![]),
        );
    }
    let output = frame(
        &mut strip,
        &context,
        &snapshot,
        current,
        true,
        input(vec![]),
    )
    .0;
    let rect = card(&output, &name);
    for _ in 0..2 {
        frame(
            &mut strip,
            &context,
            &snapshot,
            current,
            true,
            input(vec![egui::Event::PointerMoved(rect.center())]),
        );
    }
    let output = frame(
        &mut strip,
        &context,
        &snapshot,
        current,
        true,
        input(vec![]),
    )
    .0;
    let outlines = |output: &egui::FullOutput| {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect)
                    if rect.stroke.color == Color32::WHITE && rect.stroke.width == 1.0 =>
                {
                    Some(rect.rect)
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        outlines(&output),
        [rect.expand(3.0)],
        "hover replaces the current-item outline"
    );
    let text = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == name => Some(text),
            _ => None,
        })
        .expect("one filename label");
    assert_eq!(text.galley.rows.len(), 2);
    let bounds = text.galley.rect.translate(text.pos.to_vec2());
    assert!(bounds.top() >= rect.bottom() + 8.0);
    assert!(bounds.width() > rect.width());
    assert!((bounds.center().x - rect.center().x).abs() < 1.0);
    let third = card(&output, "third.png");
    let third_id = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree")
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("third.png"))
        .expect("third card")
        .0;
    let output = frame(
        &mut strip,
        &context,
        &snapshot,
        current,
        true,
        input(vec![egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Focus,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: third_id,
                data: None,
            },
        )]),
    )
    .0;
    assert_eq!(
        outlines(&output),
        [rect.expand(3.0)],
        "hover wins over a different keyboard focus"
    );
    frame(
        &mut strip,
        &context,
        &snapshot,
        current,
        true,
        input(vec![egui::Event::PointerGone]),
    );
    // egui keeps the last interaction position until the frame after PointerGone.
    let output = frame(
        &mut strip,
        &context,
        &snapshot,
        current,
        true,
        input(vec![]),
    )
    .0;
    assert_eq!(
        outlines(&output),
        [third.expand(3.0)],
        "keyboard target remains after pointer leaves"
    );
    context.memory_mut(|memory| {
        if let Some(id) = memory.focused() {
            memory.surrender_focus(id);
        }
    });
    let _ = context.run_ui(input(vec![]), |_| {});
    let reopened = frame(
        &mut strip,
        &context,
        &snapshot,
        current,
        true,
        input(vec![]),
    )
    .0;
    assert_eq!(
        outlines(&reopened),
        [card(&reopened, "source.png").expand(3.0)],
        "returning filmstrip is immediately opaque with the current item selected"
    );
}

#[test]
fn filmstrip_disables_underlying_seek_hover_and_input_until_closed() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_disables_underlying_seek_hover_and_input_until_closed",
    ) else {
        return;
    };
    let context = crate::fonts::test_context();
    context.enable_accesskit();
    let mut app = Application::new(None, |_| {}).expect("app");
    app.ui_context = Some(context.clone());
    app.folder_snapshot = Some(snapshot(&root));
    app.path = Some(root.join("source.png"));
    app.media_kind = Some(MediaKind::Image);
    let render = |app: &mut Application<_>, events| {
        let mut actions = Vec::new();
        let output = context.run_ui(input(events), |_| {
            app.draw_seek_bar(
                &context,
                Rect::from_min_max(egui::pos2(0.0, 550.0), egui::pos2(960.0, 576.0)),
                None,
                &mut actions,
            );
        });
        (output, actions)
    };
    let position = egui::pos2(900.0, 550.0);
    for open in [false, true, false] {
        app.filmstrip_open = open;
        for _ in 0..3 {
            render(&mut app, vec![egui::Event::PointerMoved(position)]);
        }
        let (output, _) = render(&mut app, vec![]);
        assert_eq!(
            output.platform_output.cursor_icon == egui::CursorIcon::PointingHand,
            !open,
            "seek hover cursor, filmstrip={open}"
        );
        assert_eq!(
            output
                .shapes
                .iter()
                .any(|shape| matches!(shape.shape, egui::Shape::Circle(_))),
            !open,
            "seek hover thumb, filmstrip={open}"
        );
        let (id, node) = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("tree")
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Image position"))
            .expect("seek slider");
        assert_eq!(node.is_disabled(), open);
        let (_, actions) = render(
            &mut app,
            vec![egui::Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::SetValue,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: *id,
                    data: Some(egui::accesskit::ActionData::NumericValue(3.0)),
                },
            )],
        );
        assert_eq!(
            actions.is_empty(),
            open,
            "accessible seek, filmstrip={open}"
        );
        render(&mut app, vec![pointer(position, true)]);
        let (_, actions) = render(&mut app, vec![pointer(position, false)]);
        assert_eq!(actions.is_empty(), open, "pointer seek, filmstrip={open}");
    }
}

#[test]
fn filmstrip_media_bounds_own_dimming_wheel_and_scrollbar_without_dragging_cards() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_media_bounds_own_dimming_wheel_and_scrollbar_without_dragging_cards",
    ) else {
        return;
    };
    let context = crate::fonts::test_context();
    context.enable_accesskit();
    let snapshot = snapshot(&root);
    let current = &snapshot.items[1].path;
    let mut strip = Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
        .expect("filmstrip");
    let media = Rect::from_min_max(egui::pos2(0.0, 40.0), egui::pos2(960.0, 536.0));
    let mut time = 0.0;
    let mut render = |strip: &mut Filmstrip, enabled, events| {
        let mut actions = Vec::new();
        let output = context.run_ui(
            egui::RawInput {
                time: Some(time),
                ..input(events)
            },
            |_| {
                strip.show(
                    &context,
                    media,
                    Some(&snapshot),
                    Some(current),
                    enabled,
                    &mut actions,
                )
            },
        );
        time += 0.1;
        assert!(
            actions.is_empty(),
            "scrolling must not open or detach media"
        );
        output
    };
    for _ in 0..3 {
        render(&mut strip, true, vec![]);
    }
    let output = render(&mut strip, true, vec![]);
    let bounds = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree")
        .nodes
        .iter()
        .find(|(_, node)| node.role() == egui::accesskit::Role::ScrollBar)
        .expect("scrollbar")
        .1
        .bounds()
        .expect("bounds");
    assert!(bounds.x0 >= f64::from(media.left() + 8.0));
    assert!(bounds.x1 <= f64::from(media.right() - 8.0));
    assert!(bounds.y1 <= f64::from(media.bottom() - 8.0));
    assert!((bounds.height() - 5.0).abs() < 0.01);
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Rect(rect) if rect.fill == Color32::from_black_alpha(191) && rect.rect == media
    )), "75% dim stays inside the media panel");
    assert!(
        output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(rect) if rect.rect.top() > media.bottom() - 20.0
                && rect.fill.a() > 0
                && rect.rect.bottom() <= media.bottom() - 8.0
                && rect.rect.width() > 20.0 && rect.rect.height() <= 5.0
        )),
        "visible scrollbar is inset from status/resize edges"
    );
    for (point, enabled, moves) in [
        (egui::pos2(480.0, 80.0), true, true),
        (egui::pos2(480.0, 500.0), true, true),
        (egui::pos2(2.0, 280.0), true, true),
        (egui::pos2(958.0, 280.0), true, true),
        (egui::pos2(480.0, 42.0), true, true),
        (egui::pos2(480.0, 534.0), true, true),
        (egui::pos2(480.0, 20.0), true, false),
        (egui::pos2(480.0, 555.0), true, false),
        (egui::pos2(480.0, 80.0), false, false),
        (egui::pos2(480.0, 534.0), false, false),
    ] {
        strip.focus = None;
        for _ in 0..3 {
            render(&mut strip, enabled, vec![egui::Event::PointerMoved(point)]);
        }
        let before = strip.scroll_offset;
        render(
            &mut strip,
            enabled,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -64.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        for _ in 0..20 {
            render(&mut strip, enabled, vec![]);
        }
        assert_eq!(
            strip.scroll_offset > before + 1.0,
            moves,
            "{point:?}, enabled={enabled}"
        );
        assert_eq!(strip.focus.as_deref(), Some(current.as_path()));
    }
    strip.focus = None;
    let grab = egui::pos2(480.0, media.bottom() - 10.0);
    for _ in 0..3 {
        render(&mut strip, true, vec![egui::Event::PointerMoved(grab)]);
    }
    let before = strip.scroll_offset;
    render(&mut strip, true, vec![pointer(grab, true)]);
    let moved = grab + egui::vec2(100.0, 0.0);
    render(&mut strip, true, vec![egui::Event::PointerMoved(moved)]);
    render(&mut strip, true, vec![pointer(moved, false)]);
    assert!(
        strip.scroll_offset > before + 1.0,
        "thumb drag scrolls instead of dragging a card"
    );
    for gutter in [
        egui::pos2(480.0, media.bottom() - 1.0),
        egui::pos2(media.left() + 1.0, media.bottom() - 10.0),
        egui::pos2(media.right() - 1.0, media.bottom() - 10.0),
    ] {
        strip.focus = None;
        for _ in 0..3 {
            render(&mut strip, true, vec![egui::Event::PointerMoved(gutter)]);
        }
        let before = strip.scroll_offset;
        render(&mut strip, true, vec![pointer(gutter, true)]);
        let end = gutter + egui::vec2(100.0, 0.0);
        render(&mut strip, true, vec![egui::Event::PointerMoved(end)]);
        render(&mut strip, true, vec![pointer(end, false)]);
        assert_eq!(strip.scroll_offset, before, "gutters must not drag the bar");
    }
    for kind in [MediaKind::Image, MediaKind::Video, MediaKind::Audio] {
        let context = crate::fonts::test_context();
        let mut app = Application::new(None, |_| {}).expect("app");
        app.ui_context = Some(context.clone());
        app.tabs.open_new(current.clone(), kind);
        app.path = Some(current.clone());
        app.media_kind = Some(kind);
        app.folder_snapshot = Some(snapshot.clone());
        app.filmstrip_open = true;
        for fullscreen in [false, true] {
            app.fullscreen = fullscreen;
            for _ in 0..3 {
                let output = context.run_ui(input(vec![]), |ui| {
                    app.draw_ui(ui, &mut Vec::new());
                });
                let bounds = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect) if rect.fill == Color32::from_black_alpha(191) => {
                            Some(rect.rect)
                        }
                        _ => None,
                    })
                    .expect("filmstrip dim");
                assert_eq!(bounds.left(), 0.0);
                assert_eq!(bounds.right(), 960.0);
                if fullscreen {
                    assert_eq!(bounds.top(), 0.0, "no reserved tab area in fullscreen");
                } else {
                    assert!(bounds.top() >= 24.0, "tab bar remains outside dim");
                }
                if fullscreen && kind != MediaKind::Audio {
                    assert_eq!(bounds.bottom(), 576.0);
                } else {
                    assert!(
                        bounds.bottom() < 556.0,
                        "status/timeline remain outside dim"
                    );
                }
            }
        }
    }
}

#[test]
fn filmstrip_clicks_dismiss_and_background_open_preserves_the_current_edit() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_clicks_dismiss_and_background_open_preserves_the_current_edit",
    ) else {
        return;
    };
    let snapshot = snapshot(&root);
    for item in &snapshot.items {
        std::fs::write(&item.path, b"owned path fixture").expect("fixture");
    }
    for (button, target_index) in [
        (egui::PointerButton::Primary, 0),
        (egui::PointerButton::Middle, 1),
        (egui::PointerButton::Middle, 0),
        (egui::PointerButton::Primary, 1),
    ] {
        let context = crate::fonts::test_context();
        context.enable_accesskit();
        let source = &snapshot.items[0].path;
        let target = &snapshot.items[target_index].path;
        let mut app = Application::new(None, |_| {}).expect("app");
        app.ui_context = Some(context.clone());
        let tab = app.tabs.open_new(source.clone(), MediaKind::Image);
        app.path = Some(source.clone());
        app.media_kind = Some(MediaKind::Image);
        app.displayed_tab = Some(tab);
        app.folder_snapshot = Some(snapshot.clone());
        app.filmstrip_open = true;
        app.edits
            .entry(tab)
            .or_default()
            .push(EditOperation::RotateClockwise, MediaKind::Image);
        let history = app.edits[&tab].clone();
        let generation = app.media_generation;
        for _ in 0..3 {
            frame(
                &mut app.filmstrip,
                &context,
                &snapshot,
                source,
                true,
                input(vec![]),
            );
        }
        let output = frame(
            &mut app.filmstrip,
            &context,
            &snapshot,
            source,
            true,
            input(vec![]),
        )
        .0;
        let point = card(&output, &display_name(target)).center();
        let offset = app.filmstrip.scroll_offset;
        let focus = app.filmstrip.focus.clone();
        let visible = app.filmstrip.visible.clone();
        let return_focus = Some((generation, egui::Id::new("filmstrip-origin")));
        app.filmstrip_return_focus = return_focus;
        for pressed in [true, false] {
            let (_, actions) = frame(
                &mut app.filmstrip,
                &context,
                &snapshot,
                source,
                true,
                input(vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]),
            );
            for action in actions {
                app.handle_ui_action(action);
            }
        }
        assert_eq!(
            app.filmstrip_open,
            button == egui::PointerButton::Middle,
            "primary clicks dismiss; background opening keeps filmstrip open"
        );
        assert_eq!(app.tabs.active().expect("current tab").id, tab);
        assert_eq!(app.path.as_ref(), Some(source));
        assert_eq!(app.media_generation, generation);
        assert_eq!(app.edits[&tab], history);
        if button == egui::PointerButton::Middle {
            assert_eq!(app.filmstrip.scroll_offset, offset);
            assert_eq!(app.filmstrip.focus, focus);
            assert_eq!(app.filmstrip.visible, visible);
            assert_eq!(app.filmstrip_return_focus, return_focus);
            assert_eq!(app.tabs.tabs().len(), 2);
            let added = &app.tabs.tabs()[1];
            assert_eq!(added.target.current_path(), target);
            assert!(!app.edits[&added.id].is_dirty());
            assert!(app.pending_guard.is_none());
            assert!(
                !app.image_loading,
                "background opening does not decode or disturb active media"
            );
            for _ in 0..3 {
                let output = context.run_ui(input(vec![]), |ui| {
                    app.draw_ui(ui, &mut Vec::new());
                });
                assert!(app.filmstrip_open);
                assert!(
                    output.shapes.iter().any(|shape| matches!(&shape.shape,
                        egui::Shape::Rect(rect) if rect.fill == Color32::from_black_alpha(191)
                    )),
                    "filmstrip remains drawn after background opening"
                );
                assert_eq!(app.tabs.active().expect("current tab").id, tab);
                assert_eq!(app.filmstrip.scroll_offset, offset);
                assert_eq!(app.edits[&tab], history);
            }
        } else {
            assert_eq!(app.tabs.tabs().len(), 1);
            assert_eq!(app.pending_guard.is_some(), target_index != 0);
            if target_index != 0 {
                app.resolve_guard(GuardDecision::Cancel);
                assert_eq!(app.path.as_ref(), Some(source));
                assert_eq!(app.edits[&tab], history);
            }
        }
    }
}

#[test]
fn filmstrip_empty_space_dismisses_without_opening_and_respects_disabled_loading_and_drag() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_empty_space_dismisses_without_opening_and_respects_disabled_loading_and_drag",
    ) else {
        return;
    };
    let snapshot = snapshot(&root);
    let media = Rect::from_min_max(egui::pos2(0.0, 40.0), egui::pos2(960.0, 536.0));
    for (loading, enabled, outside, drag) in [
        (false, true, false, false),
        (true, true, false, false),
        (false, false, false, false),
        (true, false, false, false),
        (false, true, true, false),
        (false, true, false, true),
    ] {
        let context = crate::fonts::test_context();
        let mut strip =
            Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                .expect("filmstrip");
        let mut render = |events| {
            let mut actions = Vec::new();
            let _ = context.run_ui(input(events), |_| {
                strip.show(
                    &context,
                    media,
                    (!loading).then_some(&snapshot),
                    Some(&snapshot.items[0].path),
                    enabled,
                    &mut actions,
                )
            });
            actions
        };
        let point = egui::pos2(480.0, if outside { 20.0 } else { 80.0 });
        for _ in 0..3 {
            render(vec![egui::Event::PointerMoved(point)]);
        }
        assert!(render(vec![pointer(point, true)]).is_empty());
        let release = if drag {
            point + egui::vec2(100.0, 0.0)
        } else {
            point
        };
        if drag {
            assert!(render(vec![egui::Event::PointerMoved(release)]).is_empty());
        }
        let actions = render(vec![pointer(release, false)]);
        if enabled && !outside && !drag {
            assert!(
                actions == [UiAction::CloseFilmstrip],
                "empty space closes only the filmstrip"
            );
        } else {
            assert!(
                actions.is_empty(),
                "disabled/outside/drag release cannot dismiss"
            );
        }
    }
}

#[test]
fn filmstrip_drag_copies_the_owned_path_once_and_reuses_its_preview() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_drag_copies_the_owned_path_once_and_reuses_its_preview",
    ) else {
        return;
    };
    for batched in [false, true] {
        let context = crate::fonts::test_context();
        context.global_style_mut(|style| {
            crate::chrome::style(style);
            style.animation_time = 0.0;
        });
        context.enable_accesskit();
        let snapshot = snapshot(&root);
        let source = &snapshot.items[0].path;
        let target = &snapshot.items[1].path;
        let mut strip =
            Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                .expect("strip");
        for _ in 0..3 {
            frame(&mut strip, &context, &snapshot, source, true, input(vec![]));
        }
        let texture = context.load_texture(
            "drag fixture",
            egui::ColorImage::filled([4, 2], Color32::GREEN),
            egui::TextureOptions::LINEAR,
        );
        strip
            .previews
            .insert(target.clone(), Ok((texture.clone(), None)));
        let output = frame(&mut strip, &context, &snapshot, source, true, input(vec![])).0;
        let rect = card(&output, "other.png");
        assert!(
            output.shapes.iter().any(|shape| matches!(
                &shape.shape, egui::Shape::Rect(shape)
                    if shape.rect == rect && shape.fill == crate::chrome::BORDER
            )),
            "card uses the shared grayscale surface"
        );
        let origin = rect.center();
        let moved = origin + egui::vec2(12.0, -120.0);
        let outside = egui::pos2(-20.0, 100.0);
        let generation = strip.generation;
        if !batched {
            frame(
                &mut strip,
                &context,
                &snapshot,
                source,
                true,
                input(vec![
                    egui::Event::PointerMoved(origin),
                    pointer(origin, true),
                ]),
            );
            let (output, actions) = frame(
                &mut strip,
                &context,
                &snapshot,
                source,
                true,
                input(vec![egui::Event::PointerMoved(moved)]),
            );
            assert!(actions.is_empty());
            let floating = Rect::from_min_size(moved - (origin - rect.min), rect.size());
            assert!(
                output
                    .shapes
                    .iter()
                    .any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect)
                        if rect.rect == floating && rect.fill == crate::chrome::BORDER)),
                "floating card"
            );
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == texture.id() && mesh.calc_bounds().center() == floating.center())), "same preview texture at floating position");
            frame(
                &mut strip,
                &context,
                &snapshot,
                source,
                true,
                input(vec![egui::Event::PointerGone]),
            );
            for _ in 0..2 {
                frame(&mut strip, &context, &snapshot, source, true, input(vec![]));
            }
        }
        let mut events = if batched {
            vec![egui::Event::PointerMoved(origin), pointer(origin, true)]
        } else {
            vec![]
        };
        events.extend([egui::Event::PointerMoved(outside), pointer(outside, false)]);
        let (_, actions) = frame(&mut strip, &context, &snapshot, source, true, input(events));
        assert!(
            actions
                == vec![UiAction::OpenWindow(
                    target.clone(),
                    snapshot.generation,
                    outside,
                    origin - rect.min,
                )]
        );
        assert!(
            frame(&mut strip, &context, &snapshot, source, true, input(vec![]))
                .1
                .is_empty()
        );
        assert_eq!(
            strip.generation, generation,
            "drag does not request another preview decode"
        );
    }
}

#[test]
fn filmstrip_window_origin_preserves_the_card_grab_across_sizes_and_densities() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_window_origin_preserves_the_card_grab_across_sizes_and_densities",
    ) else {
        return;
    };
    for width in [480.0, 960.0, 1440.0] {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.enable_accesskit();
            context.set_pixels_per_point(density);
            let snapshot = snapshot(&root);
            let source = &snapshot.items[0].path;
            let target = &snapshot.items[1].path;
            let mut strip =
                Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                    .expect("strip");
            let raw = |events| egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 576.0),
                )),
                ..input(events)
            };
            for _ in 0..3 {
                frame(&mut strip, &context, &snapshot, source, true, raw(vec![]));
            }
            let output = frame(&mut strip, &context, &snapshot, source, true, raw(vec![])).0;
            let rect = card(&output, "other.png");
            let offset = egui::vec2(24.0, 20.0);
            let origin = rect.min + offset;
            let outside = egui::pos2(width + 120.0, 110.0);
            frame(
                &mut strip,
                &context,
                &snapshot,
                source,
                true,
                raw(vec![pointer(origin, true)]),
            );
            let (_, actions) = frame(
                &mut strip,
                &context,
                &snapshot,
                source,
                true,
                raw(vec![
                    egui::Event::PointerMoved(outside),
                    pointer(outside, false),
                ]),
            );
            assert!(
                actions
                    == vec![UiAction::OpenWindow(
                        target.clone(),
                        snapshot.generation,
                        outside,
                        offset,
                    )],
                "width {width}, density {density}"
            );
            assert!(
                frame(&mut strip, &context, &snapshot, source, true, raw(vec![]))
                    .1
                    .is_empty()
            );
        }
    }
}

#[test]
fn filmstrip_window_request_preserves_the_original_tab_and_handles_failure_and_stale_actions() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_window_request_preserves_the_original_tab_and_handles_failure_and_stale_actions",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    let snapshot = snapshot(&root);
    let source = snapshot.items[0].path.clone();
    let target = snapshot.items[1].path.clone();
    let tab = app.tabs.open_new(source.clone(), MediaKind::Image);
    app.path = Some(source.clone());
    app.media_kind = Some(MediaKind::Image);
    app.state = PlaybackState::Paused;
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::RotateClockwise, MediaKind::Image);
    app.export_paths.insert(tab, root.join("saved.png"));
    app.folder_snapshot = Some(snapshot);
    app.filmstrip_open = true;
    let tabs = app.tabs.clone();
    let edits = app.edits.clone();
    let exports = app.export_paths.clone();
    let generation = app.media_generation;
    app.open_filmstrip_window(target.clone(), 41, |_| panic!("stale launch"));
    app.open_filmstrip_window(root.join("absent.png"), 42, |_| panic!("foreign path"));
    app.palette_open = true;
    app.open_filmstrip_window(target.clone(), 42, |_| panic!("covered launch"));
    app.palette_open = false;
    app.open_filmstrip_window(target.clone(), 42, |path| {
        assert_eq!(path, target);
        Err(std::io::Error::other("injected start failure"))
    });
    assert!(app.filmstrip_open);
    assert!(
        app.status_message
            .as_ref()
            .expect("diagnostic")
            .0
            .contains("injected start failure")
    );
    app.open_filmstrip_window(target.clone(), 42, |path| {
        assert_eq!(path, target);
        Ok(())
    });
    assert!(!app.filmstrip_open);
    app.open_filmstrip_window(target, 42, |_| panic!("duplicate launch"));
    assert_eq!(app.tabs, tabs);
    assert_eq!(app.edits, edits);
    assert_eq!(app.export_paths, exports);
    assert_eq!(app.path, Some(source));
    assert_eq!(app.state, PlaybackState::Paused);
    assert_eq!(app.media_generation, generation);
    assert!(app.pending_guard.is_none());
    let executable = root.join("app with spaces.exe");
    let path = root.join("日本語 & 'quoted' source.png");
    let command = crate::new_window_command(&executable, &path);
    assert_eq!(command.get_program(), executable.as_os_str());
    assert_eq!(command.get_args().collect::<Vec<_>>(), [path.as_os_str()]);
}

#[test]
fn filmstrip_drag_cancels_on_context_changes_without_replaying_the_release() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_drag_cancels_on_context_changes_without_replaying_the_release",
    ) else {
        return;
    };
    for case in 0..11 {
        let context = crate::fonts::test_context();
        context.enable_accesskit();
        let mut snapshot = snapshot(&root);
        let source = snapshot.items[0].path.clone();
        let mut strip =
            Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                .expect("strip");
        for _ in 0..3 {
            frame(
                &mut strip,
                &context,
                &snapshot,
                &source,
                true,
                input(vec![]),
            );
        }
        let output = frame(
            &mut strip,
            &context,
            &snapshot,
            &source,
            true,
            input(vec![]),
        )
        .0;
        let origin = card(&output, "other.png").center();
        let moved = origin + egui::vec2(12.0, -120.0);
        frame(
            &mut strip,
            &context,
            &snapshot,
            &source,
            true,
            input(vec![
                egui::Event::PointerMoved(origin),
                pointer(origin, true),
            ]),
        );
        frame(
            &mut strip,
            &context,
            &snapshot,
            &source,
            true,
            input(vec![egui::Event::PointerMoved(moved)]),
        );
        let mut raw = input(vec![]);
        let mut enabled = true;
        let mut current = source.clone();
        match case {
            0 => raw.events.push(pointer(moved, false)),
            1 => raw.events.push(egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }),
            2 => raw.focused = false,
            3 => enabled = false,
            4 => snapshot.generation += 1,
            5 => current = snapshot.items[2].path.clone(),
            6 => {
                raw.screen_rect = Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 400.0),
                ))
            }
            7 => context.set_pixels_per_point(2.0),
            8 => strip.clear(),
            9 => {
                let _ = context.run_ui(input(vec![]), |_| {});
            }
            10 => strip.clear_previews(),
            _ => unreachable!(),
        }
        assert!(
            frame(&mut strip, &context, &snapshot, &current, enabled, raw)
                .1
                .is_empty(),
            "cancel {case}"
        );
        let outside = egui::pos2(-20.0, 100.0);
        assert!(
            frame(
                &mut strip,
                &context,
                &snapshot,
                &current,
                true,
                input(vec![
                    egui::Event::PointerMoved(outside),
                    pointer(outside, false)
                ])
            )
            .1
            .is_empty(),
            "late release {case}"
        );
    }
}

pub(crate) fn hardware_drag_cancel<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
) {
    use crate::video_rotation::tests::frame as native_frame;
    let original_snapshot = app.folder_snapshot.clone();
    let original_open = app.filmstrip_open;
    let original_view = app.filmstrip.take_view();
    let path = app.path.clone().expect("source");
    let tabs = app.tabs.clone();
    let edits = app.edits.clone();
    let transport = app.state;
    let generation = app.media_generation;
    let mut snapshot = snapshot(path.parent().expect("folder"));
    snapshot.items[0].path = path;
    let context = app.ui_context.clone().expect("context");
    let texture = context.load_texture(
        "native filmstrip drag",
        egui::ColorImage::filled([16, 8], Color32::GREEN),
        egui::TextureOptions::LINEAR,
    );
    for item in &snapshot.items {
        app.filmstrip
            .previews
            .insert(item.path.clone(), Ok((texture.clone(), None)));
    }
    app.folder_snapshot = Some(snapshot);
    app.filmstrip_open = true;
    for _ in 0..3 {
        native_frame(app, vec![]);
    }
    let tree = native_frame(app, vec![]);
    let bounds = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("other.png"))
        .expect("native card")
        .1
        .bounds()
        .expect("bounds");
    let origin = egui::pos2(
        (bounds.x0 + bounds.x1) as f32 * 0.5,
        (bounds.y0 + bounds.y1) as f32 * 0.5,
    ) / context.pixels_per_point();
    let moved = origin + egui::vec2(-20.0, -100.0);
    native_frame(
        app,
        vec![egui::Event::PointerMoved(origin), pointer(origin, true)],
    );
    native_frame(app, vec![egui::Event::PointerMoved(moved)]);
    native_frame(
        app,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    native_frame(app, vec![pointer(moved, false)]);
    app.folder_snapshot = original_snapshot;
    app.filmstrip_open = original_open;
    app.filmstrip.restore_view(original_view);
    native_frame(app, vec![]);
    assert_eq!(app.tabs, tabs);
    assert_eq!(app.edits, edits);
    assert_eq!(app.state, transport);
    assert_eq!(app.media_generation, generation);
    assert_eq!(
        app.session
            .as_ref()
            .expect("session")
            .metrics()
            .cpu_transfer_count,
        0
    );
    eprintln!(
        "PASS hardware filmstrip drag: shared preview floating draw/cancel; unchanged tabs/history/transport/generation and CPU transfers 0; no child launched"
    );
}
