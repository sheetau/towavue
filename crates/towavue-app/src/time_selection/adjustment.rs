use super::*;

pub(super) fn bands(duration: MediaTime, plan: Option<&EditTimeline>) -> Vec<(TimeRange, f32)> {
    let Some(plan) = plan else {
        return TimeRange::new(MediaTime::ZERO, duration)
            .map(|range| vec![(range, 1.0)])
            .unwrap_or_default();
    };
    let mut offset = MediaTime::ZERO;
    plan.spans()
        .iter()
        .filter_map(|span| {
            let end = MediaTime::from_nanoseconds(
                offset.as_nanoseconds() + span.duration().as_nanoseconds(),
            );
            let range = TimeRange::new(offset, end);
            offset = end;
            range.map(|range| (range, span.volume()))
        })
        .collect()
}

pub(super) fn gain_at(bands: &[(TimeRange, f32)], time: MediaTime) -> f32 {
    bands
        .iter()
        .find(|(range, _)| time >= range.start() && time < range.end())
        .or_else(|| bands.last())
        .map_or(1.0, |(_, gain)| *gain)
}

pub(super) fn gain_range_at(bands: &[(TimeRange, f32)], time: MediaTime) -> Option<TimeRange> {
    let index = bands
        .iter()
        .position(|(range, _)| time >= range.start() && time < range.end())
        .or_else(|| bands.len().checked_sub(1))?;
    let gain = bands[index].1;
    let (mut left, mut right) = (index, index);
    // Source cuts and rate changes can split one visible gain line into several
    // spans. Drag the connected line, but never cross a different gain level.
    while left > 0 && bands[left - 1].1 == gain {
        left -= 1;
    }
    while right + 1 < bands.len() && bands[right + 1].1 == gain {
        right += 1;
    }
    TimeRange::new(bands[left].0.start(), bands[right].0.end())
}

pub(super) fn gain_height(rect: Rect) -> f32 {
    // Unity stays centered; 75% of either half reaches 200% or silence.
    // This is 1.5 times the former quarter-height travel per gain unit.
    // Painting, hit testing and dragging share the same scale.
    rect.height().max(1.0) * 0.375
}

pub(super) fn gain_y(rect: Rect, gain: f32) -> f32 {
    rect.center().y + (1.0 - gain) * gain_height(rect)
}

pub(super) fn stretch_limits(
    range: TimeRange,
    plan: Option<&EditTimeline>,
) -> std::ops::RangeInclusive<f64> {
    let mut minimum: f64 = 0.25;
    let mut maximum: f64 = 4.0;
    if let Some(plan) = plan {
        minimum = 0.0;
        maximum = f64::INFINITY;
        let mut offset = 0_i64;
        for span in plan.spans() {
            let end = offset + span.duration().as_nanoseconds();
            if offset < range.end().as_nanoseconds() && end > range.start().as_nanoseconds() {
                minimum = minimum.max(span.rate() / 4.0);
                maximum = maximum.min(span.rate() / 0.25);
            }
            offset = end;
        }
    }
    let length = range.duration().as_nanoseconds() as f64;
    // Round inward; core validation remains authoritative for split/sample rounding.
    let lower = (length * minimum).ceil().max(1.0) / 1e9;
    let upper = (length * maximum)
        .floor()
        .min((i64::MAX - range.start().as_nanoseconds()) as f64)
        / 1e9;
    lower..=upper.max(lower)
}

pub(super) fn changes_plan(
    duration: MediaTime,
    plan: Option<&EditTimeline>,
    edit: TimelineEdit,
) -> bool {
    let Some(original) = plan
        .cloned()
        .or_else(|| EditTimeline::new(duration, Default::default()))
    else {
        return false;
    };
    let mut candidate = original.clone();
    candidate.apply(edit) && candidate != original
}

