use crate::scroll_style::ScrollAreaStyle;
use egui::RichText;
use towavue_core::{CommandId, ShortcutBindings};

use crate::chrome;
use crate::hover_help::HoverHelp;

pub(super) fn tab(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    active: bool,
    can_close: bool,
) -> (egui::Response, egui::Response) {
    ui.painter().rect_filled(
        rect,
        3.0,
        if active {
            chrome::BORDER
        } else {
            chrome::BACKGROUND
        },
    );
    let mut label_rect = rect;
    label_rect.max.x -= chrome::TAB_CLOSE_WIDTH;
    let response = ui.put(
        label_rect,
        egui::Button::new(chrome::tab_label("Gallery".into(), active))
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE)
            .truncate()
            .sense(egui::Sense::click_and_drag()),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), "Gallery tab")
    });
    let close_rect = egui::Rect::from_min_max(egui::pos2(label_rect.right(), rect.top()), rect.max);
    let close = ui
        .add_enabled_ui(can_close, |ui| chrome::tab_close(ui, close_rect, false))
        .inner
        .help_text("Close Gallery");
    close.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            close.enabled(),
            "Close tab: Gallery",
        )
    });
    crate::tab_focus::release_pointer_focus(&response);
    crate::tab_focus::release_pointer_button_focus(&response, egui::PointerButton::Middle);
    crate::tab_focus::release_pointer_focus(&close);
    if response.has_focus() || close.has_focus() {
        ui.painter().rect_stroke(
            rect,
            3.0,
            ui.visuals().selection.stroke,
            egui::StrokeKind::Inside,
        );
    }
    (response, close)
}

