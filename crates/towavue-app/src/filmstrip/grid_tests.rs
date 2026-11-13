use super::*;

fn frame(
    strip: &mut Filmstrip,
    context: &Context,
    snapshot: &FolderSnapshot,
    time: f64,
    events: Vec<egui::Event>,
) -> Vec<UiAction> {
    let mut actions = Vec::new();
    let _ = context.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(960.0, 576.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |_| {
            strip.show_with_header(
                context,
                context.content_rect(),
                Some(snapshot),
                Some(&snapshot.items[0].path),
                true,
                &mut actions,
                0.0,
                context.content_rect(),
            )
        },
    );
    actions
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn grid_matches_gallery_and_keeps_keyboard_target_through_scroll_and_tab_restore() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::grid_tests::grid_matches_gallery_and_keeps_keyboard_target_through_scroll_and_tab_restore",
    ) else {
        return;
    };
    let snapshot = FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![]),
        folder_path: root.clone(),
        items: (0..100)
            .map(|index| towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(vec![]),
                path: root.join(format!("item-{index}.png")),
                kind: MediaKind::Image,
            })
            .collect(),
        sort_columns: vec![],
        source: towavue_core::FolderSnapshotSource::NaturalNameFallback,
        generation: 1,
        captured_at: std::time::SystemTime::now(),
    };
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.set_pixels_per_point(density);
        context.global_style_mut(crate::chrome::style);
        let mut strip = Filmstrip::new(
            PreviewCache::new(root.join(format!("cache-{density}"))).expect("cache"),
            || {},
        )
        .expect("strip");
        let texture = context.load_texture(
            "grid",
            egui::ColorImage::filled([80, 160], Color32::WHITE),
            Default::default(),
        );
        strip
            .previews
            .insert(snapshot.items[0].path.clone(), Ok((texture, None)));
        for step in 0..3 {
            frame(&mut strip, &context, &snapshot, step as f64 / 60.0, vec![]);
        }
        let geometry = strip.layout.grid.expect("production grid");
        assert_eq!(geometry.columns, 4);
        let card = |strip: &Filmstrip, index| {
            let id = strip
                .card_paths
                .iter()
                .find(|(_, _, i)| *i == index)
                .expect("card")
                .0;
            context.read_response(id).expect("response").rect
        };
        let first = card(&strip, 0);
        assert!((first.size().length() - GALLERY_DIAMETER).abs() < 0.001);
        assert!((first.center().x - 204.0).abs() < 0.001);
        assert!((first.center().y - 116.0).abs() < 0.001);
        frame(
            &mut strip,
            &context,
            &snapshot,
            0.1,
            vec![egui::Event::PointerMoved(first.center())],
        );
        frame(&mut strip, &context, &snapshot, 0.5, vec![]);
        assert_eq!(card(&strip, 0), first, "hover never zooms the frame");
        frame(
            &mut strip,
            &context,
            &snapshot,
            0.6,
            vec![key(egui::Key::ArrowDown)],
        );
        assert_eq!(strip.selection.as_ref(), Some(&snapshot.items[4].path));
        let actions = frame(
            &mut strip,
            &context,
            &snapshot,
            0.62,
            (0..12)
                .map(|_| key(egui::Key::ArrowDown))
                .chain([key(egui::Key::Enter)])
                .collect(),
        );
        assert!(actions.iter().any(|action| matches!(action, UiAction::OpenFilmstripMedia(path, false) if path == &snapshot.items[52].path)), "rapid navigation opens its offscreen target");
        assert!(strip.card_paths.len() <= VISIBLE_PREVIEW_LIMIT);
        let destination = strip.layout.center_offset(52);
        assert!(strip.scroll_offset < destination);
        let view = strip.take_view();
        assert_eq!(
            view.offset, destination,
            "tab handoff stores the pending destination"
        );
        strip.restore_view(view);
        frame(&mut strip, &context, &snapshot, 1.0, vec![]);
        assert!((strip.scroll_offset - destination).abs() < 0.01);
        assert_eq!(strip.selection.as_ref(), Some(&snapshot.items[52].path));
        let before = strip.scroll_offset;
        let mut refreshed = snapshot.clone();
        refreshed.generation += 1;
        frame(&mut strip, &context, &refreshed, 1.1, vec![]);
        assert_eq!(
            strip.scroll_offset, before,
            "folder refresh does not recenter"
        );
        strip.reveal_current();
        frame(&mut strip, &context, &snapshot, 1.2, vec![]);
        assert!(strip.scroll_offset > 0.0 && strip.scroll_offset < before);
        for step in 0..60 {
            frame(
                &mut strip,
                &context,
                &snapshot,
                1.3 + step as f64 / 60.0,
                vec![],
            );
        }
        assert_eq!(strip.scroll_offset, 0.0);
    }
}

#[test]
fn grid_cells_are_stable_bounded_and_share_gallery_geometry() {
    for width in [220.0, 480.0, 960.0, 3840.0, 100_000.0] {
        let screen = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 720.0));
        let grid = grid::Grid::new(screen, screen.shrink(8.0));
        let available = (width - 2.0 * gallery_side_inset(width)).max(1.0);
        assert_eq!(
            (grid.columns, grid.cell),
            grid::gallery_cells(available, 704.0)
        );
        for offset in [0.0, 500.0, 50_000.0] {
            let viewport = Rect::from_min_size(egui::pos2(0.0, offset), egui::vec2(width, 704.0));
            assert!(grid.visible(viewport, 50_000).len() <= VISIBLE_PREVIEW_LIMIT);
        }
        let a = grid.rect(0, egui::Pos2::ZERO, egui::vec2(1.0, 20.0));
        let b = grid.rect(0, egui::Pos2::ZERO, egui::vec2(20.0, 1.0));
        assert!(a.center().distance(b.center()) < 0.001);
        let next = grid.rect(grid.columns, egui::Pos2::ZERO, egui::vec2(1.0, 20.0));
        assert!(next.top() - a.bottom() >= GALLERY_GAP - 0.001);
    }
}
