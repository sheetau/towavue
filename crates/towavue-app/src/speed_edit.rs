use crate::*;
use localization::Text;
use towavue_core::{EditTimeline, TimeRange, TimelineEdit};

pub(super) struct Dialog {
    token: u64,
    tab: TabId,
    instance: u64,
    history: EditHistory,
    selection: Option<TimeRange>,
    source_duration: Duration,
    plan: EditTimeline,
    range: TimeRange,
    speed: String,
    duration: String,
    from_speed: bool,
}

impl Dialog {
    fn length(&self) -> Option<MediaTime> {
        if self.from_speed {
            let speed: f64 = self.speed.trim().parse().ok()?;
            if !speed.is_finite() || speed <= 0.0 {
                return None;
            }
            let nanos = self.range.duration().as_nanoseconds() as f64 / speed;
            if !(1.0..i64::MAX as f64).contains(&nanos) {
                return None;
            }
            Some(MediaTime::from_nanoseconds(nanos.round() as i64))
        } else {
            parse_duration(&self.duration)
        }
    }

    fn sync_linked_input(&mut self) {
        if let Some(length) = self.length() {
            if self.from_speed {
                self.duration = format_duration(length);
            } else {
                let speed = self.range.duration().as_seconds_f64() / length.as_seconds_f64();
                self.speed = format!("{speed:.6}")
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .into();
            }
        }
    }

    fn valid_length(&self, length: MediaTime) -> bool {
        if length == self.range.duration() {
            return false;
        }
        let mut candidate = self.plan.clone();
        candidate.apply(TimelineEdit::Stretch(self.range, length)) && candidate != self.plan
    }

    fn show(&mut self, context: &egui::Context) -> Option<Option<MediaTime>> {
        let language = localization::language(context);
        let mut action = None;
        let modal =
            chrome::modal(context, egui::Id::new("speed-duration"), true).show(context, |ui| {
                let value = chrome::modal_body(
                    ui,
                    Text::CommandEditSpeed.in_language(language),
                    &[
                        Text::ApplySpeed.in_language(language),
                        Text::Cancel.in_language(language),
                    ],
                    |ui| {
                        let font = egui::TextStyle::Body.resolve(ui.style());
                        let label_width = [Text::SpeedLabel, Text::DurationLabel]
                            .into_iter()
                            .map(|key| {
                                ui.painter()
                                    .layout_no_wrap(
                                        key.in_language(language).into(),
                                        font.clone(),
                                        chrome::FOREGROUND,
                                    )
                                    .size()
                                    .x
                            })
                            .fold(0.0_f32, f32::max);
                        let field_width =
                            (ui.available_width() - label_width - ui.spacing().item_spacing.x)
                                .clamp(40.0, 170.0);
                        egui::Grid::new("speed-duration-fields")
                            .num_columns(2)
                            .show(ui, |ui| {
                                ui.label(Text::SpeedLabel.in_language(language));
                                ui.spacing_mut().text_edit_width = field_width;
                                if resize::text_input(
                                    ui,
                                    Text::SpeedLabel.in_language(language),
                                    &mut self.speed,
                                    "×",
                                )
                                .changed()
                                {
                                    self.from_speed = true;
                                    self.sync_linked_input();
                                }
                                ui.end_row();
                                ui.label(Text::DurationLabel.in_language(language));
                                if resize::text_input(
                                    ui,
                                    Text::DurationLabel.in_language(language),
                                    &mut self.duration,
                                    "",
                                )
                                .changed()
                                {
                                    self.from_speed = false;
                                    self.sync_linked_input();
                                }
                                ui.end_row();
                            });
                        let length = self.length();
                        if length.is_none() {
                            ui.label(Text::SpeedDurationInvalid.in_language(language));
                        } else if length.is_some_and(|length| {
                            length != self.range.duration() && !self.valid_length(length)
                        }) {
                            let limits =
                                time_selection::stretch_limits(self.range, Some(&self.plan));
                            let seconds = self.range.duration().as_seconds_f64();
                            ui.label(
                                towavue_core::localization::formatted::speed_duration_limits(
                                    language,
                                    seconds / *limits.end(),
                                    seconds / *limits.start(),
                                ),
                            );
                        }
                        length.filter(|length| self.valid_length(*length))
                    },
                );
                ui.horizontal_wrapped(|ui| {
                    chrome::flat_buttons(ui);
                    if ui
                        .add_enabled(
                            value.is_some(),
                            egui::Button::new(Text::ApplySpeed.in_language(language)),
                        )
                        .clicked()
                    {
                        action = Some(value);
                    }
                    if ui.button(Text::Cancel.in_language(language)).clicked() {
                        action = Some(None);
                    }
                });
            });
        if modal.is_top_modal
            && !modal.any_popup_open
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            action = Some(None);
        }
        action
    }
}

/// Exact nanosecond text round trips avoid changing an untouched short clip.
fn format_duration(time: MediaTime) -> String {
    let nanos = time.as_nanoseconds();
    let seconds = nanos / 1_000_000_000;
    let fraction = nanos % 1_000_000_000;
    let mut text = format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );
    if fraction != 0 {
        text.push_str(format!(".{fraction:09}").trim_end_matches('0'));
    }
    text
}

