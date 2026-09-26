use super::*;

#[test]
fn audio_buttons_are_visible_from_the_source_tab_without_a_dark_backdrop() {
    for density in [1.0, 1.25, 2.0] {
        let context = fonts::test_context();
        context.enable_accesskit();
        context.set_pixels_per_point(density);
        context.global_style_mut(chrome::style);
        let thumbnail = egui::Rect::from_min_size(egui::pos2(40.0, 40.0), egui::vec2(160.0, 40.0));
        let output = context.run_ui(
            egui::RawInput {
                events: vec![egui::Event::PointerMoved(egui::pos2(90.0, 15.0))],
                ..Default::default()
            },
            |ui| {
                assert!(
                    (Transport {
                        instance: 1,
                        kind: MediaKind::Audio,
                        state: PlaybackState::Paused,
                        position: MediaTime::ZERO,
                        duration: Some(media_time(Duration::from_secs(60))),
                        enabled: true,
                        previous: true,
                        next: true,
                    })
                    .show(ui, thumbnail)
                    .is_none()
                );
            },
        );
        let nodes = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("tree")
            .nodes;
        for label in ["Previous track", "Play", "Next track"] {
            let bounds = nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .expect("button visible before entering card")
                .1
                .bounds()
                .expect("bounds");
            assert!(bounds.y0 >= 40.0 && bounds.y1 <= 80.0);
        }
        assert!(
            !output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(rect) if rect.fill == Color32::from_black_alpha(160))),
            "audio controls have no thumbnail backdrop"
        );
    }
}

