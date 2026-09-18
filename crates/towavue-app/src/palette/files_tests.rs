use super::tests::{key, open_frame_at, picker_text};
use super::*;

fn button_position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    let tree = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree");
    let bounds = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label))
        .expect("remove button")
        .1
        .bounds()
        .expect("bounds");
    // The root supplies the pixel-scale transform; node bounds are logical points.
    egui::pos2(
        (bounds.x0 + bounds.x1) as f32 / 2.0,
        (bounds.y0 + bounds.y1) as f32 / 2.0,
    )
}

#[test]
fn file_and_folder_rows_remove_without_opening_and_keep_inline_labels_in_bounds() {
    for folders in [false, true] {
        for size in [egui::vec2(600.0, 400.0), egui::vec2(240.0, 180.0)] {
            for density in [1.0, 1.25, 2.0] {
                for batched in [false, true] {
                    let context = crate::fonts::test_context();
                    context.enable_accesskit();
                    context.global_style_mut(|style| {
                        style.animation_time = 0.0;
                        style.interaction.tooltip_delay = 60.0;
                    });
                    let mut palette = CommandPalette::default();
                    palette.open_files(folders);
                    let paths = [
                        PathBuf::from("C:/media/one.png"),
                        PathBuf::from("C:/media/two.png"),
                    ];
                    let sources = OpenSources {
                        files: &paths,
                        folders: &paths,
                        ..Default::default()
                    };
                    let layout = (size, 0.0, density);
                    let frame = |palette: &mut CommandPalette, events| {
                        open_frame_at(&context, palette, sources, events, layout)
                    };
                    let mut output = egui::FullOutput::default();
                    for _ in 0..5 {
                        output = frame(&mut palette, vec![]).0;
                    }
                    let group = if folders {
                        "folders"
                    } else {
                        "recently opened"
                    };
                    let name = picker_text(&output, "one.png").expect("name").0;
                    let label = picker_text(&output, group).expect("inline group").0;
                    assert!(
                        (name.pos.y + name.galley.size().y / 2.0
                            - label.pos.y
                            - label.galley.size().y / 2.0)
                            .abs()
                            < 1.0
                    );
                    assert_eq!(
                        label.galley.job.sections[0].format.color,
                        crate::chrome::MUTED
                    );
                    let remove_label =
                        format!("Remove from Recently Opened: {}", paths[0].display());
                    let pos = button_position(&output, &remove_label);
                    assert!(label.pos.x + label.galley.size().x <= pos.x - 10.0);
                    let press = |pressed| egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::CTRL,
                    };
                    let choices = if batched {
                        frame(
                            &mut palette,
                            vec![egui::Event::PointerMoved(pos), press(true), press(false)],
                        )
                        .1
                    } else {
                        frame(
                            &mut palette,
                            vec![egui::Event::PointerMoved(pos), press(true)],
                        );
                        frame(&mut palette, vec![press(false)]).1
                    };
                    assert_eq!(
                        choices,
                        [Choice::Open(RecentAction::Remove(
                            paths[0].clone(),
                            if folders {
                                RecentKind::Folder
                            } else {
                                RecentKind::File
                            }
                        ))]
                    );
                    for _ in 0..4 {
                        output = frame(&mut palette, vec![]).0;
                    }
                    assert!(
                        picker_text(&output, "one.png").is_none(),
                        "stale source snapshots cannot restore the row"
                    );
                    assert_eq!(palette.selected_path, Some(paths[1].clone()));
                    assert!(
                        picker_text(&output, group).is_some(),
                        "first remaining row owns its group label"
                    );
                    assert_eq!(
                        frame(
                            &mut palette,
                            vec![key(egui::Key::Enter, egui::Modifiers::ALT)]
                        )
                        .1,
                        [Choice::Open(RecentAction::Open(
                            paths[1].clone(),
                            if folders {
                                RecentKind::Folder
                            } else {
                                RecentKind::File
                            },
                            OpenTarget::Replace
                        ))]
                    );
                }
            }
        }
    }
}

