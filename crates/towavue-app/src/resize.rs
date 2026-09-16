use crate::scroll_style::ScrollAreaStyle;
use towavue_core::{ImageResize, ResampleFilter};

#[cfg(test)]
mod gpu;

pub struct ResizeDialog {
    width: String,
    height: String,
    ratio: f64,
    keep_ratio: bool,
    filter: ResampleFilter,
    first_frame: bool,
    focused_control: Option<egui::Id>,
    step: u32,
    frame_count: usize,
}

impl ResizeDialog {
    pub fn new(size: (u32, u32), frame_count: usize) -> Self {
        Self {
            width: size.0.to_string(),
            height: size.1.to_string(),
            ratio: f64::from(size.0) / f64::from(size.1),
            keep_ratio: true,
            filter: ResampleFilter::Lanczos,
            first_frame: true,
            focused_control: None,
            step: 1,
            frame_count,
        }
    }

    pub(super) fn for_video(size: (u32, u32), aspect: f32) -> Self {
        let mut dialog = Self::new(size, 1);
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
        let value = ImageResize::new(
            self.width.parse().ok()?,
            self.height.parse().ok()?,
            self.filter,
        )?;
        let (width, height) = value.size();
        let frame_bytes = u64::from(width) * u64::from(height) * 4;
        (self.frame_count <= (512 * 1024 * 1024 / frame_bytes) as usize).then_some(value)
    }

    pub(super) fn controls(&mut self, ui: &mut egui::Ui) {
        let previous = (self.width.clone(), self.height.clone(), self.filter);
        ui.label("Width (pixels)");
        let width = text_input(ui, "Width in pixels", &mut self.width);
        if self.first_frame {
            width.request_focus();
            self.first_frame = false;
        }
        let width = self.reveal_focus(width);
        if width.changed()
            && self.keep_ratio
            && let Ok(value) = self.width.parse::<u32>()
        {
            self.height = self.round(f64::from(value) / self.ratio);
        }
        ui.label("Height (pixels)");
        let height = text_input(ui, "Height in pixels", &mut self.height);
        let height = self.reveal_focus(height);
        if height.changed()
            && self.keep_ratio
            && let Ok(value) = self.height.parse::<u32>()
        {
            self.width = self.round(f64::from(value) * self.ratio);
        }
        let ratio = ui
            .checkbox(&mut self.keep_ratio, "Keep aspect ratio")
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if self.reveal_focus(ratio).changed()
            && self.keep_ratio
            && let Ok(value) = self.width.parse::<u32>()
        {
            self.height = self.round(f64::from(value) / self.ratio);
        }
        let filter = egui::ComboBox::from_label("Resampling filter")
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
        self.reveal_focus(
            filter
                .response
                .on_hover_cursor(egui::CursorIcon::PointingHand),
        );
        if previous != (self.width.clone(), self.height.clone(), self.filter) {
            ui.ctx().request_repaint();
        }
    }

    fn round(&self, value: f64) -> String {
        ((value / f64::from(self.step)).round() * f64::from(self.step)).to_string()
    }

    pub(super) fn reveal_focus(&mut self, response: egui::Response) -> egui::Response {
        if response.has_focus() && self.focused_control != Some(response.id) {
            // Arrow focus is assigned after layout; gained_focus misses some
            // transitions. Remember the focused control across rendered passes.
            response.scroll_to_me(None);
            self.focused_control = Some(response.id);
        }
        response
    }

