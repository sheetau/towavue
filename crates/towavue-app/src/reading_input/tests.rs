use super::*;

#[test]
fn entering_previews_direction_without_adjusting_counts() {
    for density in [1.0, 1.25, 2.0] {
        let mut drag = ReadingDrag::new(ReadingSettings::default(), false, density);
        for (delta, expected) in [
            ((2.0, 0.0), None),
            ((-26.0, 0.0), Some(true)),
            ((72.0, 0.0), Some(false)),
            ((0.0, 100.0), None),
            ((0.0, -100.0), Some(false)),
        ] {
            drag.motion((delta.0 * density, delta.1 * density));
            assert_eq!(drag.direction, expected);
            assert_eq!(
                (drag.settings.page_count, drag.settings.first_page_count),
                (2, 2)
            );
            assert_eq!(drag.settings.reversed, expected.unwrap_or(false));
            assert_eq!(drag.before, ReadingSettings::default());
        }
    }
}

#[test]
fn a_single_adjustment_switches_both_axes_without_residual_jumps() {
    for density in [1.0, 1.25, 2.0] {
        let mut drag = ReadingDrag::new(ReadingSettings::default(), true, density);
        let move_by = |drag: &mut ReadingDrag, x, y| drag.motion((x * density, y * density));
        move_by(&mut drag, 0.0, -47.0);
        assert_eq!(
            (drag.settings.page_count, drag.settings.first_page_count),
            (3, 3)
        );
        // A small orthogonal wobble cannot select another axis.
        for x in [3.0, -3.0, 3.0, -3.0] {
            move_by(&mut drag, x, 0.0);
            assert_eq!(drag.axis, Some(true));
        }
        move_by(&mut drag, -48.0, 0.0);
        assert_eq!(drag.axis, Some(false));
        assert_eq!(
            (drag.settings.page_count, drag.settings.first_page_count),
            (3, 3)
        );
        move_by(&mut drag, -23.0, 0.0);
        assert_eq!(drag.settings.first_page_count, 3);
        move_by(&mut drag, -1.0, 0.0);
        assert_eq!(drag.settings.first_page_count, 2);
        move_by(&mut drag, 0.0, -48.0);
        assert_eq!(
            drag.axis,
            Some(false),
            "recent horizontal motion still dominates"
        );
        move_by(&mut drag, 0.0, -48.0);
        assert_eq!(drag.axis, Some(true));
        assert_eq!(
            (drag.settings.page_count, drag.settings.first_page_count),
            (3, 2)
        );
        move_by(&mut drag, 0.0, -24.0);
        assert_eq!(
            (drag.settings.page_count, drag.settings.first_page_count),
            (4, 2)
        );
        assert_eq!(drag.before, ReadingSettings::default());
    }
}

#[test]
fn limits_and_partial_first_spreads_survive_reversal_in_the_same_axis() {
    for full in [false, true] {
        let settings = ReadingSettings {
            page_count: 5,
            first_page_count: if full { 5 } else { 2 },
            ..ReadingSettings::default()
        };
        let mut drag = ReadingDrag::new(settings, true, 1.0);
        drag.motion((0.0, 24_000.0));
        assert_eq!(
            (drag.settings.page_count, drag.settings.first_page_count),
            (2, 2)
        );
        drag.motion((0.0, -24.0));
        assert_eq!(
            (drag.settings.page_count, drag.settings.first_page_count),
            (3, if full { 3 } else { 2 })
        );
        drag.motion((0.0, -24_000.0));
        assert_eq!(drag.settings.page_count, 10);
        drag.motion((0.0, -23.0));
        drag.motion((0.0, -24.0));
        drag.motion((0.0, 24.0));
        assert_eq!(drag.settings.page_count, 9);
    }
}

