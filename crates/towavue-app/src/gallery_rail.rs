use crate::chrome;
use crate::localization::{self, Language, Text};
use towavue_core::localization::formatted;

#[derive(Clone, Copy)]
struct Position {
    layout: egui::Id,
    offset: f32,
    fraction: f32,
}

#[derive(Clone)]
struct Motion {
    layout: egui::Id,
    fraction: f32,
    target: f32,
    offset: f32,
    maximum: f32,
    time: f64,
    pass: u64,
}

#[derive(Clone, Copy)]
struct DragAnchor {
    pointer: f32,
    fraction: f32,
}

/// Advance one retargetable motion before laying out cards. State changes from
/// wheel, keyboard or projection replacement cancel it instead of being replayed.
pub(super) fn advance(context: &egui::Context, scroll_id: egui::Id) {
    let id = scroll_id.with("rail-motion");
    let Some(mut motion) = context.data(|data| data.get_temp::<Motion>(id)) else {
        return;
    };
    let mut state = egui::scroll_area::State::load(context, scroll_id).unwrap_or_default();
    if (state.offset.y - motion.offset).abs() > 0.01 {
        context.data_mut(|data| data.remove::<Motion>(id));
        return;
    }
    let pass = context.cumulative_pass_nr();
    if motion.pass == pass {
        return;
    }
    motion.pass = pass;
    let now = context.input(|input| input.time);
    let dt = (now - motion.time).clamp(0.0, 0.05) as f32;
    motion.time = now;
    motion.fraction += (motion.target - motion.fraction) * (1.0 - (-dt / 0.055).exp());
    if (motion.target - motion.fraction).abs() < 0.0001 {
        motion.fraction = motion.target;
    }
    motion.offset = offset_at(motion.maximum, motion.fraction);
    let moving = motion.fraction != motion.target;
    state = egui::scroll_area::State::default();
    state.offset.y = motion.offset;
    state.store(context, scroll_id);
    context.data_mut(|data| {
        data.insert_temp(
            scroll_id.with("gallery-rail-position"),
            Position {
                layout: motion.layout,
                offset: motion.offset,
                fraction: motion.fraction,
            },
        );
        if moving {
            data.insert_temp(id, motion);
        } else {
            data.remove::<Motion>(id);
        }
    });
    if moving {
        context.request_repaint();
    }
}

pub(super) fn dragging(context: &egui::Context) -> bool {
    context
        .data(|data| data.get_temp::<(egui::Id, u64)>(egui::Id::new("gallery-rail-drag")))
        .is_some_and(|(id, frame)| {
            frame.saturating_add(1) >= context.cumulative_frame_nr()
                && context.input(|input| input.focused && input.pointer.primary_down())
                && context.read_response(id).is_some_and(|response| {
                    response.is_pointer_button_down_on() || response.dragged()
                })
        })
}

#[derive(Clone)]
pub(super) struct Month {
    pub date: Option<(u16, u16)>,
    pub offset: f32,
}

impl Month {
    fn label(&self, language: Language) -> String {
        match self.date {
            Some((year, month)) => {
                formatted::calendar_month(language, month_name(month, language), year)
            }
            None => Text::DateUnknown.in_language(language).into(),
        }
    }
}

pub(super) fn month_name(month: u16, language: Language) -> &'static str {
    const NAMES: [Text; 12] = [
        Text::January,
        Text::February,
        Text::March,
        Text::April,
        Text::May,
        Text::June,
        Text::July,
        Text::August,
        Text::September,
        Text::October,
        Text::November,
        Text::December,
    ];
    NAMES[usize::from(month) - 1].in_language(language)
}

