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
    query: &mut String,
    has_recent: bool,
    enabled: bool,
    recent: impl FnOnce(&mut egui::Ui, &str),
) -> Option<CommandId> {
    let viewport = ui.available_rect_before_wrap();
    let inset = viewport.shrink(8.0_f32.min(viewport.size().min_elem().max(0.0) * 0.25));
    let mut content = ui.new_child(egui::UiBuilder::new().max_rect(inset));
    ui.advance_cursor_after_rect(viewport);
    let ui = &mut content;
    if !enabled {
        ui.disable();
    }
    let width = (ui.available_width() - 32.0).clamp(0.0, 660.0);
    let top = (ui.available_height() * 0.08).clamp(12.0, 40.0);
    let mut chosen = None;
    ui.add_space(top);
    let search_changed = ui
        .horizontal(|ui| {
            ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
            ui.spacing_mut().item_spacing.x = 8.0;
            let search = ui
                .allocate_ui_with_layout(
                    egui::vec2((width - 64.0).max(1.0), 24.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| crate::resize::text_input(ui, "Search Gallery", query),
                )
                .inner;
            for (command, label, icon) in [
                (CommandId::OpenFile, "Open File…", chrome::Icon::OpenFile),
                (
                    CommandId::OpenFolder,
                    "Open Folder…",
                    chrome::Icon::OpenFolder,
                ),
            ] {
                let response = ui
                    .add_sized(
                        [24.0, 24.0],
                        egui::Button::new(icon.text()).frame_when_inactive(false),
                    )
                    .help_text(format!(
                        "{label}  {}",
                        shortcuts.label(command, Default::default())
                    ));
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), label)
                });
                if response.clicked() {
                    chosen = Some(command);
                }
            }
            search.changed()
        })
        .inner;
    ui.add_space(32.0);
    let content_style = ui.style().clone();
    let color = ui.visuals().widgets.inactive.fg_stroke.color;
    ui.visuals_mut().widgets.hovered.fg_stroke.color = color;
    ui.visuals_mut().widgets.active.fg_stroke.color = color;
    ui.spacing_mut().scroll.interact_background_opacity = 0.3;
    let body = ui.available_rect_before_wrap();
    let gutter_scroll = if ui.is_enabled()
        && ui.input(|input| {
            input
                .pointer
                .hover_pos()
                .is_some_and(|p| viewport.contains(p) && p.y >= body.top() && !inset.contains(p))
        }) {
        ui.input_mut(|input| std::mem::take(&mut input.smooth_scroll_delta.y))
    } else {
        0.0
    };
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt("welcome")
        .auto_shrink([false, false]);
    if search_changed {
        scroll = scroll.vertical_scroll_offset(0.0);
    }
    scroll.show_styled(ui, |ui| {
        ui.set_style(content_style);
        if gutter_scroll != 0.0 {
            ui.scroll_with_delta_animation(
                egui::vec2(0.0, gutter_scroll),
                egui::style::ScrollAnimation::none(),
            );
        }
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
            ui.allocate_ui_with_layout(
                egui::vec2(width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    recent(ui, query);
                    if !has_recent {
                        ui.add(
                            egui::Label::new(
                                RichText::new("Drop media files or a folder here to begin.")
                                    .color(chrome::MUTED),
                            )
                            .wrap(),
                        );
                    }
                    ui.add_space(16.0);
                },
            );
        });
    });
    chosen
}

pub(super) fn matches(path: &std::path::Path, query: &str) -> bool {
    let path = path.to_string_lossy().replace('\\', "/").to_lowercase();
    query
        .split_whitespace()
        .all(|word| path.contains(&word.replace('\\', "/").to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_search_matches_all_words_in_names_and_paths_without_case_or_separator_sensitivity() {
        let path = std::path::Path::new("C:\\Photos\\日本\\My IMAGE.PNG");
        for query in [
            "",
            "  ",
            "my png",
            "日本 IMAGE",
            "photos/日本",
            "PHOTOS\\日本",
        ] {
            assert!(matches(path, query), "{query}");
        }
        for query in ["image jpg", "absent", "日本 unrelated"] {
            assert!(!matches(path, query), "{query}");
        }
    }

    fn show(
        ui: &mut egui::Ui,
        shortcuts: &ShortcutBindings,
        recent: impl FnOnce(&mut egui::Ui),
    ) -> Option<CommandId> {
        super::show(ui, shortcuts, &mut String::new(), false, true, |ui, _| {
            recent(ui)
        })
    }

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
            assert!(
                track.top()
                    > text_rect(&frame(vec![]), "Search Gallery")
                        .expect("fixed search header")
                        .bottom()
                        + 24.0
            );
            for (actual, expected) in [(track.bottom(), 292.0), (track.right(), 472.0)] {
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
                if gutter.y < track.top() {
                    assert_eq!(
                        scrolled[1].rect.top(),
                        after_click[1].rect.top(),
                        "fixed header does not scroll the body"
                    );
                } else {
                    assert!(
                        scrolled[1].rect.top() > after_click[1].rect.top(),
                        "wheel works in the body gutter: {gutter:?}"
                    );
                }
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
                    if text.galley.text() == "Search Gallery" {
                        wordmark = true;
                    }
                }
                assert!(wordmark && icon, "audit search text and action icons");
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
        for label in [
            "Search Gallery",
            "Drop media files or a folder here to begin.",
        ] {
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
            context.enable_accesskit();
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
            let search = node_rect(&output, "Search Gallery");
            let open = node_rect(&output, "Open File…");
            let folder = node_rect(&output, "Open Folder…");
            assert!(search.right() < open.left() && open.right() < folder.left());
            assert!((open.center().y - folder.center().y).abs() < 1.0);
            assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(folder));
            for (label, command) in [
                ("Open File…", CommandId::OpenFile),
                ("Open Folder…", CommandId::OpenFolder),
            ] {
                let (output, _) = frame(vec![]);
                let rect = node_rect(&output, label);
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
        assert!(
            frame(key(egui::Key::Enter)).is_empty(),
            "search does not open a file"
        );
        // Enter finishes a single-line edit; Tab re-enters the first field.
        frame(key(egui::Key::Tab));
        frame(key(egui::Key::Tab));
        assert_eq!(frame(key(egui::Key::Enter)), [CommandId::OpenFile]);
        frame(key(egui::Key::Tab));
        assert_eq!(frame(key(egui::Key::Enter)), [CommandId::OpenFolder]);
    }

    fn node_rect(output: &egui::FullOutput, label: &str) -> egui::Rect {
        let bounds = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("tree")
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label))
            .expect("named widget")
            .1
            .bounds()
            .expect("bounds");
        egui::Rect::from_min_max(
            egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
            egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
        )
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
