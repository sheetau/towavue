use super::tests::{key, open_frame_at, picker_text};
use super::*;

#[test]
fn file_expressions_keep_recent_priority_and_enter_uses_the_filtered_path() {
    let context = crate::fonts::test_context();
    context.enable_accesskit();
    let paths = [
        "C:/Media/alpha cat.png",
        "C:/Media/alpha dog.mp4",
        "C:/Media/beta.wav",
    ]
    .map(PathBuf::from);
    let sources = OpenSources {
        files: &paths,
        ..Default::default()
    };
    let mut palette = CommandPalette::default();
    palette.open_files(false);
    for (query, selected) in [
        ("alpha image:|video:", Some(0)),
        ("!<alpha image:>", Some(1)),
        ("folder:C:/Media ext:wav", Some(2)),
        ("\"alpha cat\"", Some(0)),
        ("<image:", None),
    ] {
        palette.query = query.into();
        palette.selected_path = None;
        for _ in 0..3 {
            open_frame_at(
                &context,
                &mut palette,
                sources,
                vec![],
                (egui::vec2(800.0, 500.0), 0.0, 1.0),
            );
        }
        assert_eq!(
            palette.selected_path.as_ref(),
            selected.map(|index| &paths[index]),
            "{query}"
        );
        let choices = open_frame_at(
            &context,
            &mut palette,
            sources,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
            (egui::vec2(800.0, 500.0), 0.0, 1.0),
        )
        .1;
        let expected: Vec<_> = selected
            .map(|index| {
                Choice::Open(RecentAction::Open(
                    paths[index].clone(),
                    RecentKind::File,
                    OpenTarget::Tab,
                ))
            })
            .into_iter()
            .collect();
        assert_eq!(choices, expected, "{query}");
    }
}

#[test]
fn recent_file_icons_follow_the_setting_without_changing_folder_or_command_rows() {
    for enabled in [false, true] {
        for folders in [false, true] {
            let context = crate::fonts::test_context();
            context.enable_accesskit();
            context.global_style_mut(crate::chrome::style);
            crate::tab_icons::set(&context, enabled);
            let files = [PathBuf::from("C:/media/first.png")];
            let sources = OpenSources {
                files: &files,
                folders: &files,
                ..Default::default()
            };
            let mut palette = CommandPalette::default();
            palette.open_files(folders);
            let mut output = egui::FullOutput::default();
            for _ in 0..4 {
                output = open_frame_at(
                    &context,
                    &mut palette,
                    sources,
                    vec![],
                    (egui::vec2(600.0, 400.0), 0.0, 1.0),
                )
                .0;
            }
            let text = picker_text(&output, "first.png").expect("file label").0;
            let icon = output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh)
                    if (mesh.calc_bounds().center().y
                        - (text.pos.y + text.galley.size().y * 0.5))
                        .abs()
                        < 2.0 =>
                {
                    Some(mesh)
                }
                _ => None,
            });
            assert_eq!(icon.is_some(), enabled && !folders);
            if let Some(icon) = icon {
                assert!(icon.calc_bounds().right() < text.pos.x);
                let row = output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("test fixture")
                    .nodes
                    .iter()
                    .find(|(_, node)| {
                        node.role() == egui::accesskit::Role::Button
                            && node
                                .label()
                                .is_some_and(|label| label.contains("first.png"))
                    })
                    .and_then(|(_, node)| node.bounds())
                    .expect("file row");
                assert!(
                    (icon.calc_bounds().left() - row.x0 as f32 - 6.0).abs() < 1.0,
                    "icon shares the ordinary label inset"
                );
                let format = text.galley.job.sections[0].format.color;
                let color =
                    text.override_text_color
                        .unwrap_or(if format == egui::Color32::PLACEHOLDER {
                            text.fallback_color
                        } else {
                            format
                        });
                assert!(icon.vertices.iter().all(|vertex| vertex.color == color));
            }
        }
    }
}