#[test]
fn card_seek_rejects_replacement_disabled_duration_and_interrupted_drags() {
    for scenario in 0..5 {
        let context = fonts::test_context();
        let thumbnail = egui::Rect::from_min_size(egui::pos2(40.0, 40.0), egui::vec2(240.0, 80.0));
        let point = thumbnail.center_bottom() - egui::vec2(0.0, 0.5);
        let button = |pressed| egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let frame = |changed: bool, events| {
            let mut actions = Vec::new();
            let _ = context.run_ui(
                egui::RawInput {
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ui| {
                    if let Some(action) = (Transport {
                        instance: if changed && scenario == 0 { 2 } else { 1 },
                        kind: MediaKind::Video,
                        state: PlaybackState::Paused,
                        position: MediaTime::ZERO,
                        duration: (!(changed && scenario == 2))
                            .then_some(media_time(Duration::from_secs(100))),
                        enabled: !(changed && scenario == 1),
                        previous: false,
                        next: false,
                    })
                    .show(ui, thumbnail)
                    {
                        actions.push(action);
                    }
                },
            );
            actions
        };
        frame(false, vec![egui::Event::PointerMoved(point)]);
        frame(false, vec![]);
        assert!(frame(false, vec![button(true)]).is_empty());
        let interrupt = match scenario {
            3 => vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
            4 => vec![egui::Event::WindowFocused(false)],
            _ => vec![],
        };
        assert!(frame(true, interrupt).is_empty());
        assert!(
            frame(true, vec![button(false)]).is_empty(),
            "cancelled case {scenario}"
        );
    }
}

#[test]
fn replacing_media_between_press_and_release_does_not_control_the_new_track() {
    for offset in [egui::Vec2::ZERO, egui::vec2(-100.0, -25.0)] {
        let context = fonts::test_context();
        let thumbnail = egui::Rect::from_min_size(egui::pos2(40.0, 40.0), egui::vec2(240.0, 80.0));
        let frame = |instance, pressed: Option<bool>| {
            let mut events = vec![egui::Event::PointerMoved(thumbnail.center() + offset)];
            if let Some(pressed) = pressed {
                events.push(egui::Event::PointerButton {
                    pos: thumbnail.center() + offset,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                });
            }
            let mut action = None;
            let _ = context.run_ui(
                egui::RawInput {
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ui| {
                    action = Transport {
                        instance,
                        kind: MediaKind::Audio,
                        state: PlaybackState::Paused,
                        position: MediaTime::ZERO,
                        duration: None,
                        enabled: true,
                        previous: false,
                        next: false,
                    }
                    .show(ui, thumbnail);
                },
            );
            action
        };
        frame(1, None);
        frame(1, None);
        assert!(frame(1, Some(true)).is_none());
        assert!(frame(2, Some(false)).is_none());
        frame(2, None);
        frame(2, Some(true));
        assert_eq!(
            frame(2, Some(false)),
            Some(Action::Command(CommandId::TogglePause))
        );
    }
}

#[test]
fn tab_card_bridge_transport_clicks_and_progress_keep_layout_and_ownership() {
    for density in [1.0, 1.25, 2.0] {
        for kind in [MediaKind::Audio, MediaKind::Video] {
            let context = fonts::test_context();
            context.set_pixels_per_point(density);
            let source =
                egui::Rect::from_min_size(egui::pos2(220.0, 20.0), egui::vec2(100.0, 24.0));
            let transport = Transport {
                instance: 1,
                kind,
                state: PlaybackState::Paused,
                position: media_time(Duration::from_secs(25)),
                duration: Some(media_time(Duration::from_secs(100))),
                enabled: true,
                previous: true,
                next: true,
            };
            let mut time = 0.0;
            let mut frame = |pointer,
                             pressed: Option<bool>,
                             enabled: bool,
                             dropped: bool,
                             draw: bool| {
                time += 0.01;
                let mut events = vec![egui::Event::PointerMoved(pointer)];
                if let Some(pressed) = pressed {
                    events.push(egui::Event::PointerButton {
                        pos: pointer,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    });
                }
                let mut result = None;
                let output = context.run_ui(
                    egui::RawInput {
                        time: Some(time),
                        focused: true,
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(600.0, 400.0),
                        )),
                        events,
                        hovered_files: if dropped {
                            vec![egui::HoveredFile {
                                path: Some("dropped.png".into()),
                                ..Default::default()
                            }]
                        } else {
                            vec![]
                        },
                        ..Default::default()
                    },
                    |ui| {
                        result = None;
                        if !draw {
                            return;
                        }
                        ui.add_enabled_ui(enabled, |ui| {
                            let response =
                                ui.interact(source, "tab".into(), egui::Sense::click_and_drag());
                            result = media_preview::Preview::tab(&response)
                                .show(|ui| {
                                    let thumbnail = ui
                                        .allocate_exact_size(
                                            egui::vec2(
                                                media_preview::SIZE.x,
                                                if kind == MediaKind::Audio { 40.0 } else { 80.0 },
                                            ),
                                            egui::Sense::hover(),
                                        )
                                        .0;
                                    let action = transport.show(ui, thumbnail);
                                    let caption =
                                        media_preview::caption(ui, |ui| ui.label("Caption").rect);
                                    (thumbnail, caption, action)
                                })
                                .map(|shown| (shown.response.rect, shown.inner));
                        });
                    },
                );
                (result, output)
            };
            for _ in 0..16 {
                frame(source.center(), None, true, false, true);
            }
            let (card, (thumbnail, caption, action)) =
                frame(source.center(), None, true, false, true)
                    .0
                    .expect("source hover");
            assert!(action.is_none());
            let gap = egui::pos2(source.center().x, (source.bottom() + card.top()) * 0.5);
            assert!(
                frame(gap, None, true, false, true).0.is_some(),
                "cross the four-point gap"
            );
            let (shown, output) = frame(thumbnail.center(), None, true, false, true);
            assert_eq!(
                shown.expect("card hover").1.1,
                caption,
                "overlay cannot move the caption"
            );
            let tracks: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect)
                        if rect.rect.height() <= 1.1 / density
                            && [chrome::FOREGROUND, chrome::BORDER].contains(&rect.fill) =>
                    {
                        Some(rect)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(tracks.len(), 2);
            assert!((tracks[0].rect.height() * density - 1.0).abs() < 0.001);
            assert!((tracks[0].rect.bottom() * density).fract().abs() < 0.001);
            assert!((tracks[1].rect.width() / tracks[0].rect.width() - 0.25).abs() < 0.001);
            for (offset, expected) in [
                (0.0, CommandId::TogglePause),
                (-28.0, CommandId::PreviousMedia),
                (28.0, CommandId::NextMedia),
            ] {
                if kind == MediaKind::Video && offset != 0.0 {
                    continue;
                }
                let point = thumbnail.center() + egui::vec2(offset, 0.0);
                frame(point, None, true, false, true);
                assert!(
                    frame(point, Some(true), true, false, true)
                        .0
                        .expect("held card")
                        .1
                        .2
                        .is_none()
                );
                assert_eq!(
                    frame(point, Some(false), true, false, true)
                        .0
                        .expect("released card")
                        .1
                        .2,
                    Some(Action::Command(expected))
                );
            }
            for point in [
                thumbnail.left_top() + egui::vec2(12.0, 12.0),
                thumbnail.right_top() + egui::vec2(-12.0, 12.0),
            ] {
                frame(point, None, true, false, true);
                assert!(
                    frame(point, Some(true), true, false, true)
                        .0
                        .expect("card press")
                        .1
                        .2
                        .is_none()
                );
                assert_eq!(
                    frame(point, Some(false), true, false, true)
                        .0
                        .expect("card release")
                        .1
                        .2,
                    Some(Action::Command(CommandId::TogglePause)),
                    "whole thumbnail playback"
                );
            }
            let bar = thumbnail.center_bottom() - egui::vec2(0.0, 0.5 / density);
            frame(bar, None, true, false, true);
            assert!(
                frame(bar, Some(true), true, false, true)
                    .0
                    .expect("seek press")
                    .1
                    .2
                    .is_none()
            );
            assert_eq!(
                frame(bar, Some(false), true, false, true)
                    .0
                    .expect("seek click")
                    .1
                    .2,
                Some(Action::Seek(media_time(Duration::from_secs(50))))
            );
            frame(bar, Some(true), true, false, true);
            let outside = thumbnail.right_bottom() + egui::vec2(80.0, 80.0);
            assert!(
                frame(outside, None, true, false, true)
                    .0
                    .expect("captured drag outside the card")
                    .1
                    .2
                    .is_none()
            );
            assert_eq!(
                frame(outside, Some(false), true, false, true)
                    .0
                    .expect("outside release")
                    .1
                    .2,
                Some(Action::Seek(media_time(Duration::from_secs(100))))
            );
            assert!(
                frame(outside, None, true, false, true).0.is_none(),
                "capture ends on release"
            );
            for external_drop in [false, true] {
                frame(source.center(), None, true, false, true);
                frame(bar, None, true, false, true);
                frame(bar, Some(true), true, false, true);
                assert!(timeline_input::is_active(&context));
                assert!(
                    frame(outside, None, external_drop, external_drop, true)
                        .0
                        .is_none()
                );
                assert!(
                    !timeline_input::is_active(&context),
                    "hiding the owner cancels its captured seek"
                );
                frame(outside, Some(false), true, false, true);
            }
            frame(source.center(), None, true, false, true);
            assert!(
                frame(egui::pos2(5.0, 200.0), None, true, false, true)
                    .0
                    .is_none()
            );
            assert!(
                frame(thumbnail.center(), None, true, false, true)
                    .0
                    .is_none(),
                "an old card cannot reopen itself"
            );
            frame(source.center(), None, true, false, true);
            assert!(
                frame(source.center(), Some(true), true, false, true)
                    .0
                    .is_none(),
                "tab dragging dismisses the card"
            );
            frame(source.center(), Some(false), true, false, true);
            frame(source.center(), None, true, false, true);
            assert!(
                frame(thumbnail.center(), None, true, true, true)
                    .0
                    .is_none()
            );
            frame(source.center(), None, true, false, true);
            assert!(
                frame(thumbnail.center(), None, false, false, true)
                    .0
                    .is_none()
            );
            frame(source.center(), None, true, false, true);
            frame(thumbnail.center(), None, true, false, false);
            assert!(
                frame(thumbnail.center(), None, true, false, true)
                    .0
                    .is_none(),
                "hidden source expires its hover ownership"
            );
        }
    }
}