#[test]
fn search_result_dismissal_is_query_local_and_recent_results_stay_deduplicated() {
    let context = crate::fonts::test_context();
    context.enable_accesskit();
    context.global_style_mut(|style| style.interaction.tooltip_delay = 60.0);
    let mut palette = CommandPalette::default();
    palette.open_files(false);
    palette.query = "image".into();
    let recent = vec![PathBuf::from("C:/media/image-old.png")];
    let result = towavue_runtime_windows::FileSearchResult {
        request: towavue_runtime_windows::FileSearchRequest {
            root: "C:/media".into(),
            query: "image".into(),
        },
        paths: vec![recent[0].clone(), PathBuf::from("C:/media/image-new.png")],
        matches: 2,
        skipped: 0,
        error: None,
    };
    let sources = OpenSources {
        files: &recent,
        search: Some(&result),
        ..Default::default()
    };
    let frame = |palette: &mut CommandPalette, events| {
        open_frame_at(
            &context,
            palette,
            sources,
            events,
            (egui::vec2(600.0, 400.0), 0.0, 1.0),
        )
    };
    for _ in 0..5 {
        frame(&mut palette, vec![]);
    }
    let output = frame(
        &mut palette,
        vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
    )
    .0;
    assert_eq!(palette.selected_path, Some(result.paths[1].clone()));
    assert!(picker_text(&output, "recently opened").is_some());
    assert!(picker_text(&output, "file results").is_some());
    let label = format!("Dismiss search result: {}", result.paths[1].display());
    let pos = button_position(&output, &label);
    let press = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(
        &mut palette,
        vec![egui::Event::PointerMoved(pos), press(true)],
    );
    assert_eq!(
        frame(&mut palette, vec![press(false)]).1,
        [Choice::Open(RecentAction::Remove(
            result.paths[1].clone(),
            RecentKind::File
        ))]
    );
    for _ in 0..4 {
        frame(&mut palette, vec![]);
    }
    assert_eq!(palette.selected_path, Some(recent[0].clone()));
    assert!(picker_text(&frame(&mut palette, vec![]).0, "image-new.png").is_none());
    frame(&mut palette, vec![egui::Event::Text("x".into())]);
    frame(
        &mut palette,
        vec![key(egui::Key::Backspace, egui::Modifiers::NONE)],
    );
    let output = frame(&mut palette, vec![]).0;
    assert!(picker_text(&output, "image-new.png").is_some());
    assert_eq!(output.shapes.iter().filter(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "image-old.png")).count(), 1);
}

