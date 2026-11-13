use super::*;

/// A filled selector whose popup rows align with the button, outside frame padding.
pub struct ComboBox {
    id: egui::IdSalt,
    text: egui::WidgetText,
    width: Option<f32>,
    truncate: bool,
}

impl ComboBox {
    pub fn from_id_salt(id: impl egui::AsIdSalt) -> Self {
        Self {
            id: egui::IdSalt::new(id),
            text: Default::default(),
            width: None,
            truncate: false,
        }
    }
    pub fn selected_text(mut self, text: impl Into<egui::WidgetText>) -> Self {
        self.text = text.into();
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    pub fn truncate(mut self) -> Self {
        self.truncate = true;
        self
    }

    pub(super) fn show<R>(
        self,
        ui: &mut Ui,
        contents: impl FnOnce(&mut Ui) -> R,
    ) -> egui::InnerResponse<Option<R>> {
        let id = ui.make_persistent_id(self.id);
        let popup_id = id.with("popup");
        let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
        let padding = ui.spacing().button_padding;
        let minimum = self.width.unwrap_or(ui.spacing().combo_width);
        let text_width = (minimum - padding.x * 2.0 - ICON_SIZE - f32::from(PADDING)).max(0.0);
        let label = self.text.text().to_owned();
        let galley = self.text.into_galley(
            ui,
            Some(if self.truncate {
                egui::TextWrapMode::Truncate
            } else {
                egui::TextWrapMode::Extend
            }),
            text_width,
            egui::TextStyle::Button,
        );
        let width = minimum.max(galley.size().x + padding.x * 2.0 + ICON_SIZE + f32::from(PADDING));
        let height = CONTROL_HEIGHT.max(galley.size().y.max(ICON_SIZE) + padding.y * 2.0);
        let (_, rect) = ui.allocate_space(egui::vec2(width, height));
        let response = ui.interact(rect, id, egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, response.enabled(), &label)
        });
        let visuals = if open {
            &ui.visuals().widgets.open
        } else {
            ui.style().interact(&response)
        };
        ui.painter()
            .rect_filled(rect, INPUT_RADIUS, visuals.weak_bg_fill);
        ui.painter().galley(
            egui::pos2(
                rect.left() + padding.x,
                rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            visuals.text_color(),
        );
        ui.painter().text(
            egui::pos2(rect.right() - padding.x - ICON_SIZE * 0.5, rect.center().y),
            egui::Align2::CENTER_CENTER,
            "\u{eab4}",
            crate::fonts::icon_font(),
            visuals.fg_stroke.color,
        );
        let margin = f32::from(MENU_PADDING);
        let inner = egui::Popup::menu(&response)
            .id(popup_id)
            .anchor(rect.expand2(egui::vec2(margin, 0.0)))
            .align_alternatives(&[egui::RectAlign::TOP_START])
            .gap(2.0)
            .width(width + 2.0 * margin)
            .style(menu_style)
            .show(|ui| {
                ui.set_width(width);
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                egui::ScrollArea::vertical()
                    .max_height(ui.spacing().combo_height)
                    .show(ui, contents)
                    .inner
            })
            .map(|result| result.inner);
        egui::InnerResponse { inner, response }
    }
}