    pub fn show(&mut self, context: &egui::Context) -> Option<Option<ImageResize>> {
        let mut action = None;
        let modal = egui::Modal::new("resize-image".into()).show(context, |ui| {
            ui.set_width((context.content_rect().width() - 32.0).clamp(1.0, 360.0));
            egui::ScrollArea::vertical()
                .max_height((context.content_rect().height() - 32.0).max(1.0))
                .min_scrolled_height(1.0)
                .show_styled(ui, |ui| {
                    crate::chrome::modal_heading(ui, "Resize / resample image");
                    ui.label("Original file is kept. Apply adds one undoable edit.");
                    self.controls(ui);
                    let value = self.value();
                    if value.is_none() {
                        ui.label("Use 1–16384 pixels per side, up to 128 Mi pixels.");
                        if self.frame_count > 1 {
                            ui.label(format!(
                                "All {} animation frames must fit within 512 MiB.",
                                self.frame_count
                            ));
                        }
                    }
                    ui.horizontal(|ui| {
                        if self
                            .reveal_focus(
                                ui.add_enabled(value.is_some(), egui::Button::new("Apply resize")),
                            )
                            .clicked()
                        {
                            action = Some(value);
                        }
                        if self.reveal_focus(ui.button("Cancel")).clicked() {
                            action = Some(None);
                        }
                    });
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

fn scroll_on_focus(response: egui::Response) -> egui::Response {
    if response.gained_focus() {
        response.scroll_to_me(None);
    }
    response
}

pub(super) fn text_input(ui: &mut egui::Ui, label: &str, value: &mut String) -> egui::Response {
    text_input_rows(ui, label, value, 1)
}

pub(super) fn multiline_text_input(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
) -> egui::Response {
    text_input_rows(ui, label, value, 3)
}

fn text_input_rows(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    rows: usize,
) -> egui::Response {
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
    let editor = if rows == 1 {
        egui::TextEdit::singleline(value).vertical_align(egui::Align::Center)
    } else {
        egui::TextEdit::multiline(value)
            .desired_rows(rows)
            .desired_width(f32::INFINITY)
            .char_limit(4097)
    };
    let mut response = ui.add(editor.id(id).hint_text(label));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, ui.is_enabled(), label)
    });
    ui.ctx().accesskit_node_builder(id, |node| {
        node.add_action(Action::SetValue);
        if rows > 1 {
            node.set_role(egui::accesskit::Role::MultilineTextInput);
        }
    });
    if changed {
        response.mark_changed();
    }
    scroll_on_focus(response)
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
pub(crate) mod tests {
    use super::*;

    pub(crate) fn assert_centered_input(
        output: &egui::FullOutput,
        rect: egui::Rect,
        text: &str,
        density: f32,
    ) {
        let text_rect = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(shape) = &shape.shape
                    && shape.galley.text() == text
                {
                    Some(egui::Rect::from_min_size(shape.pos, shape.galley.size()))
                } else {
                    None
                }
            })
            .expect("input text or placeholder");
        assert!(
            (text_rect.center().y - rect.center().y).abs() <= 1.0 / density,
            "text centered at {density}x: {text_rect:?} in {rect:?}"
        );
        let (points, stroke) = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::LineSegment { points, stroke } = &shape.shape
                    && points[0].x == points[1].x
                    && rect.contains(points[0])
                    && rect.contains(points[1])
                    && (points[1].y - points[0].y).abs() > 5.0
                {
                    Some((*points, *stroke))
                } else {
                    None
                }
            })
            .expect("painted input caret");
        assert_eq!(stroke.color, egui::Color32::WHITE);
        assert!(
            ((points[0].y + points[1].y) * 0.5 - rect.center().y).abs() <= 1.0 / density,
            "caret centered at {density}x: {points:?} in {rect:?}"
        );
    }

    #[test]
    fn single_line_inputs_center_text_placeholder_and_white_caret() {
        for density in [1.0, 1.25, 2.0] {
            for height in [24.0, 32.0] {
                for value in ["", "Abc12あ"] {
                    let context = crate::fonts::test_context();
                    context.set_pixels_per_point(density);
                    context.global_style_mut(|style| {
                        crate::chrome::style(style);
                        style.visuals.text_cursor.blink = false;
                    });
                    let mut value = value.to_owned();
                    let mut rect = egui::Rect::NOTHING;
                    let mut frame = || {
                        context.run_ui(egui::RawInput::default(), |ui| {
                            let response = ui.add_sized([240.0, height], |ui: &mut egui::Ui| {
                                text_input(ui, "Placeholder", &mut value)
                            });
                            rect = response.rect;
                            response.request_focus();
                        })
                    };
                    frame();
                    let output = frame();
                    assert_centered_input(
                        &output,
                        rect,
                        if value.is_empty() {
                            "Placeholder"
                        } else {
                            &value
                        },
                        density,
                    );
                }
            }
        }
    }

