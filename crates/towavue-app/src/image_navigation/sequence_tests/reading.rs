use super::*;

fn stop_workers(app: &mut App) {
    app.image_loader.request(Vec::new());
    app.folder_order.request(None);
}

#[test]
fn new_image_tabs_start_outside_reading_and_restore_each_existing_owner() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::sequence_tests::reading::new_image_tabs_start_outside_reading_and_restore_each_existing_owner",
    ) else {
        return;
    };
    for dropped in [false, true] {
        let (mut app, _, paths) = fixture(&root);
        tab_transfer::tests::bitmap(&paths[1]);
        let original = app.tabs.active_id().expect("original tab");
        app.reading_mode = true;
        app.reading_settings.page_count = 4;
        app.reading_settings.first_page_count = 1;
        app.ensure_reading_focus();
        let settings = app.reading_settings;
        if dropped {
            app.open_dropped_path(paths[1].clone());
        } else {
            app.open_external(paths[1].clone(), true);
        }
        stop_workers(&mut app);
        let fresh = app.tabs.active_id().expect("new tab");
        assert_ne!(fresh, original);
        assert!(!app.reading_mode);
        assert!(app.retained_images[&original].reading_mode);
        app.activate_tab(original);
        stop_workers(&mut app);
        assert!(app.reading_mode);
        assert_eq!(app.reading_settings, settings);
        app.activate_tab(fresh);
        stop_workers(&mut app);
        assert!(!app.reading_mode);
    }
}

#[test]
fn reading_shortcuts_repeat_latest_spreads_and_jump_by_groups() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::sequence_tests::reading::reading_shortcuts_repeat_latest_spreads_and_jump_by_groups",
    ) else {
        return;
    };
    for reversed in [false, true] {
        let (mut app, _, paths) = fixture(&root);
        app.reading_mode = true;
        app.reading_settings = ReadingSettings {
            page_count: 3,
            first_page_count: 1,
            reversed,
            ..Default::default()
        };
        app.ensure_reading_focus();
        let tab = app.tabs.active_id();
        let forward = if reversed { "Left" } else { "Right" };
        app.process_shortcut(forward.parse().expect("press"));
        for _ in 0..4 {
            app.repeat_media_shortcut(forward.parse().expect("repeat"));
        }
        stop_workers(&mut app);
        assert_eq!(app.path.as_ref(), Some(&paths[13]));
        assert_eq!(app.tabs.active_id(), tab);
        assert!(app.reading_mode);
        assert_eq!(
            app.image_handoff.as_ref().expect("original held").path,
            paths[0]
        );
        assert!(app.image_sequence.steps.is_empty());
        app.release_image_repeats(&Key::ArrowRight, false);
        app.release_image_repeats(&Key::ArrowLeft, false);
        assert!(!app.coalesce_image_repeats(Instant::now() + Duration::from_secs(1)));
        assert_eq!(app.path.as_ref(), Some(&paths[13]));
        // Sequential aliases keep sequence order even for right-to-left reading.
        app.repeat_media_shortcut("Space".parse().expect("next"));
        assert_eq!(app.path.as_ref(), Some(&paths[16]));
        app.repeat_media_shortcut("Shift+Space".parse().expect("previous"));
        assert_eq!(app.path.as_ref(), Some(&paths[13]));
        for (next, previous) in [("PageDown", "PageUp"), ("Space", "Shift+Space")] {
            app.process_shortcut(next.parse().expect("next group alias"));
            assert_eq!(app.path.as_ref(), Some(&paths[16]));
            app.process_shortcut(previous.parse().expect("previous group alias"));
            assert_eq!(app.path.as_ref(), Some(&paths[13]));
        }
        let directional_next = if reversed { "A" } else { "D" };
        let directional_previous = if reversed { "D" } else { "A" };
        app.repeat_media_shortcut(directional_next.parse().expect("directional next"));
        assert_eq!(app.path.as_ref(), Some(&paths[16]));
        app.repeat_media_shortcut(directional_previous.parse().expect("directional previous"));
        assert_eq!(app.path.as_ref(), Some(&paths[13]));
        app.process_shortcut("Ctrl+2".parse().expect("two groups forward"));
        assert_eq!(app.path.as_ref(), Some(&paths[19]));
        app.process_shortcut("Ctrl+Shift+3".parse().expect("three groups back"));
        assert_eq!(app.path.as_ref(), Some(&paths[10]));
        app.dispatch(CommandId::LastImage);
        assert_eq!(app.reading_focus_path(), Some(&paths[97]));
        app.jump_images(10);
        assert_eq!(app.reading_focus_path(), Some(&paths[97]));
        app.jump_images(-2);
        assert_eq!(app.path.as_ref(), Some(&paths[91]));
        app.dispatch(CommandId::FirstImage);
        assert_eq!(app.path.as_ref(), Some(&paths[0]));
        app.jump_images(-10);
        assert_eq!(app.path.as_ref(), Some(&paths[0]));
        app.reading_mode = false;
        app.jump_images(2);
        assert_eq!(
            app.path.as_ref(),
            Some(&paths[2]),
            "ordinary jumps count images"
        );
        stop_workers(&mut app);
    }
}

