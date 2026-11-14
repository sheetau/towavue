//! Shared egui style recipes; control-specific geometry stays with its widget.
use super::tokens::*;
use egui::{Stroke, Ui};

pub fn configure_input(context: &egui::Context) {
    // Native monitor DPI still determines pixels per point. Application input
    // may zoom media, but must never change the GUI's independent zoom factor.
    context.options_mut(|options| options.zoom_with_keyboard = false);
    context.set_zoom_factor(1.0);
}

pub fn style(style: &mut egui::Style) {
    style.interaction.selectable_labels = false;
    #[cfg(debug_assertions)]
    {
        // Different tab controls can legitimately replace each other at the same rect.
        style.debug.warn_if_rect_changes_id = false;
    }
    style.visuals.panel_fill = BACKGROUND;
    style.visuals.window_fill = FLOATING_BACKGROUND;
    style.visuals.window_stroke = Stroke::NONE;
    style.visuals.window_corner_radius = egui::CornerRadius::same(RADIUS_SM);
    style.visuals.menu_corner_radius = egui::CornerRadius::same(RADIUS_SM);
    for (text, size) in [
        (egui::TextStyle::Small, TEXT_SM),
        (egui::TextStyle::Body, TEXT_LG),
        (egui::TextStyle::Button, TEXT_LG),
        (egui::TextStyle::Heading, TEXT_LG),
        (egui::TextStyle::Monospace, TEXT_MD),
    ] {
        style
            .text_styles
            .insert(text, egui::FontId::proportional(size));
    }
    style.visuals.extreme_bg_color = BACKGROUND;
    style.visuals.text_edit_bg_color = Some(BACKGROUND);
    style.visuals.text_cursor.stroke.color = FOREGROUND;
    style.visuals.faint_bg_color = BORDER;
    style.visuals.weak_text_color = Some(MUTED);
    style.visuals.selection.bg_fill = TRACK;
    style.visuals.selection.stroke = Stroke::new(1.0, FOREGROUND);
    style.spacing.scroll.bar_width = 5.0;
    style.spacing.scroll.dormant_background_opacity = 0.0;
    style.spacing.scroll.active_background_opacity = 0.0;
    style.spacing.scroll.interact_background_opacity = 0.45;
    style.spacing.scroll.dormant_handle_opacity = 0.3;
    style.spacing.scroll.active_handle_opacity = 0.3;
    style.spacing.scroll.interact_handle_opacity = 1.0;
    style.visuals.window_shadow.offset = [0, 8];
    style.visuals.window_shadow.blur = 18;
    style.visuals.window_shadow.color = TRANSLUCENT_BLACK;
    style.visuals.popup_shadow.offset = [0, 4];
    style.visuals.popup_shadow.blur = 10;
    style.visuals.popup_shadow.color = TRANSLUCENT_BLACK;
    style.visuals.hyperlink_color = FOREGROUND;
    style.visuals.warn_fg_color = FOREGROUND;
    style.visuals.error_fg_color = FOREGROUND;
    style.visuals.code_bg_color = SURFACE;
    style.visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    for (visuals, foreground, background) in [
        (&mut style.visuals.widgets.noninteractive, MUTED, BORDER),
        (&mut style.visuals.widgets.inactive, MUTED, BORDER),
        (&mut style.visuals.widgets.hovered, FOREGROUND, HOVER),
        (&mut style.visuals.widgets.active, FOREGROUND, HOVER),
        (&mut style.visuals.widgets.open, FOREGROUND, HOVER),
    ] {
        visuals.fg_stroke.color = foreground;
        visuals.bg_fill = background;
        visuals.weak_bg_fill = background;
        visuals.bg_stroke = Stroke::new(
            1.0,
            if foreground == FOREGROUND {
                DISABLED
            } else {
                BORDER
            },
        );
        visuals.expansion = 0.0;
    }
    style.spacing.button_padding = egui::vec2(6.0, 3.0);
    style.spacing.menu_margin = egui::Margin::same(MENU_PADDING);
}

pub fn flat_buttons(ui: &mut Ui) {
    flat_widget_visuals(&mut ui.visuals_mut().widgets);
}

fn flat_widget_visuals(widgets: &mut egui::style::Widgets) {
    // egui subtracts the themed stroke width from button padding before local
    // stroke overrides. Clear it for every state to keep flat content stationary.
    for visuals in [
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
        &mut widgets.noninteractive,
    ] {
        visuals.bg_stroke = egui::Stroke::NONE;
        visuals.corner_radius = egui::CornerRadius::same(RADIUS_XS);
        visuals.expansion = 0.0;
    }
}

/// Shared menu geometry, also used by combo popups and context menus.
pub fn menu_style(style: &mut egui::Style) {
    crate::scroll_style::control_style(style);
    egui::containers::menu::menu_style(style);
    // Popup edges keep their actual frame color. Translucent scroll fades mix
    // with clipped labels/highlights and can leave a lighter band at either end.
    style.spacing.scroll.fade.strength = 0.0;
    style.visuals.disabled_alpha = 1.0;
    style.visuals.widgets.noninteractive.fg_stroke.color = DISABLED;
    style.visuals.window_stroke = Stroke::NONE;
    style.override_font_id = Some(egui::FontId::proportional(TEXT_MD));
    for text in [egui::TextStyle::Body, egui::TextStyle::Button] {
        style
            .text_styles
            .insert(text, egui::FontId::proportional(TEXT_MD));
    }
    style.spacing.interact_size.y = CONTROL_HEIGHT;
    style.spacing.button_padding = egui::vec2(5.0, 2.0);
    style.spacing.item_spacing.y = ITEM_GAP;
    flat_widget_visuals(&mut style.visuals.widgets);
}

pub fn menu(ui: &mut Ui) {
    menu_style(ui.style_mut());
}

/// Frameless controls painted directly over the black content surface.
pub fn surface_hover(ui: &mut Ui) {
    let widgets = &mut ui.visuals_mut().widgets;
    for visual in [&mut widgets.hovered, &mut widgets.active, &mut widgets.open] {
        visual.bg_fill = SURFACE_HOVER;
        visual.weak_bg_fill = SURFACE_HOVER;
    }
}
