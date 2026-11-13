use super::*;
use towavue_runtime_windows::{CaptionAction, CaptionButton};

pub const CAPTION_BUTTON_WIDTH: f32 = 36.0;

pub fn caption_controls(ui: &Ui, buttons: &[CaptionButton]) -> Vec<CaptionAction> {
    let mut actions = Vec::new();
    for button in buttons {
        let id = egui::Id::new(("native-caption", button.action));
        let enabled = ui.is_enabled() && button.enabled;
        let bounds = button.bounds / ui.ctx().pixels_per_point();
        paint_caption_button(ui, button, bounds, button.enabled);
        ui.ctx().accesskit_node_builder(id, |node| {
            node.set_role(egui::accesskit::Role::Button);
            node.set_label(button.label);
            node.set_bounds(egui::accesskit::Rect::new(
                bounds.left().into(),
                bounds.top().into(),
                bounds.right().into(),
                bounds.bottom().into(),
            ));
            if enabled {
                node.add_action(egui::accesskit::Action::Click);
            } else {
                node.set_disabled();
            }
        });
        // Win32 owns pointer gestures; egui owns pixels and semantic actions.
        // Avoid a second click target or extra keyboard focus stops.
        ui.input_mut(|input| {
            input.consume_accesskit_action_requests(id, |request| {
                if request.action == egui::accesskit::Action::Click {
                    if enabled {
                        actions.push(button.action);
                    }
                    true
                } else {
                    false
                }
            });
        });
    }
    actions
}

fn paint_caption_button(ui: &Ui, button: &CaptionButton, bounds: Rect, enabled: bool) {
    // Native controls remain visible while a product modal disables its parent UI.
    let painter = ui
        .ctx()
        .layer_painter(ui.layer_id())
        .with_clip_rect(bounds.intersect(ui.ctx().viewport_rect()));
    let active = enabled && button.hovered;
    let close = button.action == CaptionAction::Close;
    let mut foreground = if enabled { FOREGROUND } else { DISABLED };
    if active {
        let background = if close {
            foreground = Color32::WHITE;
            let color = Color32::from_rgb(232, 17, 32);
            if button.pressed {
                foreground = foreground.gamma_multiply(0.8);
                color.gamma_multiply(0.8)
            } else {
                color
            }
        } else if button.pressed {
            KEY_ACTIVE
        } else {
            HOVER
        };
        painter.rect_filled(bounds, 0.0, background);
    }
    let font = egui::FontId::new(10.0, egui::FontFamily::Name("windows-caption".into()));
    // These are Zed's Segoe Fluent Icons / MDL2 codepoints, without a substitute path.
    let glyph = match button.action {
        CaptionAction::Minimize => '\u{e921}',
        CaptionAction::ToggleMaximize if button.maximized => '\u{e923}',
        CaptionAction::ToggleMaximize => '\u{e922}',
        CaptionAction::Close => '\u{e8bb}',
    };
    if ui
        .ctx()
        .fonts(|fonts| fonts.definitions().families.contains_key(&font.family))
    {
        painter.text(
            bounds.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            font,
            foreground,
        );
    }
}

#[cfg(test)]
mod tests;