pub(super) fn paint(
    painter: &egui::Painter,
    rect: Rect,
    bands: &[(TimeRange, f32)],
    preview: Option<(TimeRange, f32)>,
    x_at: &impl Fn(MediaTime) -> f32,
) {
    for &(range, gain) in bands {
        let mut cuts = vec![range.start(), range.end()];
        if let Some((selected, _)) = preview {
            cuts.extend(
                [selected.start(), selected.end()]
                    .into_iter()
                    .filter(|time| *time > range.start() && *time < range.end()),
            );
            cuts.sort();
        }
        for pair in cuts.windows(2) {
            let gain = preview
                .filter(|(selected, _)| pair[0] >= selected.start() && pair[1] <= selected.end())
                .map_or(gain, |(_, gain)| gain);
            painter.hline(
                x_at(pair[0])..=x_at(pair[1]),
                gain_y(rect, gain),
                (1.0, egui::Color32::from_white_alpha(128)),
            );
        }
    }
    if let Some((_, gain)) = preview {
        painter.text(
            rect.left_top() + egui::vec2(LABEL_INSET, 10.0),
            egui::Align2::LEFT_CENTER,
            format!("Volume {:.0}%", gain * 100.0),
            egui::FontId::proportional(LABEL_SIZE),
            crate::chrome::FOREGROUND,
        );
    }
}