    pub(crate) fn select_all_filters<N: Fn(crate::AppEvent) + Send + Sync + 'static>(
        app: &mut crate::Application<N>,
        check: impl Fn(&crate::Application<N>, ResampleFilter),
    ) {
        use crate::video_rotation::tests::{access, frame, node};
        for filter in [
            ResampleFilter::Nearest,
            ResampleFilter::Bilinear,
            ResampleFilter::Bicubic,
            ResampleFilter::Lanczos,
        ] {
            let tree = frame(app, vec![]);
            let combo = tree
                .nodes
                .iter()
                .find(|(_, node)| node.role() == egui::accesskit::Role::ComboBox)
                .expect("filter selector")
                .0;
            frame(app, vec![access(combo, None)]);
            let tree = frame(app, vec![]);
            assert!(
                egui::Popup::is_any_open(app.ui_context.as_ref().expect("context")),
                "the app must not close a modal's popup on the next frame"
            );
            frame(app, vec![access(node(&tree, filter_name(filter)), None)]);
            frame(app, vec![]);
            frame(app, vec![]);
            check(app, filter);
        }
    }

    #[test]
    fn image_resize_filter_popup_survives_full_app_frames_and_cancel() {
        let Some(root) = crate::tests::isolated_test_root(
            "resize::tests::image_resize_filter_popup_survives_full_app_frames_and_cancel",
        ) else {
            return;
        };
        let context = crate::fonts::test_context();
        context.enable_accesskit();
        let mut app = crate::Application::new(None, |_| {}).expect("app");
        app.ui_context = Some(context);
        let source = root.join("image.png");
        app.tabs
            .open_new(source.clone(), towavue_core::MediaKind::Image);
        app.path = Some(source);
        app.media_kind = Some(towavue_core::MediaKind::Image);
        app.resize_dialog = Some(ResizeDialog::new((64, 48), 1));
        for _ in 0..3 {
            crate::video_rotation::tests::frame(&mut app, vec![]);
        }
        select_all_filters(&mut app, |app, filter| {
            assert_eq!(
                app.resize_dialog
                    .as_ref()
                    .expect("dialog")
                    .value()
                    .expect("size")
                    .filter,
                filter
            )
        });
        app.handle_ui_action(crate::UiAction::FinishResize(None));
        assert!(app.resize_dialog.is_none());
        assert!(app.edits.is_empty());
    }

    #[test]
    fn compact_resize_dialog_keeps_scrolled_actions_inside_the_window() {
        for density in [1.0, 1.5, 2.0] {
            for (size, frame_count) in [
                (egui::vec2(320.0, 240.0), 1),
                (egui::vec2(480.0, 180.0), 1),
                (egui::vec2(320.0, 240.0), 3),
                (egui::vec2(480.0, 180.0), 3),
            ] {
                let context = crate::fonts::test_context();
                context.set_pixels_per_point(density);
                context.enable_accesskit();
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                let mut dialog = ResizeDialog::new(
                    if frame_count == 1 {
                        (600, 800)
                    } else {
                        (8192, 8192)
                    },
                    frame_count,
                );
                let expected = dialog.value();
                let frame = |dialog: &mut ResizeDialog, events| {
                    let mut action = None;
                    let output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            events,
                            ..Default::default()
                        },
                        |ui| action = dialog.show(ui.ctx()),
                    );
                    (
                        action,
                        output.platform_output.accesskit_update.expect("tree"),
                    )
                };
                for _ in 0..3 {
                    frame(&mut dialog, vec![]);
                }
                let modal = context
                    .memory(|memory| memory.area_rect("resize-image"))
                    .expect("modal");
                assert!(
                    screen.contains_rect(modal),
                    "density={density}, size={size:?}, modal={modal:?}"
                );
                for _ in 0..12 {
                    frame(
                        &mut dialog,
                        vec![
                            egui::Event::PointerMoved(screen.center()),
                            egui::Event::MouseWheel {
                                unit: egui::MouseWheelUnit::Point,
                                phase: egui::TouchPhase::Move,
                                delta: egui::vec2(0.0, -150.0),
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                    );
                }
                let (_, tree) = frame(&mut dialog, vec![]);
                for label in ["Apply resize", "Cancel"] {
                    let (_, node) = tree
                        .nodes
                        .iter()
                        .find(|(_, node)| node.label() == Some(label))
                        .expect("button");
                    let bounds = node.bounds().expect("button bounds");
                    assert!(
                        bounds.x0 >= 0.0
                            && bounds.x1 <= f64::from(size.x)
                            && bounds.y0 >= 0.0
                            && bounds.y1 <= f64::from(size.y),
                        "{label}: {bounds:?}"
                    );
                }
                let apply = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some("Apply resize"))
                    .expect("apply")
                    .0;
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
                assert_eq!(
                    action,
                    expected.map(Some),
                    "invalid animation resize stays disabled"
                );
                let cancel = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some("Cancel"))
                    .expect("cancel")
                    .1
                    .bounds()
                    .expect("cancel bounds");
                let pos = egui::pos2(
                    ((cancel.x0 + cancel.x1) / 2.0) as f32,
                    ((cancel.y0 + cancel.y1) / 2.0) as f32,
                );
                for pressed in [true, false] {
                    let (action, _) = frame(
                        &mut dialog,
                        vec![
                            egui::Event::PointerMoved(pos),
                            egui::Event::PointerButton {
                                pos,
                                pressed,
                                button: egui::PointerButton::Primary,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                    );
                    if !pressed {
                        assert_eq!(action, Some(None), "scrolled cancel is clickable");
                    }
                }
            }
        }
    }

    #[test]
    fn compact_resize_keyboard_navigation_keeps_focused_controls_visible() {
        for density in [1.0, 1.5, 2.0] {
            for size in [egui::vec2(480.0, 180.0), egui::vec2(320.0, 240.0)] {
                let mut dialog = ResizeDialog::new((600, 800), 1);
                keyboard_focus_stays_visible(density, size, |context| {
                    dialog.show(context).is_none()
                });
                assert_eq!(dialog.value().expect("unchanged resize").size(), (600, 800));
            }
        }
    }

    pub(crate) fn keyboard_focus_stays_visible(
        density: f32,
        size: egui::Vec2,
        mut show: impl FnMut(&egui::Context) -> bool,
    ) {
        let context = crate::fonts::test_context();
        context.set_pixels_per_point(density);
        context.enable_accesskit();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let mut time = 0.0;
        let mut frame = |events| {
            time += 0.1;
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| assert!(show(ui.ctx()), "navigation must not apply or cancel"),
            );
            output.platform_output.accesskit_update.expect("tree")
        };
        for _ in 0..3 {
            frame(vec![]);
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut filter_seen = false;
        for shift in [false, true] {
            for _ in 0..7 {
                for pressed in [true, false] {
                    frame(vec![egui::Event::Key {
                        key: egui::Key::Tab,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: egui::Modifiers {
                            shift,
                            ..egui::Modifiers::NONE
                        },
                    }]);
                }
                for _ in 0..5 {
                    frame(vec![]);
                }
                let tree = frame(vec![]);
                if let Some((_, node)) = tree.nodes.iter().find(|(id, _)| *id == tree.focus) {
                    filter_seen |= node.role() == egui::accesskit::Role::ComboBox;
                    let label = node.label().unwrap_or("unlabelled control");
                    seen.insert(label.to_owned());
                    let bounds = node.bounds().expect("focused control bounds");
                    assert!(
                        bounds.x0 >= 0.0
                            && bounds.x1 <= f64::from(screen.right())
                            && bounds.y0 >= 0.0
                            && bounds.y1 <= f64::from(screen.bottom()),
                        "density {density}, size {size:?}, shift {shift}, {label}: {bounds:?}"
                    );
                    let focused = context.memory(egui::Memory::focused).expect("focus");
                    let response = context.read_response(focused).expect("focused response");
                    // Scroll clipping and widget bounds can round to adjacent physical pixels.
                    assert!(
                        response.interact_rect.height() >= response.rect.height() - 1.0 / density,
                        "{label} is clipped: {:?} vs {:?}",
                        response.interact_rect,
                        response.rect
                    );
                }
            }
        }
        assert!(filter_seen, "Tab must reach the resampling filter");
        for label in [
            "Width in pixels",
            "Height in pixels",
            "Keep aspect ratio",
            "Apply resize",
            "Cancel",
        ] {
            assert!(seen.contains(label), "Tab must reach {label}: {seen:?}");
        }
        let tree = frame(vec![]);
        let (ratio, _) = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Keep aspect ratio"))
            .expect("ratio control");
        frame(vec![egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Focus,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: *ratio,
                data: None,
            },
        )]);
        for _ in 0..5 {
            frame(vec![]);
        }
        let mut arrow_seen = std::collections::BTreeSet::new();
        let mut arrow_filter_seen = false;
        for key in [
            egui::Key::ArrowDown,
            egui::Key::ArrowDown,
            egui::Key::ArrowRight,
            egui::Key::ArrowLeft,
            egui::Key::ArrowUp,
            egui::Key::ArrowUp,
        ] {
            for pressed in [true, false] {
                frame(vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }]);
            }
            for _ in 0..5 {
                frame(vec![]);
            }
            let focused = context.memory(egui::Memory::focused).expect("arrow focus");
            let tree = frame(vec![]);
            let focus_node = tree
                .nodes
                .iter()
                .find(|(id, _)| *id == tree.focus)
                .expect("focus node");
            arrow_filter_seen |= focus_node.1.role() == egui::accesskit::Role::ComboBox;
            if let Some(label) = focus_node.1.label() {
                arrow_seen.insert(label.to_owned());
            }
            let response = context.read_response(focused).expect("focused response");
            assert!(
                response.interact_rect.height() >= response.rect.height() - 1.0 / density,
                "density {density}, size {size:?}, {key:?}: clipped {:?} vs {:?}",
                response.interact_rect,
                response.rect
            );
        }
        assert!(arrow_filter_seen, "arrows must reach the resampling filter");
        for label in ["Apply resize", "Cancel"] {
            assert!(
                arrow_seen.contains(label),
                "arrows must reach {label}: {arrow_seen:?}"
            );
        }
        // The taller image dialog can fit without scrolling; use the short
        // viewport to require actual wheel movement in both dialog variants.
        if size.y > 180.0 {
            return;
        }
        let tree = frame(vec![]);
        let (cancel, _) = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Cancel"))
            .expect("cancel control");
        frame(vec![egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Focus,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: *cancel,
                data: None,
            },
        )]);
        for _ in 0..5 {
            frame(vec![]);
        }
        let focused = context.memory(egui::Memory::focused).expect("wheel focus");
        let before = context.read_response(focused).expect("response").rect;
        frame(vec![
            egui::Event::PointerMoved(before.center()),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: egui::vec2(0.0, 60.0),
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        for _ in 0..5 {
            frame(vec![]);
        }
        let after = context.read_response(focused).expect("wheel response").rect;
        assert_eq!(context.memory(egui::Memory::focused), Some(focused));
        assert!(
            after.top() - before.top() > 20.0,
            "density {density}, size {size:?}: manual scroll must not snap to unchanged focus: {before:?} -> {after:?}"
        );
    }

    #[test]
    fn animation_resize_budget_disables_apply_and_keeps_boundary_values() {
        assert!(ResizeDialog::new((8192, 8192), 2).value().is_some());
        assert!(ResizeDialog::new((8192, 8192), 3).value().is_none());
        assert!(ResizeDialog::for_video((8192, 8192), 1.0).value().is_some());
        for density in [1.0, 1.5, 2.0] {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            context.enable_accesskit();
            let mut dialog = ResizeDialog::new((8192, 8192), 3);
            let mut action = None;
            let mut frame = |dialog: &mut ResizeDialog, events| {
                let output = context.run_ui(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        action = dialog.show(ui.ctx());
                    },
                );
                output.platform_output.accesskit_update.expect("tree")
            };
            frame(&mut dialog, vec![]);
            let tree = frame(&mut dialog, vec![]);
            let (apply, node) = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some("Apply resize"))
                .expect("apply");
            assert!(node.is_disabled());
            assert!(
                tree.nodes.iter().any(|(_, node)| node.value()
                    == Some("All 3 animation frames must fit within 512 MiB."))
            );
            frame(
                &mut dialog,
                vec![egui::Event::AccessKitActionRequest(
                    egui::accesskit::ActionRequest {
                        action: egui::accesskit::Action::Click,
                        target_tree: egui::accesskit::TreeId::ROOT,
                        target_node: *apply,
                        data: None,
                    },
                )],
            );
            assert!(action.is_none(), "disabled apply cannot start a worker");
            dialog.width = "4096".into();
            dialog.height = "4096".into();
            assert!(
                dialog.value().is_some(),
                "valid smaller size remains available"
            );
        }
    }

    #[test]
    fn resize_dimensions_validate_both_edges_and_total_area() {
        let mut dialog = ResizeDialog::new((600, 800), 1);
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
        let mut dialog = ResizeDialog::new((600, 800), 1);
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