#[test]
fn only_the_enter_target_has_selected_text_while_another_row_is_hovered() {
    for mode in 0..3 {
        let context = crate::fonts::test_context();
        context.enable_accesskit();
        context.global_style_mut(|style| {
            crate::chrome::style(style);
            style.animation_time = 0.0;
            style.interaction.tooltip_delay = 60.0;
        });
        let paths = vec![
            PathBuf::from("C:/media/first.png"),
            PathBuf::from("C:/media/second.png"),
        ];
        let mut palette = CommandPalette::default();
        if mode != 0 {
            palette.open_files(mode == 2);
        }
        let sources = OpenSources {
            files: &paths,
            folders: &paths,
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
        for _ in 0..20 {
            frame(&mut palette, vec![]);
        }
        let selected = if mode == 0 { "Open file" } else { "first.png" };
        let other = if mode == 0 {
            "Toggle fullscreen"
        } else {
            "second.png"
        };
        let output = frame(&mut palette, vec![]).0;
        let point = picker_text(&output, other).expect("other row").0.pos + egui::vec2(10.0, 5.0);
        for _ in 0..3 {
            frame(&mut palette, vec![egui::Event::PointerMoved(point)]);
        }
        let output = frame(&mut palette, vec![egui::Event::PointerMoved(point)]).0;
        let color = |label| {
            let shape = picker_text(&output, label).expect("row label").0;
            let color = shape.galley.job.sections[0].format.color;
            shape
                .override_text_color
                .unwrap_or(if color == egui::Color32::PLACEHOLDER {
                    shape.fallback_color
                } else {
                    color
                })
        };
        let selected_color = color(selected);
        let hovered_color = color(other);
        assert_eq!(selected_color, crate::chrome::FOREGROUND);
        assert_eq!(hovered_color, crate::chrome::MUTED);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.rect.contains(point) && rect.fill == crate::chrome::HOVER)));
        let expected = if mode == 0 {
            Choice::Command(CommandId::OpenFile)
        } else {
            Choice::Open(RecentAction::Open(
                paths[0].clone(),
                if mode == 2 {
                    RecentKind::Folder
                } else {
                    RecentKind::File
                },
                OpenTarget::Tab,
            ))
        };
        assert_eq!(
            frame(
                &mut palette,
                vec![key(egui::Key::Enter, egui::Modifiers::NONE)]
            )
            .1,
            [expected]
        );
    }
}

#[test]
fn current_prefixes_exclude_history_and_stale_scopes_and_keep_open_modifiers() {
    use towavue_runtime_windows::{FileSearchRequest, FileSearchResult, FileSearchScope};
    for (prefix, scope, kind) in [
        ('@', FileSearchScope::CurrentFiles, RecentKind::File),
        (':', FileSearchScope::CurrentFolders, RecentKind::Folder),
    ] {
        let context = crate::fonts::test_context();
        let mut palette = CommandPalette::default();
        palette.open_files(true);
        palette.query = prefix.to_string();
        assert_eq!(palette.file_query(), Some(String::new()));
        let history = vec![PathBuf::from("C:/elsewhere/history.png")];
        let mut result = FileSearchResult {
            request: FileSearchRequest {
                root: "C:/media".into(),
                query: String::new(),
                scope: FileSearchScope::RecursiveFiles,
            },
            paths: vec![PathBuf::from("C:/media/current.png")],
            matches: 1,
            skipped: 0,
            error: None,
        };
        let frame = |palette: &mut CommandPalette, result: &FileSearchResult, events| {
            open_frame_at(
                &context,
                palette,
                OpenSources {
                    files: &history,
                    folders: &history,
                    search: Some(result),
                    ..Default::default()
                },
                events,
                (egui::vec2(600.0, 400.0), 0.0, 1.0),
            )
        };
        frame(&mut palette, &result, vec![]);
        assert!(
            palette.selected_path.is_none(),
            "scope transition must not show stale rows"
        );
        result.request.scope = scope;
        for _ in 0..3 {
            frame(&mut palette, &result, vec![]);
        }
        assert_eq!(palette.selected_path.as_ref(), result.paths.first());
        for (modifiers, target) in [
            (egui::Modifiers::NONE, OpenTarget::Tab),
            (egui::Modifiers::CTRL, OpenTarget::Window),
            (egui::Modifiers::ALT, OpenTarget::Replace),
        ] {
            assert_eq!(
                frame(
                    &mut palette,
                    &result,
                    vec![key(egui::Key::Enter, modifiers)]
                )
                .1,
                [Choice::Open(RecentAction::Open(
                    result.paths[0].clone(),
                    kind,
                    target
                ))]
            );
        }
    }
}

