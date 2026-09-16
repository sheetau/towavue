use super::*;

#[test]
fn seek_delta_is_white_inline_with_clock_and_does_not_replace_path_or_controls() {
    let Some(root) = tests::isolated_test_root(
        "seek_notice_tests::seek_delta_is_white_inline_with_clock_and_does_not_replace_path_or_controls",
    ) else {
        return;
    };
    for kind in [MediaKind::Audio, MediaKind::Video] {
        for density in [1.0, 1.25, 2.0] {
            for width in [240.0, 340.0, 480.0, 800.0] {
                let context = fonts::test_context();
                context.set_pixels_per_point(density);
                let mut app = Application::new(None, |_| {}).expect("app");
                let path = root.join("clip.mkv");
                app.tabs.open_new(path.clone(), kind);
                app.path = Some(path);
                app.media_kind = Some(kind);
                app.state = PlaybackState::Paused;
                app.clock = Some(PlaybackClock::paused(
                    media_time(Duration::from_secs(569)),
                    1.0,
                ));
                app.media_duration = Some(Duration::from_secs(2177));
                app.relative_seek_notice = Some((Instant::now(), 15_000_000_000));
                let frame = |app: &mut Application<_>| {
                    context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 300.0),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            app.draw_status_bar(ui, &mut Vec::new(), &mut Vec::new());
                        },
                    )
                };
                frame(&mut app);
                frame(&mut app);
                let output = frame(&mut app);
                let texts: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| {
                        if let egui::Shape::Text(text) = &shape.shape {
                            Some(text)
                        } else {
                            None
                        }
                    })
                    .collect();
                // The 340-point screen still has the bar's inner margins.
                let expected = if width <= 340.0 {
                    "09:29 +15s"
                } else {
                    "09:29 +15s / 36:17"
                };
                let clock = texts
                    .iter()
                    .find(|text| text.galley.text() == expected)
                    .unwrap_or_else(|| {
                        panic!(
                            "inline clock at {kind:?}/{width}/{density}: {:?}",
                            texts
                                .iter()
                                .map(|text| text.galley.text())
                                .collect::<Vec<_>>()
                        )
                    });
                assert_eq!(clock.galley.job.sections[0].format.color, chrome::MUTED);
                assert_eq!(
                    &clock.galley.job.text[clock.galley.job.sections[1].byte_range.start.0
                        ..clock.galley.job.sections[1].byte_range.end.0],
                    " +15s"
                );
                assert_eq!(
                    clock.galley.job.sections[1].format.color,
                    chrome::FOREGROUND
                );
                if width > 340.0 {
                    assert_eq!(clock.galley.job.sections[2].format.color, chrome::MUTED);
                }
                let volume = texts
                    .iter()
                    .find(|text| text.galley.text() == "100%")
                    .expect("volume");
                assert!(
                    volume.pos.x >= 0.0 && volume.pos.x + volume.galley.size().x <= width,
                    "volume stays visible at {width}/{density}: {:?}",
                    volume.pos
                );
                assert!(texts.iter().all(|text| text.galley.text() != "+15s"));
                if width >= 480.0 {
                    assert!(
                        texts
                            .iter()
                            .any(|text| text.galley.text().ends_with("\\clip.mkv")),
                        "path remains visible"
                    );
                }
                assert!(app.status_notice().is_none());
                app.relative_seek_notice = None;
                let cleared = frame(&mut app);
                assert!(cleared.shapes.iter().all(|shape| !matches!(&shape.shape,
                    egui::Shape::Text(text) if text.galley.text().contains("+15s"))));
            }
        }
    }
}

#[test]
fn seek_notice_owns_an_idle_deadline_and_resets_on_expiry_replacement_and_media_changes() {
    let Some(root) = tests::isolated_test_root(
        "seek_notice_tests::seek_notice_owns_an_idle_deadline_and_resets_on_expiry_replacement_and_media_changes",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    let now = Instant::now();
    app.relative_seek_notice = Some((now, -1_250_000_000));
    assert_eq!(app.relative_seek_text().as_deref(), Some("-1.25s"));
    assert_eq!(app.idle_wakeup(now), Some(now + STATUS_MESSAGE_DURATION));
    app.relative_seek_notice = Some((now - STATUS_MESSAGE_DURATION, 5_000_000_000));
    assert!(app.relative_seek_text().is_none());
    app.schedule();
    assert!(app.relative_seek_notice.is_none());
    assert_eq!(app.idle_wakeup(Instant::now()), None);
    app.relative_seek_notice = Some((now, 5_000_000_000));
    app.set_status("A different operation".into());
    assert!(app.relative_seek_text().is_none());
    assert_eq!(
        app.status_notice().as_deref(),
        Some("A different operation")
    );
    app.relative_seek_notice = Some((now, 5_000_000_000));
    app.seek_to(MediaTime::ZERO);
    assert!(app.relative_seek_notice.is_none());
    app.relative_seek_notice = Some((now, 5_000_000_000));
    app.clear_active_media();
    assert!(app.relative_seek_notice.is_none());
    app.relative_seek_notice = Some((now, 5_000_000_000));
    app.load_path(root.join("missing.mkv"), MediaKind::Video);
    assert!(app.relative_seek_notice.is_none());

    app.clear_active_media();
    let path = root.join("retained.mkv");
    let tab = app.tabs.open_new(path.clone(), MediaKind::Video);
    app.path = Some(path);
    app.displayed_tab = Some(tab);
    app.media_kind = Some(MediaKind::Video);
    app.relative_seek_notice = Some((Instant::now(), 5_000_000_000));
    let gallery = app.tabs.gallery().expect("Gallery");
    app.activate_tab(gallery);
    assert!(app.relative_seek_notice.is_none());
    app.activate_tab(tab);
    assert!(
        app.relative_seek_notice.is_none(),
        "returning to a retained tab must not revive a seek notice"
    );
    assert!(
        app.status_message
            .as_ref()
            .is_none_or(|(text, _)| !text.contains("+5s"))
    );
}