#[test]
fn localized_preview_controls_keep_geometry_and_dispatch_numeric_and_button_actions() {
    use crate::localization::{Language, test_ui};
    use egui::accesskit::{Action as AccessAction, ActionData, ActionRequest, TreeId};
    for language in [Language::English, Language::Japanese] {
        for density in [1.0, 1.25, 2.0] {
            for kind in [MediaKind::Audio, MediaKind::Video] {
                for playing in [false, true] {
                    let context = test_ui::japanese_context(density);
                    localization::set_language(&context, language);
                    let thumbnail = egui::Rect::from_min_size(
                        egui::pos2(40.0, 40.0),
                        egui::vec2(
                            160.0,
                            if kind == MediaKind::Audio {
                                40.0
                            } else {
                                108.0
                            },
                        ),
                    );
                    let size = egui::vec2(420.0, 260.0);
                    let transport = Transport {
                        instance: 55,
                        kind,
                        state: if playing {
                            PlaybackState::Playing
                        } else {
                            PlaybackState::Paused
                        },
                        position: media_time(Duration::from_secs(12)),
                        duration: Some(media_time(Duration::from_secs(60))),
                        enabled: true,
                        previous: true,
                        next: true,
                    };
                    let frame = |events: Vec<egui::Event>| {
                        let mut actions = Vec::new();
                        let mut events = events;
                        events.insert(0, egui::Event::PointerMoved(thumbnail.center()));
                        let output = context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    size,
                                )),
                                focused: true,
                                events,
                                ..Default::default()
                            },
                            |ui| actions.extend(transport.show(ui, thumbnail)),
                        );
                        (output, actions)
                    };
                    for _ in 0..3 {
                        assert!(frame(vec![]).1.is_empty());
                    }
                    let (output, _) = frame(vec![]);
                    assert_eq!(output.pixels_per_point, density);
                    let label = if playing { Text::Pause } else { Text::Play };
                    let nodes = &output
                        .platform_output
                        .accesskit_update
                        .as_ref()
                        .expect("tree")
                        .nodes;
                    let node = nodes
                        .iter()
                        .find(|(_, node)| node.label() == Some(label.in_language(language)))
                        .expect("localized button");
                    let bounds = node.1.bounds().expect("bounds");
                    assert!(
                        bounds.x0 >= f64::from(thumbnail.left())
                            && bounds.x1 <= f64::from(thumbnail.right())
                    );
                    let (output, actions) = frame(vec![test_ui::action(
                        &output,
                        label.in_language(language),
                        None,
                    )]);
                    assert_eq!(actions, vec![Action::Command(CommandId::TogglePause)]);
                    if kind == MediaKind::Audio {
                        for (label, command) in [
                            (Text::PreviousTrack, CommandId::PreviousMedia),
                            (Text::NextTrack, CommandId::NextMedia),
                        ] {
                            let (_, actions) = frame(vec![test_ui::action(
                                &output,
                                label.in_language(language),
                                None,
                            )]);
                            assert_eq!(actions, vec![Action::Command(command)]);
                        }
                    }
                    let tree = output
                        .platform_output
                        .accesskit_update
                        .as_ref()
                        .expect("tree");
                    let target = crate::video_rotation::tests::node(
                        tree,
                        Text::PreviewPlaybackPosition.in_language(language),
                    );
                    let (_, actions) =
                        frame(vec![egui::Event::AccessKitActionRequest(ActionRequest {
                            action: AccessAction::SetValue,
                            target_tree: TreeId::ROOT,
                            target_node: target,
                            data: Some(ActionData::NumericValue(25.5)),
                        })]);
                    assert_eq!(
                        actions,
                        vec![Action::Seek(media_time(Duration::from_secs_f64(25.5)))]
                    );
                }
            }
        }
    }
}