#[test]
fn filename_priority_and_full_row_background_survive_hover_and_selection() {
    for folders in [false, true] {
        for density in [1.0, 1.25, 2.0] {
            for width in [240.0, 600.0] {
                for long in [false, true] {
                    let context = crate::fonts::test_context();
                    context.enable_accesskit();
                    context.global_style_mut(|style| {
                        crate::chrome::style(style);
                        style.animation_time = 0.0;
                        style.interaction.tooltip_delay = 60.0;
                    });
                    let name = if long {
                        "long-filename-".repeat(20) + ".png"
                    } else {
                        "2026-09-18_holiday_photo_0123.png".into()
                    };
                    let parent =
                        "C:/long-parent-directory/".to_owned() + &"nested-directory/".repeat(30);
                    let paths = [
                        PathBuf::from(format!("{parent}{name}")),
                        PathBuf::from(format!("{parent}second.png")),
                    ];
                    let sources = OpenSources {
                        files: &paths,
                        folders: &paths,
                        ..Default::default()
                    };
                    let mut palette = CommandPalette::default();
                    palette.open_files(folders);
                    let render = |palette: &mut CommandPalette, events| {
                        open_frame_at(
                            &context,
                            palette,
                            sources,
                            events,
                            (egui::vec2(width, 400.0), 0.0, density),
                        )
                        .0
                    };
                    let mut output = render(&mut palette, vec![]);
                    for _ in 0..4 {
                        output = render(&mut palette, vec![]);
                    }
                    let rect = |output: &egui::FullOutput, path: &Path| {
                        let tree = output
                            .platform_output
                            .accesskit_update
                            .as_ref()
                            .expect("tree");
                        let bounds = tree
                            .nodes
                            .iter()
                            .find(|(_, node)| node.label() == Some(path.to_string_lossy().as_ref()))
                            .expect("path")
                            .1
                            .bounds()
                            .expect("bounds");
                        egui::Rect::from_min_max(
                            egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                            egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                        )
                    };
                    let body = rect(&output, &paths[0]);
                    let (label, clip) = picker_text(&output, &name).expect("filename");
                    let label_pos = label.pos;
                    assert!(label.galley.size().x <= body.width() * 0.8 + 1.0 / density);
                    if long {
                        assert!(label.galley.elided);
                        assert!(
                            label.galley.size().x > body.width() * 0.6,
                            "filename receives priority over the long parent"
                        );
                    } else if width == 600.0 {
                        assert!(
                            !label.galley.elided,
                            "fitting filename must remain complete"
                        );
                    }
                    assert!(
                        clip.contains_rect(egui::Rect::from_min_size(
                            label.pos,
                            label.galley.size()
                        ))
                    );
                    let assert_background =
                        |output: &egui::FullOutput, body: egui::Rect, label: &str| {
                            let point = button_position(output, label);
                            let full = egui::Rect::from_min_max(
                                body.min,
                                body.max + egui::vec2(22.0, 0.0),
                            );
                            assert!(
                                output.shapes.iter().any(|shape| matches!(&shape.shape,
                            egui::Shape::Rect(painted) if painted.fill == crate::chrome::HOVER
                                && painted.rect.min.distance(full.min) < 0.1
                                && painted.rect.max.distance(full.max) < 0.1
                                && painted.rect.contains(point))),
                                "background includes the independent removal button"
                            );
                        };
                    assert_background(
                        &output,
                        body,
                        &format!("Remove from Recently Opened: {}", paths[0].display()),
                    );
                    let second = rect(&output, &paths[1]);
                    let before = picker_text(&output, "second.png").expect("second").0.pos;
                    let hovered =
                        egui::Rect::from_min_max(second.min, second.max - egui::vec2(22.0, 0.0));
                    let assert_rect = |actual: egui::Rect, expected: egui::Rect| {
                        assert!(
                            actual.min.distance(expected.min) <= 1.0 / density
                                && actual.max.distance(expected.max) <= 1.0 / density,
                            "close-slot geometry within one physical pixel: {actual:?} != {expected:?}"
                        );
                    };
                    for point in [
                        second.center(),
                        second.right_center() - egui::vec2(11.0, 0.0),
                    ] {
                        render(&mut palette, vec![egui::Event::PointerMoved(point)]);
                        output = render(&mut palette, vec![]);
                        assert_rect(rect(&output, &paths[1]), hovered);
                        assert!(
                            picker_text(&output, "second.png")
                                .expect("second")
                                .0
                                .pos
                                .distance(before)
                                <= 1.0 / density
                        );
                        assert!(
                            picker_text(&output, &name)
                                .expect("first")
                                .0
                                .pos
                                .distance(label_pos)
                                <= 1.0 / density
                        );
                        assert_background(
                            &output,
                            rect(&output, &paths[1]),
                            &format!("Remove from Recently Opened: {}", paths[1].display()),
                        );
                    }
                    output = render(
                        &mut palette,
                        vec![
                            egui::Event::PointerGone,
                            key(egui::Key::ArrowDown, egui::Modifiers::NONE),
                        ],
                    );
                    assert_rect(rect(&output, &paths[1]), hovered);
                    assert_background(
                        &output,
                        rect(&output, &paths[1]),
                        &format!("Remove from Recently Opened: {}", paths[1].display()),
                    );
                    output = render(
                        &mut palette,
                        vec![key(egui::Key::ArrowUp, egui::Modifiers::NONE)],
                    );
                    assert_rect(rect(&output, &paths[1]), second);
                }
            }
        }
    }
}