pub fn show(
    ui: &mut egui::Ui,
    shortcuts: &ShortcutBindings,
    recent: impl FnOnce(&mut egui::Ui),
) -> Option<CommandId> {
    let viewport = ui.available_rect_before_wrap();
    let inset = viewport.shrink(8.0_f32.min(viewport.size().min_elem().max(0.0) * 0.25));
    let gutter_scroll = if ui.is_enabled()
        && ui.rect_contains_pointer(viewport)
        && ui.input(|input| {
            input
                .pointer
                .hover_pos()
                .is_some_and(|p| !inset.contains(p))
        }) {
        ui.input_mut(|input| std::mem::take(&mut input.smooth_scroll_delta.y))
    } else {
        0.0
    };
    let content_style = ui.style().clone();
    let mut scroll_ui = ui.new_child(egui::UiBuilder::new().max_rect(inset));
    ui.advance_cursor_after_rect(viewport);
    let ui = &mut scroll_ui;
    // Only the scrollbar gets the subdued hover palette, not its cards or buttons.
    let color = ui.visuals().widgets.inactive.fg_stroke.color;
    ui.visuals_mut().widgets.hovered.fg_stroke.color = color;
    ui.visuals_mut().widgets.active.fg_stroke.color = color;
    ui.spacing_mut().scroll.interact_background_opacity = 0.3;
    let mut chosen = None;
    let width = (ui.available_width() - 32.0).clamp(0.0, 660.0);
    let top = (ui.available_height() * 0.1).clamp(12.0, 60.0);
    egui::ScrollArea::vertical()
        .id_salt("welcome")
        .auto_shrink([false, false])
        .show_styled(ui, |ui| {
            ui.set_style(content_style);
            if gutter_scroll != 0.0 {
                ui.scroll_with_delta_animation(
                    egui::vec2(0.0, gutter_scroll),
                    egui::style::ScrollAnimation::none(),
                );
            }
            ui.add_space(top);
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                ui.allocate_ui_with_layout(
                    egui::vec2(width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.label(RichText::new("towavue").size(32.0).color(chrome::MUTED));
                        ui.add_space(32.0);
                        ui.label(RichText::new("START").size(12.0).color(chrome::MUTED));
                        ui.add_space(8.0);
                        for (command, label) in [
                            (CommandId::OpenFile, "Open File…"),
                            (CommandId::OpenFolder, "Open Folder…"),
                        ] {
                            let shortcut = shortcuts.label(command, Default::default());
                            let icon = if command == CommandId::OpenFolder {
                                chrome::Icon::OpenFolder
                            } else {
                                chrome::Icon::OpenFile
                            };
                            let response = ui
                                .add_sized(
                                    [width.min(340.0), 30.0],
                                    egui::Button::new((
                                        icon.text(),
                                        RichText::new(label).color(chrome::FOREGROUND),
                                    ))
                                    .frame_when_inactive(false)
                                    .truncate()
                                    .shortcut_text(if width >= 300.0 { &shortcut } else { "" }),
                                )
                                .help_text(format!("{label}  {shortcut}"));
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    ui.is_enabled(),
                                    label,
                                )
                            });
                            if response.clicked() {
                                chosen = Some(command);
                            }
                        }
                        ui.add_space(24.0);
                        ui.label(RichText::new("RECENT").size(12.0).color(chrome::MUTED));
                        ui.add_space(8.0);
                        recent(ui);
                        ui.add_space(16.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new("Drop media files or a folder here to begin.")
                                    .color(chrome::MUTED),
                            )
                            .wrap(),
                        );
                        ui.add_space(16.0);
                    },
                );
            });
        });
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_scrollbar_is_inset_muted_and_keeps_gutter_wheel_input() {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            context.global_style_mut(chrome::style);
            context.global_style_mut(|style| style.animation_time = 0.0);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(480.0, 300.0));
            let original = context.global_style().visuals.widgets.clone();
            let frame = |events| {
                context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        assert!(
                            show(ui, &ShortcutBindings::default(), |ui| {
                                assert_eq!(
                                    ui.visuals().widgets,
                                    original,
                                    "recent cards retain their style"
                                );
                                ui.set_min_height(1200.0);
                            })
                            .is_none()
                        );
                        assert_eq!(
                            ui.visuals().widgets,
                            original,
                            "siblings retain their style"
                        );
                    },
                )
            };
            let bars = |output: &egui::FullOutput| {
                output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect)
                            if rect.rect.left() >= screen.right() - 14.0
                                && rect.rect.width() <= 5.0
                                && rect.rect.height() > 6.0 =>
                        {
                            Some(rect.clone())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            };
            for _ in 0..3 {
                frame(vec![]);
            }
            let idle = bars(&frame(vec![]));
            assert_eq!(idle.len(), 2, "track and handle");
            let track = idle[0].rect;
            for (actual, expected) in [
                (track.top(), 8.0),
                (track.bottom(), 292.0),
                (track.right(), 472.0),
            ] {
                assert!(
                    (actual - expected).abs() <= 1.0 / density,
                    "inset track: {track:?}"
                );
            }
            assert_eq!(idle[0].fill.a(), 0);
            assert!(idle[1].fill.a() > 0);
            let track_point = egui::pos2(track.center().x, track.bottom() - 3.0);
            frame(vec![egui::Event::PointerMoved(track_point)]);
            let hovered_track = bars(&frame(vec![]));
            assert!(hovered_track[0].fill.a() > 0 && hovered_track[0].fill.a() < 128);
            let handle = hovered_track[1].rect;
            frame(vec![egui::Event::PointerMoved(handle.center())]);
            let hovered_handle = bars(&frame(vec![]));
            assert_eq!(
                hovered_handle[1].fill, hovered_track[1].fill,
                "handle hover stays gray"
            );
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            for gutter in [
                egui::pos2(478.0, 150.0),
                egui::pos2(2.0, 150.0),
                egui::pos2(240.0, 2.0),
                egui::pos2(240.0, 298.0),
            ] {
                let before = bars(&frame(vec![]));
                frame(vec![
                    egui::Event::PointerMoved(gutter),
                    button(gutter, true),
                ]);
                frame(vec![button(gutter, false)]);
                let after_click = bars(&frame(vec![]));
                assert_eq!(
                    after_click[1].rect.top(),
                    before[1].rect.top(),
                    "gutter is outside the bar hit region"
                );
                frame(vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -80.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                }]);
                for _ in 0..30 {
                    frame(vec![]);
                }
                let scrolled = bars(&frame(vec![]));
                assert!(
                    scrolled[1].rect.top() > after_click[1].rect.top(),
                    "wheel works in the gutter: {gutter:?}"
                );
            }
            let scrolled = bars(&frame(vec![]));
            let start = scrolled[1].rect.center();
            let end = start + egui::vec2(0.0, 40.0);
            frame(vec![egui::Event::PointerMoved(start), button(start, true)]);
            frame(vec![egui::Event::PointerMoved(end)]);
            frame(vec![button(end, false)]);
            let dragged = bars(&frame(vec![]));
            assert!(
                dragged[1].rect.top() > scrolled[1].rect.top(),
                "handle remains draggable"
            );
        }
    }

    #[test]
    fn welcome_text_uses_ui_font_while_icons_keep_their_own_family() {
        for density in [1.0, 1.25, 2.0] {
            for width in [240.0, 480.0, 960.0] {
                let context = crate::fonts::test_context();
                context.set_pixels_per_point(density);
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 576.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        show(ui, &ShortcutBindings::default(), |ui| {
                            ui.label("No recent files");
                        });
                    },
                );
                let mut wordmark = false;
                let mut icon = false;
                let mut open_file_icon = false;
                for shape in &output.shapes {
                    let egui::Shape::Text(text) = &shape.shape else {
                        continue;
                    };
                    for section in &text.galley.job.sections {
                        let family = &section.format.font_id.family;
                        if family == &crate::fonts::icon_font().family {
                            icon = true;
                            open_file_icon |= text.galley.job.text
                                [section.byte_range.start.0..section.byte_range.end.0]
                                .contains('\u{ea94}');
                        } else {
                            assert_eq!(
                                family,
                                &egui::FontFamily::Proportional,
                                "UI font for {:?} at {density}x / {width}px",
                                text.galley.text()
                            );
                        }
                    }
                    if text.galley.text() == "towavue" {
                        wordmark = true;
                        assert_eq!(text.galley.job.sections[0].format.font_id.size, 32.0);
                    }
                }
                assert!(wordmark && icon, "audit both wordmark and action icons");
                assert!(open_file_icon, "Open File uses the requested Codicon");
            }
        }
    }

    #[test]
    fn welcome_idle_output_keeps_wordmark_and_shortcuts_visible() {
        let mut app = crate::Application::new(None, |_| {}).expect("headless app");
        let context = crate::fonts::test_context();
        context.global_style_mut(crate::chrome::style);
        app.ui_context = Some(context.clone());
        let mut last = egui::FullOutput::default();
        for index in 0..5 {
            last = context.run_ui(
                egui::RawInput {
                    time: Some(index as f64 * 0.1),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut Vec::new()),
            );
            if last.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                > std::time::Duration::from_millis(100)
            {
                break;
            }
        }
        for label in ["towavue", "START", "RECENT", "Ctrl+O", "Ctrl+Shift+O"] {
            assert!(
                text_rect(&last, label).is_some(),
                "{label} is present in idle CPU output"
            );
        }
    }

    #[test]
    fn welcome_keeps_open_actions_together_and_accessible_at_small_sizes() {
        for size in [
            egui::vec2(960.0, 514.0),
            egui::vec2(480.0, 238.0),
            egui::vec2(240.0, 119.0),
        ] {
            let context = crate::fonts::test_context();
            let mut shortcuts = ShortcutBindings::default();
            shortcuts.set(
                CommandId::OpenFile,
                "Ctrl+K Ctrl+O".parse().expect("shortcut"),
            );
            let frame = |events| {
                let mut commands = Vec::new();
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        if let Some(command) = show(ui, &shortcuts, |_| {}) {
                            commands.push(command);
                        }
                    },
                );
                (output, commands)
            };
            for _ in 0..3 {
                frame(vec![]);
            }
            let (output, _) = frame(vec![]);
            let title = text_rect(&output, "towavue").expect("wordmark is visible");
            if size.y > 200.0 {
                let start = text_rect(&output, "START").expect("Start heading");
                assert!((title.left() - start.left()).abs() < 1.0);
                let open = text_rect(&output, "Open File…").expect("Open file");
                assert!(open.left() > title.left() && open.left() < title.left() + 50.0);
                assert!(text_rect(&output, "Ctrl+K Ctrl+O").is_some());
            } else {
                frame(vec![egui::Event::PointerMoved(egui::pos2(120.0, 70.0))]);
                frame(vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -80.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                }]);
                for _ in 0..30 {
                    frame(vec![]);
                }
            }
            for (label, command) in [
                ("Open File…", CommandId::OpenFile),
                ("Open Folder…", CommandId::OpenFolder),
            ] {
                let (output, _) = frame(vec![]);
                let rect = text_rect(&output, label)
                    .unwrap_or_else(|| panic!("whole {label} must be visible at {size:?}"));
                let pos = rect.center();
                frame(vec![egui::Event::PointerMoved(pos)]);
                let mut chosen = Vec::new();
                for pressed in [true, false] {
                    chosen.extend(
                        frame(vec![egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        }])
                        .1,
                    );
                }
                assert_eq!(chosen, [command]);
            }
        }
    }

    #[test]
    fn welcome_buttons_support_keyboard_focus_and_activation() {
        let context = crate::fonts::test_context();
        let frame = |events| {
            let mut commands = Vec::new();
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    if let Some(command) = show(ui, &ShortcutBindings::default(), |_| {}) {
                        commands.push(command);
                    }
                },
            );
            commands
        };
        for _ in 0..3 {
            frame(vec![]);
        }
        let key = |key| {
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]
        };
        frame(key(egui::Key::Tab));
        assert_eq!(frame(key(egui::Key::Enter)), [CommandId::OpenFile]);
        frame(key(egui::Key::Tab));
        assert_eq!(frame(key(egui::Key::Enter)), [CommandId::OpenFolder]);
    }

    fn text_rect(output: &egui::FullOutput, label: &str) -> Option<egui::Rect> {
        output.shapes.iter().find_map(|shape| {
            let egui::Shape::Text(text) = &shape.shape else {
                return None;
            };
            let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
            (text.galley.text() == label && shape.clip_rect.contains_rect(rect)).then_some(rect)
        })
    }
}