fn parse_duration(text: &str) -> Option<MediaTime> {
    let fields: Vec<_> = text.trim().split(':').collect();
    if fields.len() != 3 {
        return None;
    }
    let integer = |value: &str| -> Option<i64> {
        (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| value.parse().ok())
            .flatten()
    };
    let hours = integer(fields[0])?;
    let minutes = integer(fields[1])?;
    let (seconds, fraction) = fields[2].split_once('.').unwrap_or((fields[2], ""));
    let seconds = integer(seconds)?;
    if minutes >= 60
        || seconds >= 60
        || fraction.len() > 9
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let subsecond = if fraction.is_empty() {
        0
    } else {
        integer(fraction)?.checked_mul(10_i64.pow(9 - fraction.len() as u32))?
    };
    let nanos = hours
        .checked_mul(3600)?
        .checked_add(minutes * 60 + seconds)?
        .checked_mul(1_000_000_000)?
        .checked_add(subsecond)?;
    (nanos > 0).then_some(MediaTime::from_nanoseconds(nanos))
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    fn speed_edit_current(&self, dialog: &Dialog) -> bool {
        self.tabs.active_id() == Some(dialog.tab)
            && self.media_generation == dialog.instance
            && self.media_duration == Some(dialog.source_duration)
            && self.time_selection == dialog.selection
            && self.timeline_is_visible()
            && !matches!(self.state, PlaybackState::Loading | PlaybackState::Faulted)
            && self.document_source_available(Some(dialog.tab))
            && self.edits.get(&dialog.tab).cloned().unwrap_or_default() == dialog.history
    }

    pub(super) fn open_speed_edit(&mut self) {
        if self.modal_input_blocked()
            || !self.timeline_is_visible()
            || !matches!(self.media_kind, Some(MediaKind::Audio | MediaKind::Video))
            || matches!(self.state, PlaybackState::Loading | PlaybackState::Faulted)
        {
            return;
        }
        let (Some(tab), Some(source_duration)) = (self.tabs.active_id(), self.media_duration)
        else {
            return;
        };
        if !self.document_source_available(Some(tab)) {
            return;
        }
        self.cancel_hold_speed();
        self.cancel_frame_steps();
        let history = self.edits.get(&tab).cloned().unwrap_or_default();
        let Some(plan) = history.timeline(media_time(source_duration)) else {
            return;
        };
        // Legacy global trims still expose source-axis selection before the first
        // timeline edit. Translate that selection into the edited axis once.
        let offset = if history
            .operations()
            .iter()
            .any(|op| matches!(op, EditOperation::Timeline(_)))
        {
            MediaTime::ZERO
        } else {
            history.state().trim_start.unwrap_or(MediaTime::ZERO)
        };
        let range = self
            .time_selection
            .and_then(|range| {
                TimeRange::new(
                    MediaTime::from_nanoseconds(
                        range.start().as_nanoseconds() - offset.as_nanoseconds(),
                    ),
                    MediaTime::from_nanoseconds(
                        range.end().as_nanoseconds() - offset.as_nanoseconds(),
                    ),
                )
            })
            .or_else(|| {
                self.time_selection
                    .is_none()
                    .then(|| TimeRange::new(MediaTime::ZERO, plan.duration()))
                    .flatten()
            });
        let Some(range) = range.filter(|range| range.end() <= plan.duration()) else {
            return;
        };
        self.guard_return_focus = self
            .ui_context
            .as_ref()
            .and_then(|context| context.memory(egui::Memory::focused))
            .map(|focus| (tab, focus));
        self.rotation_generation = self.rotation_generation.wrapping_add(1);
        if let Some(context) = &self.ui_context {
            egui::Popup::close_all(context);
        }
        self.speed_dialog = Some(Dialog {
            token: self.rotation_generation,
            tab,
            instance: self.media_generation,
            history,
            selection: self.time_selection,
            source_duration,
            plan,
            range,
            speed: "1.00".into(),
            duration: format_duration(range.duration()),
            from_speed: true,
        });
        self.request_redraw();
    }

    pub(super) fn cancel_stale_speed_edit(&mut self) {
        if self
            .speed_dialog
            .as_ref()
            .is_some_and(|dialog| !self.speed_edit_current(dialog))
        {
            self.speed_dialog = None;
            self.set_status(Text::SpeedEditChanged.in_language(self.language()).into());
        }
    }

    pub(super) fn show_speed_edit(&mut self, context: &egui::Context, actions: &mut Vec<UiAction>) {
        if let Some(dialog) = &mut self.speed_dialog
            && let Some(value) = dialog.show(context)
        {
            actions.push(UiAction::FinishSpeedEdit(dialog.token, value));
        }
    }

    pub(super) fn finish_speed_edit(&mut self, token: u64, value: Option<MediaTime>) {
        if self
            .speed_dialog
            .as_ref()
            .is_none_or(|dialog| dialog.token != token)
        {
            return;
        }
        let dialog = self.speed_dialog.take().expect("matching speed dialog");
        if let Some(length) = value {
            if self.speed_edit_current(&dialog) && dialog.valid_length(length) {
                self.push_edit(EditOperation::Timeline(TimelineEdit::Stretch(
                    dialog.range,
                    length,
                )));
            } else {
                self.set_status(Text::SpeedEditChanged.in_language(self.language()).into());
            }
        }
        self.request_redraw();
    }
}

#[cfg(test)]
mod tests;