// All pointer motion uses the same linear content scale as a native scrollbar.
// Dates annotate that scale; they never change the amount scrolled per pixel.
pub(super) fn show(
    ui: &mut egui::Ui,
    output: &mut egui::scroll_area::ScrollAreaOutput<Vec<Month>>,
    rect: egui::Rect,
) {
    let language = localization::language(ui.ctx());
    let drag_id = egui::Id::new("gallery-rail-drag");
    ui.data_mut(|data| data.remove::<(egui::Id, u64)>(drag_id));
    let months = &output.inner;
    let position_id = output.id.with("gallery-rail-position");
    if months.is_empty() || rect.height() < 12.0 {
        ui.data_mut(|data| {
            data.remove::<Position>(position_id);
            data.remove::<Motion>(output.id.with("rail-motion"));
        });
        return;
    }
    let maximum = (output.content_size.y - output.inner_rect.height()).max(0.0);
    let layout = months
        .iter()
        .fold(output.id.with(maximum.to_bits()), |id, month| {
            id.with((month.date, month.offset.to_bits()))
        });
    let pixel = 1.0 / ui.ctx().pixels_per_point();
    let thumb_height = (rect.height() * output.inner_rect.height()
        / output.content_size.y.max(1.0))
    .clamp(pixel, rect.height());
    let travel = rect.top() + thumb_height * 0.5..=rect.bottom() - thumb_height * 0.5;
    let remembered = ui
        .data(|data| data.get_temp::<Position>(position_id))
        .filter(|position| {
            position.layout == layout && (position.offset - output.state.offset.y).abs() < 0.01
        });
    let fraction = remembered.map_or_else(
        || fraction_at(maximum, output.state.offset.y),
        |position| position.fraction,
    );
    let motion_id = output.id.with("rail-motion");
    if ui
        .data(|data| data.get_temp::<Motion>(motion_id))
        .is_some_and(|motion| {
            motion.layout != layout || (motion.offset - output.state.offset.y).abs() > 0.01
        })
    {
        ui.data_mut(|data| data.remove::<Motion>(motion_id));
    }
    let pointer_fraction = |point: egui::Pos2| {
        ((point.y - *travel.start()) / (travel.end() - travel.start()).max(1.0)).clamp(0.0, 1.0)
    };
    let line_x = ((rect.left() + rect.width() * 0.075) / pixel).round() * pixel
        ..=((rect.right() - rect.width() * 0.075) / pixel).round() * pixel;
    let line = |left: f32, y: f32| {
        // A one-physical-pixel fill needs integer edges, not an integer center.
        let top = ((y - pixel * 0.5) / pixel).round() * pixel;
        egui::Rect::from_min_max(
            egui::pos2(left, top),
            egui::pos2(*line_x.end(), top + pixel),
        )
    };
    // Keep the axis ID used by the native wheel adapter, including page-sized input.
    ui.interact(rect, output.id.with(1_usize), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let thumb = egui::Rect::from_center_size(
        egui::pos2(
            rect.center().x,
            rect.top() + thumb_height * 0.5 + fraction * (rect.height() - thumb_height),
        ),
        egui::vec2(line_x.end() - line_x.start(), thumb_height),
    );
    let thumb_background = painter.add(egui::Shape::Noop);
    let mut target = None;
    let mut hover = None;
    let mut hint = None;
    let mut dragging = false;
    let mut last_label_bottom = f32::NEG_INFINITY;
    for (index, month) in months.iter().enumerate() {
        let start = fraction_at(maximum, month.offset);
        let date_y =
            |offset| rect.top() + rect.height() * fraction_at(output.content_size.y, offset);
        let bottom = months
            .get(index + 1)
            .map_or(rect.bottom(), |month| date_y(month.offset));
        let band = egui::Rect::from_min_max(
            egui::pos2(rect.left(), date_y(month.offset)),
            egui::pos2(rect.right(), bottom),
        );
        let response = ui.interact(
            band,
            output.id.with(("month", month.date)),
            egui::Sense::click_and_drag(),
        );
        let label = month.label(language);
        if response.enabled() && (response.is_pointer_button_down_on() || response.dragged()) {
            let frame = ui.ctx().cumulative_frame_nr();
            ui.data_mut(|data| data.insert_temp(drag_id, (response.id, frame)));
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), &label)
        });
        if response.clicked() {
            let mut fraction = start;
            if ui.input(|input| input.pointer.primary_clicked())
                && let Some(pointer) = response.interact_pointer_pos()
            {
                fraction = pointer_fraction(pointer);
            }
            target = Some(fraction);
        }
        // The rail is direct manipulation: do not wait for egui's click/drag
        // distance threshold before following the owned primary pointer.
        if (response.is_pointer_button_down_on() || response.dragged() || response.drag_stopped())
            && ui.input(|input| input.pointer.primary_down() || input.pointer.primary_released())
            && let Some(pointer) = response.interact_pointer_pos()
        {
            target = Some(pointer_fraction(pointer));
            dragging = true;
        }
        let year = month.date.map(|date| date.0);
        // Newest months are above older ones. A year's chronological start is
        // the lower boundary of its oldest populated month, not its first row.
        let year_start = months
            .get(index + 1)
            .is_none_or(|next| next.date.map(|date| date.0) != year);
        let label = year_start.then(|| {
            painter.layout_no_wrap(
                year.map_or_else(|| "?".into(), |year| year.to_string()),
                egui::FontId::proportional(11.0),
                chrome::MUTED,
            )
        });
        if let Some(label) =
            label.filter(|label| bottom - label.size().y >= last_label_bottom.max(rect.top()))
        {
            let origin = egui::pos2(*line_x.end() - label.size().x, bottom - label.size().y);
            painter.galley(origin, label, chrome::MUTED);
            last_label_bottom = bottom + 2.0;
        } else {
            painter.rect_filled(
                line(*line_x.end() - 10.0 * pixel, bottom),
                0.0,
                chrome::MUTED,
            );
        }
        if response.hovered() || (hover.is_none() && response.has_focus()) {
            hover = Some(
                response
                    .hover_pos()
                    .map_or_else(|| pointer_fraction(band.center()), pointer_fraction),
            );
            if response.hovered() {
                hint = hover;
            }
        }
    }
    let thumb_response = ui.interact(
        thumb,
        output.id.with("thumb"),
        egui::Sense::click_and_drag(),
    );
    if thumb_response.hovered()
        && let Some(pointer) = thumb_response.hover_pos()
    {
        hover = Some(pointer_fraction(pointer));
        hint = hover;
    }
    let anchor_id = output.id.with("thumb-anchor");
    if thumb_response.enabled()
        && (thumb_response.is_pointer_button_down_on()
            || thumb_response.dragged()
            || thumb_response.drag_stopped())
    {
        if let Some(pointer) = thumb_response.interact_pointer_pos() {
            let anchor = ui.data_mut(|data| {
                *data.get_temp_mut_or_insert_with(anchor_id, || DragAnchor {
                    pointer: pointer.y,
                    fraction,
                })
            });
            target = Some(
                (anchor.fraction
                    + (pointer.y - anchor.pointer) / (rect.height() - thumb_height).max(1.0))
                .clamp(0.0, 1.0),
            );
            dragging = true;
            let frame = ui.ctx().cumulative_frame_nr();
            ui.data_mut(|data| data.insert_temp(drag_id, (thumb_response.id, frame)));
        }
    } else {
        ui.data_mut(|data| data.remove::<DragAnchor>(anchor_id));
    }
    let opacity = ui.ctx().animate_bool_with_time(
        output.id.with("thumb-hover"),
        ui.is_enabled() && (thumb_response.hovered() || dragging || ui.rect_contains_pointer(rect)),
        0.12,
    );
    if opacity > 0.0 {
        painter.set(
            thumb_background,
            egui::Shape::rect_filled(
                thumb,
                0.0,
                egui::Color32::from_rgb(24, 24, 24).gamma_multiply(opacity),
            ),
        );
    }
    if let Some(target) = target.filter(|_| ui.is_enabled()) {
        let now = ui.input(|input| input.time);
        let mut motion = ui
            .data(|data| data.get_temp::<Motion>(motion_id))
            .unwrap_or_else(|| Motion {
                layout,
                fraction,
                target,
                offset: output.state.offset.y,
                maximum,
                time: now,
                pass: ui.ctx().cumulative_pass_nr(),
            });
        motion.target = target;
        motion.time = now;
        ui.data_mut(|data| data.insert_temp(motion_id, motion));
        ui.ctx().request_repaint();
    }
    let y = egui::lerp(travel.clone(), fraction);
    if let Some(hover) = hover.filter(|_| !dragging) {
        painter.rect_filled(
            line(*line_x.start(), egui::lerp(travel.clone(), hover)),
            0.0,
            chrome::MUTED,
        );
    }
    painter.rect_filled(line(*line_x.start(), y), 0.0, chrome::FOREGROUND);
    if ui.is_enabled()
        && !egui::Popup::is_any_open(ui.ctx())
        && ui.input(|input| input.raw.hovered_files.is_empty())
        && let Some(position) = if dragging { Some(fraction) } else { hint }
    {
        let index = months
            .partition_point(|month| {
                month.offset <= offset_at(maximum, position) + output.inner_rect.height() * 0.5
            })
            .saturating_sub(1);
        let anchor = egui::pos2(*line_x.start() - 4.0, egui::lerp(travel, position));
        show_label(ui, output.id, anchor, months[index].label(language));
    }
    crate::wheel_input::record_scroll_area(ui, output);
}

