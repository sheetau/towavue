use crate::{chrome, hover_help::HoverHelp};

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
    if months.is_empty() || rect.height() < 12.0 {
        return;
    }
    let maximum = (output.content_size.y - output.inner_rect.height()).max(0.0);
    // Keep the axis ID used by the native wheel adapter, including page-sized input.
    ui.interact(rect, output.id.with(1_usize), egui::Sense::hover());
    let slot = rect.height() / months.len() as f32;
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let mut target = None;
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
            target = Some(month.offset.min(maximum));
            if months.len() == 1
                && response.clicked_by(egui::PointerButton::Primary)
                && let Some(pointer) = response.interact_pointer_pos()
            {
                target = Some(((pointer.y - rect.top()) / rect.height()).clamp(0.0, 1.0) * maximum);
            }
        }
        if response.dragged()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let position = ((pointer.y - rect.top()) / slot).clamp(0.0, months.len() as f32);
            let left = (position.floor() as usize).min(months.len() - 1);
            let end = months
                .get(left + 1)
                .map_or(maximum, |month| month.offset.min(maximum));
            target = Some(egui::lerp(
                months[left].offset.min(maximum)..=end,
                position - left as f32,
            ));
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
        if response.hovered() || response.has_focus() {
            let hover_y = response.hover_pos().map_or(y, |point| point.y);
            painter.hline(rect.x_range(), hover_y, (2.0, chrome::MUTED));
        }
        response.help_text(label);
    }
    if let Some(target) = target
        && ui.is_enabled()
    {
        // Explicit navigation cancels pending focus-scroll animation and momentum.
        output.state = egui::scroll_area::State::default();
        output.state.offset.y = target;
        output.state.store(ui.ctx(), output.id);
        ui.ctx().request_repaint();
    }
    let offset = output.state.offset.y;
    let fraction = if maximum <= 0.0 || offset <= 0.0 {
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
    };
    let y = egui::lerp(rect.top() + 2.0..=rect.bottom() - 2.0, fraction);
    painter.hline(rect.x_range(), y, (2.0, chrome::FOREGROUND));
    crate::wheel_input::record_scroll_area(ui, output);
}