#[test]
fn japanese_file_search_preserves_paths_modifiers_and_remove_targets() {
    for density in [1.0, 1.25, 2.0] {
        for folders in [false, true] {
            for size in [egui::vec2(600.0, 400.0), egui::vec2(240.0, 180.0)] {
                let context = crate::localization::test_ui::japanese_context(density);
                let paths = [
                    PathBuf::from("C:/media/日本語{original}.png"),
                    PathBuf::from("C:/media/second.png"),
                ];
                let sources = OpenSources {
                    files: &paths,
                    folders: &paths,
                    ..Default::default()
                };
                let mut palette = CommandPalette::default();
                palette.open_files(folders);
                let frame = |palette: &mut CommandPalette, events| {
                    open_frame_at(&context, palette, sources, events, (size, 0.0, density))
                };
                for _ in 0..4 {
                    assert!(frame(&mut palette, vec![]).1.is_empty());
                }
                let output = frame(&mut palette, vec![]).0;
                let search_label = if folders {
                    "最近開いたフォルダーを検索"
                } else {
                    "ファイルを検索"
                };
                let tree = output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("tree");
                let query = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some(search_label))
                    .expect("localized query");
                assert_eq!(
                    query.0,
                    egui::Id::new("command-palette-query").accesskit_id()
                );
                let group = if folders {
                    "フォルダー"
                } else {
                    "最近開いた項目"
                };
                let (text, _) = picker_text(&output, group).expect("translated inline group");
                if size.x >= 600.0 {
                    assert!(
                        !text.galley.elided,
                        "{group} at {size:?}, density {density}"
                    );
                }
                // Compact rows intentionally shorten secondary groups before filenames.
                // The complete target remains available to accessibility and hover help.
                assert!(tree.nodes.iter().any(|(_, node)| {
                    node.label() == Some(paths[0].to_string_lossy().as_ref())
                }));
                let remove_label = format!("最近開いた履歴から削除: {}", paths[0].display());
                let remove = button_rect(&output, &remove_label);
                assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(remove));
                assert!(text.pos.x + text.galley.size().x <= remove.left());
                for (modifiers, target) in [
                    (egui::Modifiers::CTRL, OpenTarget::Window),
                    (egui::Modifiers::ALT, OpenTarget::Replace),
                ] {
                    assert_eq!(
                        frame(&mut palette, vec![key(egui::Key::Enter, modifiers)]).1,
                        [Choice::Open(RecentAction::Open(
                            paths[0].clone(),
                            if folders {
                                RecentKind::Folder
                            } else {
                                RecentKind::File
                            },
                            target,
                        ))]
                    );
                }
                let output = frame(&mut palette, vec![]).0;
                let action = crate::localization::test_ui::action(&output, &remove_label, None);
                assert_eq!(
                    frame(&mut palette, vec![action]).1,
                    [Choice::Open(RecentAction::Remove(
                        paths[0].clone(),
                        if folders {
                            RecentKind::Folder
                        } else {
                            RecentKind::File
                        },
                    ))]
                );
                assert_eq!(palette.selected_path.as_ref(), None);
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
                        OpenTarget::Replace,
                    ))]
                );
            }
        }
    }
}

