use super::*;

#[test]
fn gallery_counts_combine_visit_dates_types_and_unknown_without_source_io() {
    let paths = ["C:/Media/a.png", "C:/Media/b.mp4", "C:/Media/c.wav"].map(PathBuf::from);
    let dates = [
        (paths[0].clone(), (2026, 10, 23)),
        (paths[1].clone(), (2026, 9, 1)),
    ]
    .into_iter()
    .collect();
    let mut counts = Counts::default();
    counts.gallery_dates(&dates);
    for (query, expected) in [
        ("dg:2026/10/23", [1, 1, 0, 0]),
        ("dg:2026 image:|video:", [2, 1, 1, 0]),
        ("dg:unknown", [1, 0, 0, 1]),
        ("!<dg:2026/10/23 image:>", [2, 0, 1, 1]),
        ("dg:2026-02-30", [0, 0, 0, 0]),
    ] {
        counts.update(&paths, query);
        assert_eq!(counts.values, expected, "{query}");
    }
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
fn search_opens_with_keyboard_keeps_nonempty_blur_and_collapses_on_escape_or_empty_blur() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.global_style_mut(crate::chrome::style);
        context.set_pixels_per_point(density);
        let id = Id::new("search-contract");
        let query = std::cell::RefCell::new(String::new());
        let frame = |events| {
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 100.0),
                    )),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ui| {
                    search(
                        ui,
                        Rect::from_min_size(egui::pos2(30.0, 10.0), egui::vec2(220.0, 26.0)),
                        id,
                        "Search Gallery",
                        &mut query.borrow_mut(),
                    );
                },
            )
        };
        let assert_border = |output: &egui::FullOutput| {
            let border = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Rect(border)
                        if border.fill == Color32::WHITE && border.rect.width() > 200.0 =>
                    {
                        Some(border.rect)
                    }
                    _ => None,
                })
                .expect("white input bottom border, including after blur");
            assert!((border.height() * density - 1.0).abs() < 0.001);
            assert!((border.bottom() - 36.0).abs() <= 0.5 / density);
        };
        frame(vec![]);
        context.memory_mut(|memory| memory.request_focus(id.with("button")));
        frame(vec![key(egui::Key::Enter)]);
        assert_border(&frame(vec![]));
        assert!(context.memory(|memory| memory.has_focus(id)));
        frame(vec![egui::Event::Text("日本語 photo".into())]);
        assert_eq!(&*query.borrow(), "日本語 photo");
        context.memory_mut(|memory| memory.surrender_focus(id));
        let output = frame(vec![]);
        assert_border(&output);
        let entered = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "日本語 photo" => Some(text),
                _ => None,
            })
            .expect("entered query");
        assert!(
            entered
                .galley
                .job
                .sections
                .iter()
                .all(|section| section.format.color == Color32::WHITE)
        );
        assert!(
            !output
                .shapes
                .iter()
                .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "Search"))
        );
        context.memory_mut(|memory| memory.request_focus(id));
        frame(vec![]); // Present the newly focused editor and its key ownership.
        frame(vec![key(egui::Key::Escape)]);
        assert!(query.borrow().is_empty());
        assert!(!context.memory(|memory| memory.has_focus(id)));
        let output = frame(vec![]);
        assert!(
            output
                .shapes
                .iter()
                .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "Search"))
        );
        context.memory_mut(|memory| memory.request_focus(id.with("button")));
        frame(vec![key(egui::Key::Enter)]);
        context.memory_mut(|memory| memory.surrender_focus(id));
        let output = frame(vec![]);
        assert!(
            output
                .shapes
                .iter()
                .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "Search"))
        );
    }
}

#[test]
fn filter_labels_share_one_row_with_superscripts_and_omit_absent_kinds() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.global_style_mut(crate::chrome::style);
        context.set_pixels_per_point(density);
        let selected = std::cell::Cell::new(None);
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            let values = [123, 100, 23, 0];
            let size = filter_size(ui, &values);
            let mut value = selected.get();
            filters(
                ui,
                Rect::from_min_size(egui::pos2(20.0, 20.0), size),
                &values,
                &mut value,
            );
            selected.set(value);
        });
        let text = |label: &str| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => Some(text),
                    _ => None,
                })
                .expect("label")
        };
        let right = text("All").pos.x + text("All").galley.size().x;
        for label in ["Image", "Video"] {
            let label = text(label);
            assert!((label.pos.y - text("All").pos.y).abs() <= 1.0 / density);
            assert!(label.pos.x > right);
        }
        let count = text("123");
        assert!(count.pos.x > right && count.pos.y < text("All").pos.y);
        assert!(
            count
                .galley
                .job
                .sections
                .iter()
                .all(|s| s.format.underline.is_empty())
        );
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text() == "Audio")));
        assert_eq!(
            visible_filters(&[12, 12, 0, 0]),
            [false, true, false, false]
        );
    }
}

#[test]
fn counts_follow_query_and_source_invalidation() {
    let mut counts = Counts::default();
    let paths = ["photo.png", "photo.mp4", "song.wav"].map(PathBuf::from);
    counts.update(&paths, "");
    assert_eq!(counts.values, [3, 1, 1, 1]);
    counts.update(&paths, "photo");
    assert_eq!(counts.values, [2, 1, 1, 0]);
    counts.invalidate();
    counts.update(&paths[..1], "photo");
    assert_eq!(counts.values, [1, 1, 0, 0]);
}
