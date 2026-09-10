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

pub(super) fn gain_height(rect: Rect) -> f32 {
    (rect.height() - 44.0).max(1.0)
}

pub(super) fn gain_y(rect: Rect, gain: f32) -> f32 {
    rect.center().y + (1.0 - gain) * gain_height(rect) * 0.5
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
                (1.0, crate::chrome::FOREGROUND),
            );
        }
    }
    if let Some((range, gain)) = preview {
        painter.text(
            egui::pos2(x_at(range.start()), rect.top() + 2.0),
            egui::Align2::LEFT_TOP,
            format!("Volume {:.0}%", gain * 100.0),
            egui::FontId::proportional(11.0),
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
    enabled: bool,
) -> Option<TimelineEdit> {
    let range = selection.or_else(|| TimeRange::new(MediaTime::ZERO, duration))?;
    let bands = bands(duration, plan);
    let gain = gain_at(&bands, range.start());
    let mixed = bands.iter().any(|(span, volume)| {
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
            && !egui::Popup::is_any_open(ui.ctx())
            && (!stretch || selection.is_some());
        let bounds = if stretch {
            stretch_limits(range, plan)
        } else {
            0.0..=200.0
        };
        // An explicit value also unifies a mixed selection when its first span already matches.
        let uniform_gain = if mixed && !stretch && available {
            ui.input(|input| {
                input
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        egui::Event::AccessKitActionRequest(request)
                            if request.target_tree == egui::accesskit::TreeId::ROOT
                                && request.target_node == control.id.accesskit_id()
                                && request.action == egui::accesskit::Action::SetValue =>
                        {
                            match request.data {
                                Some(egui::accesskit::ActionData::NumericValue(value))
                                    if value.is_finite() =>
                                {
                                    Some(value.clamp(0.0, 200.0))
                                }
                                _ => None,
                            }
                        }
                        _ => None,
                    })
                    .next_back()
            })
        } else {
            None
        };
        let next = crate::seekbar::value_input(
            &control,
            name,
            value,
            bounds,
            if stretch { 0.1 } else { 5.0 },
            available,
        )
        .or(uniform_gain);
        if control.has_focus() || selection.is_some() {
            let label = if stretch {
                format!("Length {value:.3}s")
            } else if mixed {
                format!("Mixed (start {value:.0}%)")
            } else {
                format!("Volume {value:.0}%")
            };
            ui.painter().with_clip_rect(rect).text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(11.0),
                crate::chrome::FOREGROUND,
            );
        }
        super::describe_focus(&control, available, name, value);
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
        let event = || {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::SetValue,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: id,
                data: Some(egui::accesskit::ActionData::NumericValue(100.0)),
            })
        };
        assert_eq!(draw(vec![event()], false, false).1, None);
        assert_eq!(
            draw(vec![event()], true, false).1,
            Some(TimelineEdit::SetVolume(range(0, 10), 1.0))
        );
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