#[test]
fn japanese_command_search_displays_translated_result_and_executes_original_id() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        if !crate::fonts::install(&context) {
            eprintln!(
                "SKIP Japanese glyph qualification: no installed Japanese UI font; command search remains checked"
            );
        }
        context.enable_accesskit();
        context.global_style_mut(crate::chrome::style);
        crate::localization::set_language(&context, towavue_core::localization::Language::Japanese);
        let mut palette = CommandPalette {
            query: ">ファイルを開く".into(),
            ..Default::default()
        };
        let frame = |palette: &mut CommandPalette, events| {
            open_frame_at(
                &context,
                palette,
                OpenSources::default(),
                events,
                (egui::vec2(600.0, 400.0), 0.0, density),
            )
        };
        for _ in 0..4 {
            assert!(frame(&mut palette, vec![]).1.is_empty());
        }
        let output = frame(&mut palette, vec![]).0;
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text().contains("ファイルを開く") && !text.galley.elided)));
        assert!(button_rect(&output, "ファイルを開く").width() > 0.0);
        assert_eq!(
            frame(
                &mut palette,
                vec![key(egui::Key::Enter, egui::Modifiers::NONE)]
            )
            .1,
            [Choice::Command(CommandId::OpenFile)]
        );
    }
}

fn button_rect(output: &egui::FullOutput, label: &str) -> egui::Rect {
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
    egui::Rect::from_min_max(
        egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
        egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
    )
}

fn button_position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    button_rect(output, label).center()
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
            scope: towavue_runtime_windows::FileSearchScope::RecursiveFiles,
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
                            label.galley.size().x
                                > (body.width()
                                    - if folders {
                                        0.0
                                    } else {
                                        crate::chrome::TAB_AUDIO_WIDTH
                                    })
                                    * 0.6,
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
                                && painted.rect.contains(point)
                                && painted.stroke == egui::Stroke::NONE)),
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

#[test]
fn command_history_returns_hidden_removal_width_and_keeps_removal_independent() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.enable_accesskit();
        context.global_style_mut(|style| {
            style.animation_time = 0.0;
            style.scroll_animation = egui::style::ScrollAnimation::none();
            style.interaction.tooltip_delay = 60.0;
        });
        let mut palette = CommandPalette::default();
        let history = [CommandId::OpenFile, CommandId::OpenFolder];
        let sources = OpenSources {
            commands: &history,
            ..Default::default()
        };
        let render = |palette: &mut CommandPalette, events| {
            open_frame_at(
                &context,
                palette,
                sources,
                events,
                (egui::vec2(600.0, 400.0), 0.0, density),
            )
        };
        let mut output = egui::FullOutput::default();
        for _ in 0..5 {
            output = render(&mut palette, vec![]).0;
        }
        let title = command_definitions()
            .iter()
            .find(|definition| definition.id == CommandId::OpenFolder)
            .expect("folder command")
            .title;
        let bounds = |output: &egui::FullOutput| {
            let tree = output
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("tree");
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(title))
                .expect("row")
                .1
                .bounds()
                .expect("bounds");
            egui::Rect::from_min_max(
                egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
            )
        };
        let idle = bounds(&output);
        render(&mut palette, vec![egui::Event::PointerMoved(idle.center())]);
        output = render(&mut palette, vec![]).0;
        let hovered = bounds(&output);
        assert!((idle.width() - hovered.width() - 44.0).abs() <= 1.0 / density);
        let gear = button_rect(&output, &format!("Configure keybinding: {title}"));
        let remove = button_rect(&output, &format!("Remove {title} from Recently Used"));
        assert_eq!(gear.size(), egui::Vec2::splat(20.0));
        assert_eq!(remove.size(), egui::Vec2::splat(20.0));
        let tree = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("tree");
        let bar = tree
            .nodes
            .iter()
            .find(|(_, node)| node.role() == egui::accesskit::Role::ScrollBar)
            .expect("scrollbar")
            .1
            .bounds()
            .expect("scrollbar bounds");
        assert!(
            f64::from(remove.right()) > bar.x0,
            "actions extend underneath the scrollbar at {density}: {remove:?}, {bar:?}"
        );
        assert!((gear.left() - hovered.right() - 2.0).abs() < 0.01);
        assert!((remove.left() - gear.right() - 2.0).abs() < 0.01);
        let shortcut =
            crate::shortcuts::defaults().label(CommandId::OpenFolder, CommandContext::default());
        let (text, _) = picker_text(&output, &shortcut).expect("shortcut label");
        assert!((gear.left() - text.pos.x - text.galley.size().x - 2.0).abs() <= 1.0 / density);
        // The exposed portion still invokes removal, before the bar scrolls it away.
        let point = egui::pos2(remove.left() + 2.0, remove.center().y);
        render(&mut palette, vec![egui::Event::PointerMoved(point)]);
        let click = |pressed| egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        render(&mut palette, vec![click(true)]);
        let choices = render(&mut palette, vec![click(false)]).1;
        assert_eq!(
            choices,
            [Choice::RemoveCommand(CommandId::OpenFolder)],
            "exposed action at {density}: {remove:?}, {bar:?}"
        );
        let overlap = egui::pos2((bar.x0 + bar.x1) as f32 * 0.5, remove.center().y);
        render(&mut palette, vec![egui::Event::PointerMoved(overlap)]);
        for pressed in [true, false] {
            let choices = render(
                &mut palette,
                vec![egui::Event::PointerButton {
                    pos: overlap,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
            )
            .1;
            assert!(choices.is_empty(), "scrollbar owns clicks over row actions");
        }
    }
}

