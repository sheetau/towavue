use super::*;

fn button(pos: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    }
}

struct Trial {
    context: Context,
    inline: bool,
    reversed: bool,
    enabled: bool,
    discard: bool,
    status: Rect,
}

impl Trial {
    fn new(density: f32, inline: bool, reversed: bool) -> Self {
        let context = crate::fonts::test_context();
        context.set_pixels_per_point(density);
        let trial = Self {
            context,
            inline,
            reversed,
            enabled: true,
            discard: false,
            status: Rect::from_min_size(Pos2::new(0.0, 650.0), egui::vec2(800.0, 30.0)),
        };
        for _ in 0..3 {
            trial.frame(vec![]);
        }
        trial
    }

    fn frame(&self, events: Vec<egui::Event>) -> (timeline_input::Drag, egui::FullOutput) {
        let mut result = timeline_input::Drag::default();
        let mut commits = 0;
        let mut passes = 0;
        let output = self.context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 700.0))),
                events,
                ..Default::default()
            },
            |ui| {
                let (_, drag) = if self.inline {
                    ui.add_enabled_ui(self.enabled, |ui| {
                        super::super::inline_directed(
                            ui,
                            Rect::from_center_size(
                                self.status.center_top(),
                                egui::vec2(800.0, 14.0),
                            ),
                            Id::new("inline-test"),
                            0.0,
                            self.reversed,
                        )
                    })
                    .inner
                } else {
                    super::super::show_directed_drag(
                        &self.context,
                        self.status,
                        0.0,
                        None,
                        self.enabled,
                        self.reversed,
                    )
                };
                assert!(!drag.open_timeline);
                commits += usize::from(drag.released);
                if !result.released {
                    result = drag;
                }
                if let Some(hint) = status(&self.context) {
                    ui.label(hint);
                }
                passes += 1;
                if self.discard && passes == 1 {
                    self.context
                        .request_discard("precision motion applies once per input frame");
                }
            },
        );
        assert!(commits <= 1);
        (result, output)
    }
}

#[test]
fn four_speeds_reverse_without_jumping_and_keep_fractional_motion() {
    for density in [1.0, 1.25, 2.0] {
        for inline in [false, true] {
            let trial = Trial::new(density, inline, false);
            let origin = Pos2::new(100.0, 650.0);
            trial.frame(vec![
                egui::Event::PointerMoved(origin),
                button(origin, true),
            ]);
            let mut raw = origin;
            let mut expected = origin.x;
            for band in [0, 1, 2, 3, 2, 1, 0] {
                raw.y = 650.0 - (band as f32 + 0.25) * 160.0;
                let (drag, _) = trial.frame(vec![egui::Event::PointerMoved(raw)]);
                assert!((drag.position.expect("held position").x - expected).abs() < 0.001);
                for _ in 0..8 {
                    raw.x += 0.5;
                    expected += 0.5 * [1.0, 0.5, 0.25, 0.1][band];
                    let (drag, output) = trial.frame(vec![egui::Event::PointerMoved(raw)]);
                    assert!((drag.position.expect("subpixel position").x - expected).abs() < 0.001);
                    assert_eq!(status(&trial.context), Some(LABELS[band]));
                    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                        egui::Shape::Text(text) if text.galley.text() == LABELS[band])));
                }
            }
            let (drag, _) = trial.frame(vec![button(raw, false)]);
            assert!(drag.released);
            assert!((drag.position.expect("commit").x - expected).abs() < 0.001);
            assert!(status(&trial.context).is_none());
        }
    }
}

#[test]
fn precision_release_is_independent_of_batched_events_discard_and_reading_direction() {
    for reversed in [false, true] {
        for batched in [false, true] {
            let mut trial = Trial::new(1.25, false, reversed);
            trial.discard = true;
            let points = [
                Pos2::new(100.0, 650.0),
                Pos2::new(100.0, 470.0),
                Pos2::new(180.0, 470.0),
                Pos2::new(180.0, 150.0),
                Pos2::new(260.0, 150.0),
                Pos2::new(260.0, 650.0),
                Pos2::new(300.0, 650.0),
            ];
            let events = std::iter::once(button(points[0], true))
                .chain(points[1..].iter().copied().map(egui::Event::PointerMoved))
                .chain([button(points[6], false)])
                .collect::<Vec<_>>();
            let drag = if batched {
                trial.frame(events).0
            } else {
                let mut last = timeline_input::Drag::default();
                for event in events {
                    last = trial.frame(vec![event]).0;
                }
                last
            };
            assert!(drag.released);
            let x = drag.position.expect("commit").x;
            assert!(
                (x - 188.0).abs() < 0.001,
                "scaled motion, not cursor position: {x}"
            );
            let rect = Rect::from_center_size(trial.status.center_top(), egui::vec2(800.0, 14.0));
            let ratio = super::super::directed_ratio(rect, x, reversed);
            let ordinary = super::super::compact_ratio(rect, 188.0);
            assert!((ratio - if reversed { 1.0 - ordinary } else { ordinary }).abs() < 0.001);
            assert!(!trial.frame(vec![]).0.released);
            assert!(status(&trial.context).is_none());
        }
    }
}

#[test]
fn precision_cancellation_geometry_and_new_press_do_not_reuse_old_offsets() {
    for mode in 0..5 {
        let mut trial = Trial::new(1.0, false, false);
        let origin = Pos2::new(100.0, 650.0);
        trial.frame(vec![
            egui::Event::PointerMoved(origin),
            button(origin, true),
        ]);
        let end = Pos2::new(300.0, 100.0);
        trial.frame(vec![egui::Event::PointerMoved(end)]);
        assert!(status(&trial.context).is_some());
        let events = match mode {
            0 => vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            1 => vec![egui::Event::WindowFocused(false)],
            2 => {
                trial.enabled = false;
                vec![]
            }
            3 => {
                trial.status = trial.status.translate(egui::vec2(0.0, -10.0));
                vec![]
            }
            _ => {
                timeline_input::cancel(&trial.context);
                vec![]
            }
        };
        assert!(!trial.frame(events).0.released);
        assert!(status(&trial.context).is_none());
        trial.enabled = true;
        assert!(
            !trial
                .frame(vec![egui::Event::WindowFocused(true), button(end, false)])
                .0
                .released
        );
        let fresh = Pos2::new(400.0, trial.status.top());
        trial.frame(vec![egui::Event::PointerMoved(fresh), button(fresh, true)]);
        let (drag, _) = trial.frame(vec![button(fresh, false)]);
        assert_eq!(drag.position, Some(fresh));
    }
}

#[test]
fn diagonal_band_crossings_are_stable_when_motion_events_are_coalesced() {
    let rect = Rect::from_center_size(Pos2::new(400.0, 650.0), egui::vec2(800.0, 14.0));
    for (from, to) in [
        (Pos2::new(100.0, 650.0), Pos2::new(700.0, 50.0)),
        (Pos2::new(700.0, 50.0), Pos2::new(100.0, 650.0)),
    ] {
        let total = scaled_delta(rect, from, to);
        let split: f32 = (0..30)
            .map(|i| {
                scaled_delta(
                    rect,
                    from.lerp(to, i as f32 / 30.0),
                    from.lerp(to, (i + 1) as f32 / 30.0),
                )
            })
            .sum();
        assert!((total - split).abs() < 0.001);
        assert!((total.abs() - 292.0).abs() < 0.001);
    }
}
