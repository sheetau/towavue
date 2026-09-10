use towavue_core::{ImageResize, ResampleFilter};

pub struct ResizeDialog {
    width: String,
    height: String,
    ratio: f64,
    keep_ratio: bool,
    filter: ResampleFilter,
    first_frame: bool,
    step: u32,
}

impl ResizeDialog {
    pub fn new(size: (u32, u32)) -> Self {
        Self {
            width: size.0.to_string(),
            height: size.1.to_string(),
            ratio: f64::from(size.0) / f64::from(size.1),
            keep_ratio: true,
            filter: ResampleFilter::Lanczos,
            first_frame: true,
            step: 1,
        }
    }

    pub(super) fn for_video(size: (u32, u32), aspect: f32) -> Self {
        let mut dialog = Self::new(size);
        dialog.step = 2;
        dialog.ratio *= f64::from(aspect);
        let (width, height) = if aspect >= 1.0 {
            (f64::from(size.0) * f64::from(aspect), f64::from(size.1))
        } else {
            (f64::from(size.0), f64::from(size.1) / f64::from(aspect))
        };
        dialog.width = ((width / 2.0).round().max(8.0) * 2.0).to_string();
        dialog.height = ((height / 2.0).round().max(8.0) * 2.0).to_string();
        dialog
    }

    pub(super) fn value(&self) -> Option<ImageResize> {
        ImageResize::new(
            self.width.parse().ok()?,
            self.height.parse().ok()?,
            self.filter,
        )
    }

    pub(super) fn controls(&mut self, ui: &mut egui::Ui) {
        let previous = (self.width.clone(), self.height.clone(), self.filter);
        ui.label("Width (pixels)");
        let width = text_input(ui, "Width in pixels", &mut self.width);
        if self.first_frame {
            width.request_focus();
            self.first_frame = false;
        }
        if width.changed()
            && self.keep_ratio
            && let Ok(value) = self.width.parse::<u32>()
        {
            self.height = self.round(f64::from(value) / self.ratio);
        }
        ui.label("Height (pixels)");
        let height = text_input(ui, "Height in pixels", &mut self.height);
        if height.changed()
            && self.keep_ratio
            && let Ok(value) = self.height.parse::<u32>()
        {
            self.width = self.round(f64::from(value) * self.ratio);
        }
        if ui
            .checkbox(&mut self.keep_ratio, "Keep aspect ratio")
            .changed()
            && self.keep_ratio
            && let Ok(value) = self.width.parse::<u32>()
        {
            self.height = self.round(f64::from(value) / self.ratio);
        }
        egui::ComboBox::from_label("Resampling filter")
            .selected_text(filter_name(self.filter))
            .show_ui(ui, |ui| {
                for filter in [
                    ResampleFilter::Nearest,
                    ResampleFilter::Bilinear,
                    ResampleFilter::Bicubic,
                    ResampleFilter::Lanczos,
                ] {
                    if ui
                        .selectable_value(&mut self.filter, filter, filter_name(filter))
                        .clicked()
                    {
                        ui.close();
                    }
                }
            });
        if previous != (self.width.clone(), self.height.clone(), self.filter) {
            ui.ctx().request_repaint();
        }
    }

    fn round(&self, value: f64) -> String {
        ((value / f64::from(self.step)).round() * f64::from(self.step)).to_string()
    }

