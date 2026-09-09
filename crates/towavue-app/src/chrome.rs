use egui::{Color32, Pos2, Rect, Stroke, Ui};
use winit::window::ResizeDirection;

pub const BACKGROUND: Color32 = Color32::BLACK;
pub const MUTED: Color32 = Color32::from_gray(128);
pub const FOREGROUND: Color32 = Color32::WHITE;
pub const BORDER: Color32 = Color32::from_gray(24);
pub const HOVER: Color32 = Color32::from_gray(76);
pub const TITLE_HEIGHT: f32 = 32.0;
pub const STATUS_HEIGHT: f32 = 30.0;
pub const TAB_HEIGHT: f32 = 26.0;
pub const TAB_CLOSE_WIDTH: f32 = 24.0;
pub const TAB_PADDING: f32 = 10.0;

pub fn style(style: &mut egui::Style) {
    style.visuals.panel_fill = BACKGROUND;
    style.visuals.selection.bg_fill = HOVER;
    style.visuals.selection.stroke = Stroke::new(1.0, FOREGROUND);
    style.visuals.hyperlink_color = FOREGROUND;
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
        visuals.bg_stroke.color = BORDER;
        visuals.expansion = 0.0;
    }
    style.spacing.button_padding = egui::vec2(6.0, 3.0);
}

pub fn bar() -> egui::Frame {
    egui::Frame::NONE
        .fill(BACKGROUND)
        .inner_margin(egui::Margin::symmetric(6, 3))
}

pub fn modal_heading(ui: &mut Ui, title: &str) {
    if ui.ctx().content_rect().height() < 200.0 {
        ui.spacing_mut().item_spacing.y = 2.0;
    }
    ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
        node.set_role(egui::accesskit::Role::Dialog);
        node.set_label(title);
        node.set_modal();
    });
    ui.heading(title);
}

#[derive(Clone, Copy)]
pub enum Icon {
    Close,
    Reading,
    Minimize,
    Maximize,
    Restore,
    CloseWindow,
    Pause,
    Play,
    Waveform,
    ExitFullscreen,
}

impl Icon {
    pub fn text(self) -> egui::RichText {
        let glyph = match self {
            Self::Close => '\u{ea76}',
            Self::Reading => '\u{eaa4}',
            Self::CloseWindow => '\u{eab8}',
            Self::Maximize => '\u{eab9}',
            Self::Minimize => '\u{eaba}',
            Self::Restore => '\u{eabb}',
            Self::Pause => '\u{ead1}',
            Self::Play => '\u{eb2c}',
            Self::Waveform => '\u{eb31}',
            Self::ExitFullscreen => '\u{eb4d}',
        };
        egui::RichText::new(glyph).font(crate::fonts::icon_font())
    }
}

pub fn button(ui: &mut Ui, icon: Icon, label: &str) -> egui::Response {
    let response = ui
        .add_sized([28.0, 24.0], egui::Button::new(icon.text()).frame(false))
        .on_hover_text(label);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response
}

pub fn tab_width(available: f32, count: usize) -> f32 {
    (available / count.max(1) as f32).clamp(72.0, 160.0)
}

pub fn tab_drop_gap(tabs: &[Rect], strip: Rect, pointer: Pos2) -> Option<(usize, f32)> {
    if tabs.is_empty() || strip.width() < 2.0 || !strip.contains(pointer) {
        return None;
    }
    let gap = tabs
        .iter()
        .position(|rect| pointer.x < rect.center().x)
        .unwrap_or(tabs.len());
    let x = tabs.get(gap).map_or(tabs.last()?.right(), Rect::left);
    Some((gap, x.clamp(strip.left() + 1.0, strip.right() - 1.0)))
}