pub(super) fn values(
    ui: &Ui,
    response: &Response,
    duration: MediaTime,
    selection: Option<TimeRange>,
    plan: Option<&EditTimeline>,
    preview: Option<(TimeRange, f32)>,
    enabled: bool,
) -> Option<TimelineEdit> {
    let range = selection.or_else(|| TimeRange::new(MediaTime::ZERO, duration))?;
    let bands = bands(duration, plan);
    // A discarded release pass precedes model commit. Preserve its displayed
    // uniform gain while keeping the existing control IDs/focus and input path.
    let preview_gain = preview
        .filter(|(selected, _)| *selected == range)
        .map(|(_, gain)| gain);
    let gain = preview_gain.unwrap_or_else(|| gain_at(&bands, range.start()));
    let mixed = preview_gain.is_none()
        && bands.iter().any(|(span, volume)| {
            span.start() < range.end() && span.end() > range.start() && *volume != gain
        });
    let mut result = None;
    for stretch in [false, true] {
        let width = (response.rect.width() * 0.5).min(160.0);
        let rect = Rect::from_min_size(
            egui::pos2(
                if stretch {
                    response.rect.right() - width
                } else {
                    response.rect.left()
                },
                response.rect.top(),
            ),
            egui::vec2(width, 20.0),
        );
        let control = ui.interact(
            rect,
            response.id.with(("timeline-adjustment-value", stretch)),
            egui::Sense::focusable_noninteractive(),
        );
        let name = if stretch {
            "Selected duration (seconds)"
        } else {
            "Local volume (%)"
        };
        let value = if stretch {
            range.duration().as_seconds_f64()
        } else {
            f64::from(gain) * 100.0
        };
        let available = enabled
            && response.enabled()
            && range.end() <= duration
            && !egui::Popup::is_any_open(ui.ctx())
            && (!stretch || selection.is_some());
        let bounds = if stretch {
            stretch_limits(range, plan)
        } else {
            0.0..=f64::from(towavue_core::MAX_VOLUME) * 100.0
        };
        // Preserve explicit unification when the ordered batch ends at the starting value.
        // value_input consumes the events and computes their final value below.
        let uniform_gain = if mixed && !stretch && available {
            ui.input(|input| {
                input.events.iter().any(|event| match event {
                    egui::Event::AccessKitActionRequest(request)
                        if request.target_tree == egui::accesskit::TreeId::ROOT
                            && request.target_node == control.id.accesskit_id()
                            && request.action == egui::accesskit::Action::SetValue =>
                    {
                        matches!(
                            request.data,
                            Some(egui::accesskit::ActionData::NumericValue(value))
                                if value.is_finite()
                        )
                    }
                    _ => false,
                })
            })
        } else {
            false
        };
        let next = crate::seekbar::value_input(
            &control,
            name,
            value,
            bounds,
            if stretch { 0.1 } else { 5.0 },
            available,
        )
        .or(uniform_gain.then_some(value));
        // The gain-line preview already paints its value. Do not overlay the
        // ordinary gain caption, but retain its numeric/accessibility control.
        if (control.has_focus() || selection.is_some()) && (stretch || preview.is_none()) {
            let label = if stretch {
                format!("Length {}", crate::format_time_precise(range.duration()))
            } else if mixed {
                format!("Mixed (start {value:.0}%)")
            } else {
                format!("Volume {value:.0}%")
            };
            ui.painter().with_clip_rect(rect).text(
                if stretch {
                    rect.right_center() - egui::vec2(LABEL_INSET, 0.0)
                } else {
                    rect.left_center() + egui::vec2(LABEL_INSET, 0.0)
                },
                if stretch {
                    egui::Align2::RIGHT_CENTER
                } else {
                    egui::Align2::LEFT_CENTER
                },
                label,
                egui::FontId::proportional(LABEL_SIZE),
                crate::chrome::FOREGROUND,
            );
        }
        super::describe_focus(&control, available, name, value, stretch);
        if let Some(next) = next {
            let edit = if stretch {
                TimelineEdit::Stretch(
                    range,
                    crate::media_time(std::time::Duration::from_secs_f64(next)),
                )
            } else {
                TimelineEdit::SetVolume(range, (next / 100.0) as f32)
            };
            if result.is_none() && changes_plan(duration, plan, edit) {
                result = Some(edit);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn time(seconds: i64) -> MediaTime {
        MediaTime::from_nanoseconds(seconds * 1_000_000_000)
    }
    fn range(start: i64, end: i64) -> TimeRange {
        TimeRange::new(time(start), time(end)).expect("range")
    }

    #[test]
    fn gain_travel_uses_three_quarters_of_each_half_and_keeps_unity_centered() {
        for top in [0.0, 30.0] {
            for height in [80.0, 100.0, 180.0] {
                let rect = Rect::from_min_size(egui::pos2(20.0, top), egui::vec2(400.0, height));
                assert_eq!(gain_height(rect), height * 0.375);
                for (gain, fraction) in [
                    (0.0, 0.875),
                    (0.5, 0.6875),
                    (1.0, 0.5),
                    (1.5, 0.3125),
                    (2.0, 0.125),
                ] {
                    assert_eq!(gain_y(rect, gain), top + height * fraction);
                }
            }
        }
    }

    #[test]
    fn gain_ranges_join_equal_neighbors_and_use_half_open_boundaries() {
        let bands = [
            (range(0, 1), 1.0),
            (range(1, 2), 1.0),
            (range(2, 4), 0.0),
            (range(4, 8), 1.0),
        ];
        for (at, expected) in [
            (0, range(0, 2)),
            (1, range(0, 2)),
            (2, range(2, 4)),
            (4, range(4, 8)),
            (8, range(4, 8)),
        ] {
            assert_eq!(gain_range_at(&bands, time(at)), Some(expected));
        }
        assert_eq!(gain_range_at(&[], time(0)), None);
    }

    #[test]
    fn limits_respect_every_selected_rate_and_gain_changes_are_atomic() {
        let mut plan = EditTimeline::new(time(10), Default::default()).expect("plan");
        assert!(plan.apply(TimelineEdit::Stretch(range(0, 4), time(2))));
        assert_eq!(stretch_limits(range(0, 8), Some(&plan)), 4.0..=32.0);
        assert_eq!(stretch_limits(range(2, 8), Some(&plan)), 1.5..=24.0);
        assert!(changes_plan(
            time(10),
            Some(&plan),
            TimelineEdit::Stretch(range(0, 8), time(4))
        ));
        assert!(!changes_plan(
            time(10),
            Some(&plan),
            TimelineEdit::Stretch(range(0, 8), time(3))
        ));
        assert!(!changes_plan(
            time(10),
            Some(&plan),
            TimelineEdit::SetVolume(range(0, 8), 1.0)
        ));
        assert!(!changes_plan(
            time(10),
            Some(&plan),
            TimelineEdit::SetVolume(range(0, 8), f32::NAN)
        ));
        assert!(plan.apply(TimelineEdit::SetVolume(range(2, 4), 0.0)));
        assert_eq!(gain_at(&bands(time(8), Some(&plan)), time(2)), 0.0);
        assert_eq!(gain_at(&bands(time(8), Some(&plan)), time(4)), 1.0);
        assert!(changes_plan(
            time(10),
            Some(&plan),
            TimelineEdit::SetVolume(range(0, 8), 1.0)
        ));
    }

    #[test]
    fn mixed_gain_value_batches_preserve_event_order_without_replaying_edits() {
        use egui::accesskit::{Action, ActionData, ActionRequest, TreeId};
        for density in [1.0, 1.25, 2.0] {
            for selection in [None, Some(range(2, 8))] {
                for discard in [false, true] {
                    let context = crate::fonts::test_context();
                    context.set_pixels_per_point(density);
                    context.enable_accesskit();
                    let mut plan = EditTimeline::new(time(10), Default::default()).expect("plan");
                    assert!(plan.apply(TimelineEdit::SetVolume(range(3, 7), 0.0)));
                    let id = egui::Id::new("mixed-gain-order");
                    let event = |action, value: Option<f64>| {
                        egui::Event::AccessKitActionRequest(ActionRequest {
                            action,
                            target_tree: TreeId::ROOT,
                            target_node: id
                                .with(("timeline-adjustment-value", false))
                                .accesskit_id(),
                            data: value.map(ActionData::NumericValue),
                        })
                    };
                    for events in [
                        vec![
                            event(Action::SetValue, Some(105.0)),
                            event(Action::Decrement, None),
                        ],
                        vec![
                            event(Action::SetValue, Some(95.0)),
                            event(Action::Increment, None),
                        ],
                        vec![event(Action::SetValue, Some(100.0))],
                        vec![
                            event(Action::SetValue, Some(80.0)),
                            event(Action::SetValue, Some(100.0)),
                        ],
                    ] {
                        let mut edits = Vec::new();
                        let mut passes = 0;
                        let _ = context.run_ui(
                            egui::RawInput {
                                events,
                                ..Default::default()
                            },
                            |ui| {
                                let response = ui.interact(
                                    Rect::from_min_size(
                                        egui::pos2(20.0, 20.0),
                                        egui::vec2(400.0, 100.0),
                                    ),
                                    id,
                                    egui::Sense::click_and_drag(),
                                );
                                if let Some(edit) = values(
                                    ui,
                                    &response,
                                    time(10),
                                    selection,
                                    Some(&plan),
                                    None,
                                    true,
                                ) {
                                    edits.push(edit);
                                }
                                passes += 1;
                                if discard && passes == 1 {
                                    context.request_discard("mixed gain input must not replay");
                                }
                            },
                        );
                        assert_eq!(
                            edits,
                            vec![TimelineEdit::SetVolume(
                                selection.unwrap_or(range(0, 10)),
                                1.0
                            )]
                        );
                        if discard {
                            assert!(passes > 1);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn accessible_volume_unifies_mixed_values_and_focused_keyboard_edits_length() {
        let context = crate::fonts::test_context();
        context.enable_accesskit();
        let mut plan = EditTimeline::new(time(10), Default::default()).expect("plan");
        assert!(plan.apply(TimelineEdit::SetVolume(range(3, 7), 0.0)));
        let mut result = None;
        let mut draw = |events, enabled, focus| {
            let output = context.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    let response = ui.interact(
                        Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(400.0, 100.0)),
                        "test-values".into(),
                        egui::Sense::click_and_drag(),
                    );
                    if focus {
                        ui.memory_mut(|memory| {
                            memory.request_focus(
                                response.id.with(("timeline-adjustment-value", true)),
                            )
                        });
                    }
                    result = values(
                        ui,
                        &response,
                        time(10),
                        Some(range(0, 10)),
                        Some(&plan),
                        None,
                        enabled,
                    );
                },
            );
            (
                output.platform_output.accesskit_update.expect("tree"),
                result.take(),
            )
        };
        let (tree, _) = draw(vec![], true, false);
        let id = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Local volume (%)"))
            .expect("volume")
            .0;
        assert_eq!(
            tree.nodes
                .iter()
                .find(|(node_id, _)| *node_id == id)
                .expect("volume")
                .1
                .max_numeric_value(),
            Some(200.0)
        );
        let event = |value| {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::SetValue,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: id,
                data: Some(egui::accesskit::ActionData::NumericValue(value)),
            })
        };
        assert_eq!(draw(vec![event(100.0)], false, false).1, None);
        assert_eq!(
            draw(vec![event(100.0)], true, false).1,
            Some(TimelineEdit::SetVolume(range(0, 10), 1.0))
        );
        for value in [200.0, 300.0, 400.0] {
            assert_eq!(
                draw(vec![event(value)], true, false).1,
                Some(TimelineEdit::SetVolume(range(0, 10), 2.0))
            );
        }
        let key = egui::Event::Key {
            key: egui::Key::ArrowRight,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        assert_eq!(
            draw(vec![key], true, true).1,
            Some(TimelineEdit::Stretch(
                range(0, 10),
                MediaTime::from_nanoseconds(10_100_000_000)
            ))
        );
    }
}