#[test]
fn palette_gear_configures_enabled_and_context_disabled_commands_without_execution() {
    for density in [1.0, 1.25, 2.0] {
        for command in [CommandId::OpenFile, CommandId::TogglePause] {
            let context = crate::fonts::test_context();
            context.enable_accesskit();
            context.global_style_mut(|style| {
                crate::chrome::style(style);
                style.animation_time = 0.0;
                style.interaction.tooltip_delay = 60.0;
            });
            let definition = command_definitions()
                .iter()
                .find(|item| item.id == command)
                .expect("command");
            let mut palette = CommandPalette {
                query: format!(">{}", definition.title),
                ..Default::default()
            };
            let frame = |palette: &mut CommandPalette, events| {
                open_frame_at(
                    &context,
                    palette,
                    OpenSources::default(),
                    events,
                    (egui::vec2(600.0, 400.0), 0.0, density),
                )
            };
            let mut output = egui::FullOutput::default();
            for _ in 0..4 {
                output = frame(&mut palette, vec![]).0;
            }
            let body = button_rect(&output, definition.title);
            frame(&mut palette, vec![egui::Event::PointerMoved(body.center())]);
            output = frame(&mut palette, vec![]).0;
            let label = format!("Configure keybinding: {}", definition.title);
            let gear = button_rect(&output, &label);
            assert_eq!(gear.size(), egui::Vec2::splat(20.0));
            assert!(
                (gear.left() - button_rect(&output, definition.title).right() - 2.0).abs() < 0.01
            );
            let tree = output
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("tree");
            let (_, node) = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(&label))
                .expect("gear");
            assert!(
                !node.is_disabled(),
                "context-disabled commands still support editing their binding"
            );
            assert!(!tree.nodes.iter().any(|(_, node)| {
                node.label()
                    .is_some_and(|label| label.starts_with("Remove "))
            }));
            frame(&mut palette, vec![egui::Event::PointerMoved(gear.center())]);
            let click = |pressed| egui::Event::PointerButton {
                pos: gear.center(),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            assert!(frame(&mut palette, vec![click(true)]).1.is_empty());
            assert_eq!(
                frame(&mut palette, vec![click(false)]).1,
                [Choice::Configure(command)]
            );
        }
    }
}