pub fn logo(ui: &Ui, rect: Rect) {
    let rect = Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0));
    let point = |x: f32, y: f32| rect.min + egui::vec2(x, y) * (16.0 / 27.68);
    let stroke = Stroke::new(1.1, FOREGROUND);
    for (a, b) in [
        ((2.17, 2.17), (10.21, 10.21)),
        ((2.17, 25.5), (10.21, 17.47)),
        ((17.47, 10.21), (25.5, 2.17)),
    ] {
        ui.painter()
            .line_segment([point(a.0, a.1), point(b.0, b.1)], stroke);
    }
    for coordinates in [
        [(9.82, 1.0), (5.0, 1.0), (2.2, 2.2), (1.0, 5.0), (1.0, 9.82)],
        [
            (17.47, 1.0),
            (22.68, 1.0),
            (25.5, 2.2),
            (26.68, 5.0),
            (26.68, 10.21),
        ],
        [
            (26.68, 17.47),
            (26.68, 22.68),
            (25.5, 25.5),
            (22.68, 26.68),
            (17.47, 26.68),
        ],
        [
            (1.0, 17.86),
            (1.0, 22.68),
            (2.2, 25.5),
            (5.0, 26.68),
            (9.82, 26.68),
        ],
    ] {
        ui.painter().add(egui::Shape::line(
            coordinates.into_iter().map(|(x, y)| point(x, y)).collect(),
            stroke,
        ));
    }
}

pub fn resize_edge(rect: Rect, position: Pos2) -> Option<ResizeDirection> {
    let left = position.x < rect.left() + 5.0;
    let right = position.x > rect.right() - 5.0;
    let top = position.y < rect.top() + 5.0;
    let bottom = position.y > rect.bottom() - 5.0;
    match (left, right, top, bottom) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (_, true, true, _) => Some(ResizeDirection::NorthEast),
        (true, _, _, true) => Some(ResizeDirection::SouthWest),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, _, _, _) => Some(ResizeDirection::West),
        (_, true, _, _) => Some(ResizeDirection::East),
        (_, _, true, _) => Some(ResizeDirection::North),
        (_, _, _, true) => Some(ResizeDirection::South),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_gaps_follow_centers_and_clip_scrolled_indicators() {
        let tabs = (0..3)
            .map(|i| {
                Rect::from_min_size(egui::pos2(i as f32 * 100.0, 0.0), egui::vec2(100.0, 26.0))
            })
            .collect::<Vec<_>>();
        let strip = Rect::from_min_max(Pos2::ZERO, egui::pos2(300.0, 26.0));
        assert_eq!(
            tab_drop_gap(&tabs, strip, egui::pos2(20.0, 10.0)),
            Some((0, 1.0))
        );
        assert_eq!(
            tab_drop_gap(&tabs, strip, egui::pos2(150.0, 10.0)),
            Some((2, 200.0))
        );
        assert_eq!(
            tab_drop_gap(&tabs, strip, egui::pos2(280.0, 10.0)),
            Some((3, 299.0))
        );
        assert!(tab_drop_gap(&tabs, strip, egui::pos2(20.0, 30.0)).is_none());
        assert!(tab_drop_gap(&[], strip, egui::pos2(20.0, 10.0)).is_none());
        let clipped = Rect::from_min_max(egui::pos2(120.0, 0.0), egui::pos2(280.0, 26.0));
        assert_eq!(
            tab_drop_gap(&tabs, clipped, egui::pos2(125.0, 10.0)),
            Some((1, 121.0))
        );
    }

    #[test]
    fn tabs_share_width_and_resize_only_uses_the_window_edge() {
        assert_eq!(tab_width(600.0, 4), 150.0);
        assert_eq!(tab_width(600.0, 1), 160.0);
        assert_eq!(tab_width(300.0, 10), 72.0);
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(960.0, 576.0));
        assert_eq!(
            resize_edge(rect, egui::pos2(1.0, 1.0)),
            Some(ResizeDirection::NorthWest)
        );
        assert_eq!(
            resize_edge(rect, egui::pos2(959.0, 575.0)),
            Some(ResizeDirection::SouthEast)
        );
        assert_eq!(resize_edge(rect, egui::pos2(400.0, 15.0)), None);
    }
}