#[test]
fn reading_repeats_preserve_overlays_prefixes_custom_bindings_and_dirty_guards() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::sequence_tests::reading::reading_repeats_preserve_overlays_prefixes_custom_bindings_and_dirty_guards",
    ) else {
        return;
    };
    let (mut app, _, paths) = fixture(&root);
    app.reading_mode = true;
    for state in (0..6).filter(|&state| state != 2) {
        app.filmstrip_open = state == 0;
        app.palette_open = state == 1;
        app.image_edit_pending = state == 3;
        app.entered_shortcut = if state == 4 {
            vec!["Ctrl+K".parse().expect("prefix")]
        } else {
            vec![]
        };
        if state == 5 {
            app.edits
                .entry(app.tabs.active_id().expect("tab"))
                .or_default()
                .push(EditOperation::RotateClockwise, MediaKind::Image);
        }
        app.repeat_media_shortcut("Right".parse().expect("repeat"));
        assert_eq!(app.path.as_ref(), Some(&paths[0]));
    }
    app.edits.clear();
    app.shortcuts
        .set(CommandId::ReadingRight, "N".parse().expect("custom key"));
    app.shortcuts.set(
        CommandId::ToggleFullscreen,
        "Right".parse().expect("rebound toggle"),
    );
    app.repeat_media_shortcut("Right".parse().expect("repeat"));
    assert!(!app.fullscreen);
    assert_eq!(app.path.as_ref(), Some(&paths[0]));
    app.repeat_media_shortcut("N".parse().expect("custom repeat"));
    assert_eq!(app.path.as_ref(), Some(&paths[2]));
    stop_workers(&mut app);
}

#[test]
fn reading_seek_positions_reach_both_endpoints_and_select_matching_groups() {
    for total in 1..=35 {
        for pages in 2..=10 {
            for first in 1..=pages {
                let reading = Some(ReadingSettings {
                    page_count: pages,
                    first_page_count: first,
                    ..Default::default()
                });
                let mut starts = vec![0];
                let mut next = first;
                while next < total {
                    starts.push(next);
                    next += pages;
                }
                for (ordinal, start) in starts.iter().copied().enumerate() {
                    let ratio = if starts.len() == 1 {
                        1.0
                    } else {
                        ordinal as f32 / (starts.len() - 1) as f32
                    };
                    assert_eq!(image_seek_progress(reading, start, total), ratio);
                    assert_eq!(image_seek_target(reading, ratio, total), start);
                }
                assert_eq!(image_seek_progress(reading, total - 1, total), 1.0);
                assert_eq!(image_seek_target(reading, 0.0, total), 0);
                assert_eq!(
                    image_seek_target(reading, 1.0, total),
                    *starts.last().expect("last group")
                );
            }
        }
    }
    assert_eq!(image_seek_progress(None, 0, 1), 0.0);
    assert_eq!(image_seek_progress(None, 1, 3), 0.5);
    assert_eq!(image_seek_target(None, 1.0, 3), 2);
}

#[test]
fn main_and_hover_seek_bars_paint_full_progress_for_the_final_group() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::sequence_tests::reading::main_and_hover_seek_bars_paint_full_progress_for_the_final_group",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for (total, pages, first) in [(1, 2, 2), (2, 3, 2), (14, 3, 1), (15, 3, 2)] {
            for reversed in [false, true] {
                let (mut app, context, paths) = fixture(&root);
                app.folder_snapshot
                    .as_mut()
                    .expect("snapshot")
                    .items
                    .truncate(total);
                app.path = Some(paths[total - 1].clone());
                app.tabs
                    .active_mut()
                    .expect("tab")
                    .target
                    .set_current_path(paths[total - 1].clone(), MediaKind::Image);
                app.reading_mode = true;
                app.reading_settings = ReadingSettings {
                    page_count: pages,
                    first_page_count: first,
                    reversed,
                    ..Default::default()
                };
                app.ensure_reading_focus();
                context.set_pixels_per_point(density);
                for pass in 0..3 {
                    let output = context.run_ui(
                        egui::RawInput {
                            time: Some(pass as f64),
                            events: vec![egui::Event::PointerMoved(egui::pos2(10.0, 200.0))],
                            viewports: [(
                                egui::ViewportId::ROOT,
                                egui::ViewportInfo {
                                    native_pixels_per_point: Some(density),
                                    ..Default::default()
                                },
                            )]
                            .into_iter()
                            .collect(),
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(500.0, 300.0),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            app.draw_seek_bar(
                                &context,
                                egui::Rect::from_min_size(
                                    egui::pos2(0.0, 270.0),
                                    egui::vec2(500.0, 30.0),
                                ),
                                None,
                                &mut Vec::new(),
                            );
                            let position = app
                                .preview_folder(app.displayed_tab.expect("tab"), &paths[total - 1])
                                .expect("position");
                            position.show(
                                ui,
                                egui::Rect::from_min_size(
                                    egui::pos2(40.0, 40.0),
                                    egui::vec2(240.0, 80.0),
                                ),
                            );
                        },
                    );
                    if pass < 2 {
                        continue;
                    }
                    let widths: Vec<_> = output
                        .shapes
                        .iter()
                        .filter_map(|shape| match &shape.shape {
                            egui::Shape::Rect(rect) if rect.fill == chrome::FOREGROUND => {
                                Some(rect.rect.width())
                            }
                            _ => None,
                        })
                        .collect();
                    assert!(
                        widths.iter().any(|width| (*width - 500.0).abs() < 0.01),
                        "full main bar: density={density}, total={total}, reversed={reversed}, shapes={:?}",
                        output.shapes
                    );
                    assert!(
                        total == 1 || widths.iter().any(|width| (*width - 240.0).abs() < 0.01),
                        "full card bar: {widths:?}"
                    );
                }
            }
        }
    }
}