#[test]
fn initial_jitter_diagonal_motion_and_switch_sign_changes_do_not_steal_an_axis() {
    let mut drag = ReadingDrag::new(ReadingSettings::default(), true, 1.0);
    drag.motion((1.0, 0.0));
    assert_eq!(drag.axis, None);
    drag.motion((0.0, -24.0));
    assert_eq!(drag.axis, Some(true));
    for _ in 0..10 {
        drag.motion((3.0, -3.0));
        assert_eq!(drag.axis, Some(true));
    }
    for x in [7.0, -7.0, 7.0, -7.0] {
        drag.motion((x, 0.0));
        assert_eq!(drag.axis, Some(true));
    }
    for _ in 0..8 {
        drag.motion((2.0, 0.0));
    }
    assert_eq!(drag.axis, Some(false), "sustained small events also switch");
}

#[test]
fn preview_commit_cancel_and_layout_changes_keep_the_current_shell_image() {
    use crate::*;
    let Some(root) = tests::isolated_test_root(
        "reading_input::tests::preview_commit_cancel_and_layout_changes_keep_the_current_shell_image",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for reversed in [false, true] {
            let mut app = Application::new(None, |_| {}).expect("app");
            let paths: Vec<_> = (0..11)
                .rev()
                .map(|i| root.join(format!("{i}.png")))
                .collect();
            let path = paths[5].clone();
            let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
            app.edits.insert(tab, EditHistory::default());
            let history = app.edits.clone();
            app.path = Some(path.clone());
            app.media_kind = Some(MediaKind::Image);
            app.folder_snapshot = Some(FolderSnapshot {
                folder_identity: towavue_core::ShellIdentity::new(vec![]),
                folder_path: root.clone(),
                items: paths
                    .iter()
                    .map(|path| towavue_core::FolderMediaItem {
                        identity: towavue_core::ShellIdentity::new(vec![]),
                        path: path.clone(),
                        kind: MediaKind::Image,
                    })
                    .collect(),
                sort_columns: vec![],
                source: FolderSnapshotSource::LiveExplorerView,
                generation: 1,
                captured_at: std::time::SystemTime::now(),
            });
            for cancel in [true, false] {
                let before = app.reading_settings;
                app.reading_drag = Some(ReadingDrag::new(before, false, density));
                app.move_reading_drag((if reversed { -24.0 } else { 24.0 } * density, 0.0));
                assert!(!app.reading_mode, "entry is a preview until release");
                assert_eq!(app.reading_settings, before);
                assert_eq!(
                    app.status_notice().as_deref(),
                    Some(if reversed {
                        "Reading left: release to enable"
                    } else {
                        "Reading right: release to enable"
                    })
                );
                assert!(app.finish_reading_drag(cancel));
                assert_eq!(app.reading_mode, !cancel);
                if cancel {
                    assert_eq!(app.reading_settings, before);
                }
            }
            assert_eq!(app.reading_settings.reversed, reversed);
            let before = app.reading_settings;
            for cancel in [true, false] {
                app.reading_drag = Some(ReadingDrag::new(app.reading_settings, true, density));
                for delta in [
                    (0.0, -48.0),
                    (-48.0, 0.0),
                    (-24.0, 0.0),
                    (0.0, -48.0),
                    (0.0, -48.0),
                    (0.0, -24.0),
                ] {
                    app.move_reading_drag((delta.0 * density, delta.1 * density));
                    let range = app.reading_settings.spread(5, paths.len());
                    let mut expected = vec![path.clone()];
                    expected.extend(paths[range].iter().filter(|p| **p != path).cloned());
                    assert_eq!(app.reading_request_paths(), expected);
                    assert_eq!(app.path.as_ref(), Some(&path));
                    assert_eq!(app.tabs.active_id(), Some(tab));
                    assert_eq!(app.edits, history);
                }
                app.finish_reading_drag(cancel);
                if cancel {
                    assert_eq!(app.reading_settings, before);
                } else {
                    assert_eq!(
                        (
                            app.reading_settings.page_count,
                            app.reading_settings.first_page_count
                        ),
                        (5, 3)
                    );
                }
                assert!(app.reading_mode && app.reading_cursor.is_none());
            }
            app.set_reading_layout(false, before);
            app.reading_drag = Some(ReadingDrag::new(before, false, density));
            app.move_reading_drag((0.0, -48.0 * density));
            app.finish_reading_drag(false);
            assert!(!app.reading_mode, "a vertical entry drag has no direction");
            assert_eq!(app.path.as_ref(), Some(&path));
        }
    }
}