    pub fn show(&mut self, context: &egui::Context) -> Option<Option<ImageResize>> {
        let mut action = None;
        let modal = egui::Modal::new("resize-image".into()).show(context, |ui| {
            ui.set_width((context.content_rect().width() - 32.0).clamp(1.0, 360.0));
            crate::chrome::modal_heading(ui, "Resize / resample image");
            ui.label("Original file is kept. Apply adds one undoable edit.");
            self.controls(ui);
            let value = self.value();
            if value.is_none() {
                ui.label("Use 1–16384 pixels per side, up to 128 Mi pixels.");
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(value.is_some(), egui::Button::new("Apply resize"))
                    .clicked()
                {
                    action = Some(value);
                }
                if ui.button("Cancel").clicked() {
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

pub(super) fn text_input(ui: &mut egui::Ui, label: &str, value: &mut String) -> egui::Response {
    use egui::accesskit::{Action, ActionData, TreeId};
    let id = ui.make_persistent_id(label);
    let mut changed = false;
    if ui.is_enabled() {
        ui.input_mut(|input| {
            input.events.retain(|event| {
                if let egui::Event::AccessKitActionRequest(request) = event
                    && request.target_tree == TreeId::ROOT
                    && request.target_node == id.accesskit_id()
                    && request.action == Action::SetValue
                    && let Some(ActionData::Value(text)) = &request.data
                {
                    *value = text.to_string();
                    changed = true;
                    return false;
                }
                true
            })
        });
    }
    let mut response = ui.add(egui::TextEdit::singleline(value).id(id).hint_text(label));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, ui.is_enabled(), label)
    });
    ui.ctx()
        .accesskit_node_builder(id, |node| node.add_action(Action::SetValue));
    if changed {
        response.mark_changed();
    }
    response
}

fn filter_name(filter: ResampleFilter) -> &'static str {
    match filter {
        ResampleFilter::Nearest => "Nearest",
        ResampleFilter::Bilinear => "Bilinear",
        ResampleFilter::Bicubic => "Bicubic",
        ResampleFilter::Lanczos => "Lanczos",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_dimensions_validate_both_edges_and_total_area() {
        let mut dialog = ResizeDialog::new((600, 800));
        assert_eq!(dialog.value().expect("initial").size(), (600, 800));
        assert_eq!(dialog.filter, ResampleFilter::Lanczos);
        for (width, height) in [
            ("0", "1"),
            ("-1", "10"),
            ("abc", "10"),
            ("1", ""),
            ("16385", "1"),
            ("16384", "16384"),
        ] {
            dialog.width = width.into();
            dialog.height = height.into();
            assert!(dialog.value().is_none(), "{width}x{height}");
        }
        dialog.width = "16384".into();
        dialog.height = "8192".into();
        assert!(dialog.value().is_some());
    }

    #[test]
    fn resize_modal_keyboard_ratio_accessible_apply_and_escape_cancel() {
        let context = crate::fonts::test_context();
        context.enable_accesskit();
        let mut dialog = ResizeDialog::new((600, 800));
        let frame = |dialog: &mut ResizeDialog, events| {
            let mut action = None;
            let output = context.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    action = dialog.show(ui.ctx());
                },
            );
            (
                action,
                output.platform_output.accesskit_update.expect("tree"),
            )
        };
        frame(&mut dialog, vec![]);
        let (_, tree) = frame(
            &mut dialog,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        ctrl: true,
                        command: true,
                        ..egui::Modifiers::NONE
                    },
                },
                egui::Event::Text("300".into()),
            ],
        );
        assert_eq!(dialog.value().expect("locked ratio").size(), (300, 400));
        let apply = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Apply resize"))
            .map(|(id, _)| *id)
            .expect("accessible apply");
        let (action, _) = frame(
            &mut dialog,
            vec![egui::Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Click,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: apply,
                    data: None,
                },
            )],
        );
        assert_eq!(action.flatten().expect("apply").size(), (300, 400));
        let (_, tree) = frame(&mut dialog, vec![]);
        let width = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Width in pixels"))
            .map(|(id, _)| *id)
            .expect("width editor");
        let set_width = |value: &str| {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::SetValue,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: width,
                data: Some(egui::accesskit::ActionData::Value(value.into())),
            })
        };
        frame(&mut dialog, vec![set_width("450")]);
        assert_eq!(dialog.value().expect("accessible ratio").size(), (450, 600));
        let (_, tree) = frame(&mut dialog, vec![set_width("0")]);
        assert!(dialog.value().is_none());
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Apply resize") && node.is_disabled())
        );
        let (action, _) = frame(
            &mut dialog,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(action, Some(None));
    }
}