#[cfg(test)]
mod tests;

fn offset_at(maximum: f32, fraction: f32) -> f32 {
    maximum * fraction.clamp(0.0, 1.0)
}

fn fraction_at(maximum: f32, offset: f32) -> f32 {
    if maximum <= 0.0 {
        0.0
    } else {
        (offset / maximum).clamp(0.0, 1.0)
    }
}

fn show_label(ui: &egui::Ui, id: egui::Id, anchor: egui::Pos2, label: String) {
    // Paint-only help cannot steal input from cards beneath it or delay a drag.
    // Month buttons already expose the same labels to accessibility clients.
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        id.with("month-label"),
    ));
    let frame = crate::hover_help::frame(ui.style());
    let galley =
        painter.layout_no_wrap(label, egui::FontId::proportional(12.0), chrome::FOREGROUND);
    let size = galley.size() + frame.total_margin().sum();
    let outer = egui::Rect::from_min_size(anchor - egui::vec2(size.x, size.y * 0.5), size);
    let screen = ui.ctx().content_rect();
    let outer = egui::Rect::from_min_size(
        outer
            .min
            .clamp(screen.min, (screen.max - size).max(screen.min)),
        size,
    );
    let content = outer - frame.total_margin();
    painter.add(frame.paint(content));
    let text_origin = content.min + egui::vec2(0.0, crate::hover_help::text_offset(ui));
    painter.galley(text_origin, galley, chrome::FOREGROUND);
}
