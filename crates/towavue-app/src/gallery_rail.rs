use crate::chrome;

#[derive(Clone, Copy)]
struct Position {
    layout: egui::Id,
    offset: f32,
    fraction: f32,
}

pub(super) struct Month {
    pub date: Option<(u16, u16)>,
    pub offset: f32,
}

impl Month {
    fn label(&self) -> String {
        const NAMES: [&str; 12] = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        match self.date {
            Some((year, month)) => format!("{} {year}", NAMES[usize::from(month) - 1]),
            None => "Date unknown".into(),
        }
    }
}

// Equal slots expose only populated months, including months sharing a card row.
// Actual row offsets determine navigation; missing calendar intervals take no space.
pub(super) fn show(
    ui: &mut egui::Ui,
    output: &mut egui::scroll_area::ScrollAreaOutput<Vec<Month>>,
    rect: egui::Rect,
) {
    let months = &output.inner;
    let position_id = output.id.with("gallery-rail-position");
    if months.is_empty() || rect.height() < 12.0 {
        ui.data_mut(|data| data.remove::<Position>(position_id));
        return;
    }
    let maximum = (output.content_size.y - output.inner_rect.height()).max(0.0);
    let layout = months
        .iter()
        .fold(output.id.with(maximum.to_bits()), |id, month| {
            id.with((month.date, month.offset.to_bits()))
        });
    let travel = rect.top() + 2.0..=rect.bottom() - 2.0;
    let pointer_fraction = |point: egui::Pos2| {
        ((point.y - *travel.start()) / (travel.end() - travel.start())).clamp(0.0, 1.0)
    };
    let line_x = rect.left() + rect.width() * 0.075..=rect.right() - rect.width() * 0.075;
    // Keep the axis ID used by the native wheel adapter, including page-sized input.
    ui.interact(rect, output.id.with(1_usize), egui::Sense::hover());
    let slot = rect.height() / months.len() as f32;
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let mut target = None;
    let mut hover = None;
    let mut hint = None;
    let mut dragging = false;
    let mut last_label_bottom = f32::NEG_INFINITY;
    for (index, month) in months.iter().enumerate() {
        let band = egui::Rect::from_min_size(
            rect.min + egui::vec2(0.0, slot * index as f32),
            egui::vec2(rect.width(), slot),
        );
        let response = ui.interact(
            band,
            output.id.with(("month", month.date)),
            egui::Sense::click_and_drag(),
        );
        let label = month.label();
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), &label)
        });
        if response.clicked() {
            let mut fraction = index as f32 / months.len() as f32;
            if ui.input(|input| input.pointer.primary_clicked())
                && let Some(pointer) = response.interact_pointer_pos()
            {
                fraction = pointer_fraction(pointer);
            }
            target = Some(fraction);
        }
        if (response.dragged() || response.drag_stopped())
            && let Some(pointer) = response.interact_pointer_pos()
        {
            target = Some(pointer_fraction(pointer));
            dragging = true;
        }
        let y = band.center().y;
        let year = month.date.map(|date| date.0);
        let new_year = index == 0 || months[index - 1].date.map(|date| date.0) != year;
        if new_year && y - 7.0 >= last_label_bottom.max(rect.top()) && y + 7.0 <= rect.bottom() {
            painter.text(
                band.center(),
                egui::Align2::CENTER_CENTER,
                year.map_or_else(|| "?".into(), |year| year.to_string()),
                egui::FontId::proportional(11.0),
                chrome::MUTED,
            );
            last_label_bottom = y + 9.0;
        } else {
            painter.circle_filled(band.center(), 1.5, chrome::MUTED);
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
    if let Some(fraction) = target
        && ui.is_enabled()
    {
        // Explicit navigation cancels pending focus-scroll animation and momentum.
        output.state = egui::scroll_area::State::default();
        output.state.offset.y = offset_at(months, maximum, fraction);
        output.state.store(ui.ctx(), output.id);
        // Several months can share a row or the clamped bottom offset. Remember
        // the chosen rail position until scrolling or layout changes disambiguate it.
        ui.data_mut(|data| {
            data.insert_temp(
                position_id,
                Position {
                    layout,
                    offset: output.state.offset.y,
                    fraction,
                },
            )
        });
        ui.ctx().request_repaint();
    }
    let offset = output.state.offset.y;
    let remembered = ui
        .data(|data| data.get_temp::<Position>(position_id))
        .filter(|position| position.layout == layout && position.offset == offset);
    if remembered.is_none() {
        ui.data_mut(|data| data.remove::<Position>(position_id));
    }
    let fraction = remembered.map_or_else(
        || fraction_at(months, maximum, offset),
        |position| position.fraction,
    );
    let y = egui::lerp(travel.clone(), fraction);
    if let Some(hover) = hover.filter(|_| !dragging) {
        painter.hline(
            line_x.clone(),
            egui::lerp(travel.clone(), hover),
            (2.0, chrome::MUTED),
        );
    }
    painter.hline(line_x.clone(), y, (2.0, chrome::FOREGROUND));
    if ui.is_enabled()
        && !egui::Popup::is_any_open(ui.ctx())
        && ui.input(|input| input.raw.hovered_files.is_empty())
        && let Some(position) = if dragging { Some(fraction) } else { hint }
    {
        let index = ((position * months.len() as f32) as usize).min(months.len() - 1);
        let anchor = egui::pos2(*line_x.start() - 4.0, egui::lerp(travel, position));
        show_label(ui, output.id, anchor, months[index].label());
    }
    crate::wheel_input::record_scroll_area(ui, output);
}

#[cfg(test)]
mod tests;

fn offset_at(months: &[Month], maximum: f32, fraction: f32) -> f32 {
    let position = fraction * months.len() as f32;
    let left = (position as usize).min(months.len() - 1);
    let end = months
        .get(left + 1)
        .map_or(maximum, |month| month.offset.min(maximum));
    egui::lerp(
        months[left].offset.min(maximum)..=end,
        position - left as f32,
    )
}

fn fraction_at(months: &[Month], maximum: f32, offset: f32) -> f32 {
    if maximum <= 0.0 || offset <= 0.0 {
        0.0
    } else if offset >= maximum {
        1.0
    } else if months.len() == 1 {
        offset / maximum
    } else {
        let left = months
            .partition_point(|month| month.offset <= offset)
            .saturating_sub(1);
        let start = months[left].offset.min(maximum);
        let end = months
            .get(left + 1)
            .map_or(maximum, |month| month.offset.min(maximum));
        let within = if end > start {
            (offset - start) / (end - start)
        } else {
            0.0
        };
        (left as f32 + within) / months.len() as f32
    }
}

fn show_label(ui: &egui::Ui, id: egui::Id, anchor: egui::Pos2, label: String) {
    // Paint-only help cannot steal input from cards beneath it or delay a drag.
    // Month buttons already expose the same labels to accessibility clients.
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        id.with("month-label"),
    ));
    let frame = egui::Frame::popup(ui.style());
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
    painter.galley(content.min, galley, chrome::FOREGROUND);
}
