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
