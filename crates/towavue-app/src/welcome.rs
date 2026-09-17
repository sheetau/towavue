use crate::scroll_style::ScrollAreaStyle;
use egui::RichText;
use towavue_core::{CommandId, MediaKind, ShortcutBindings};

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
    let hover_background = ui.painter().add(egui::Shape::Noop);
    ui.spacing_mut().button_padding = egui::vec2(chrome::TAB_PADDING, 0.0);
    ui.visuals_mut().widgets.inactive.bg_stroke = egui::Stroke::NONE;
    ui.visuals_mut().widgets.hovered.bg_stroke = egui::Stroke::NONE;
    ui.visuals_mut().widgets.active.bg_stroke = egui::Stroke::NONE;
    let mut label_rect = rect;
    label_rect.max.x -= chrome::TAB_CLOSE_WIDTH;
    let response = chrome::tab_title(ui, label_rect, "Gallery", active);
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
    let hovered = response.hovered() || close.hovered();
    chrome::tab_title_fade(ui, label_rect, active, hovered);
    if hovered {
        ui.painter().set(
            hover_background,
            egui::Shape::rect_filled(rect, 3.0, chrome::HOVER),
        );
    }
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
    filter: &mut Option<MediaKind>,
    paths: &[std::path::PathBuf],
    enabled: bool,
    recent: impl FnOnce(&mut egui::Ui, &str, Option<MediaKind>) -> Vec<crate::gallery_rail::Month>,
) -> Option<CommandId> {
    let previous_filter = *filter;
    let viewport = ui.available_rect_before_wrap();
    let inset = viewport.shrink(8.0_f32.min(viewport.size().min_elem().max(0.0) * 0.25));
    // Keep the header/rail insets, but let scrolling cards reach the media edge.
    let content_rect =
        egui::Rect::from_min_max(inset.min, egui::pos2(inset.right(), viewport.bottom()));
    let mut content = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
    ui.advance_cursor_after_rect(viewport);
    let ui = &mut content;
    if !enabled {
        let opacity = ui.opacity();
        ui.disable();
        ui.set_opacity(opacity);
    }
    let width = (ui.available_width() - 40.0).clamp(0.0, 660.0);
    let gap = (ui.available_width() - width).max(0.0);
    let left = (gap / 2.0).min((gap - 40.0).max(0.0));
    let top = (ui.available_height() * 0.04).clamp(6.0, 20.0);
    let mut chosen = None;
    ui.add_space(top);
    let mut header = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
        ui.cursor().min + egui::vec2(left, 0.0),
        egui::vec2(width, 24.0),
    )));
    let search_changed = header
        .horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.spacing_mut().button_padding = egui::vec2(4.0, 0.0);
            let search = ui
                .scope(|ui| {
                    ui.visuals_mut().widgets.inactive.bg_stroke =
                        ui.visuals().widgets.hovered.bg_stroke;
                    ui.spacing_mut().text_edit_width = f32::INFINITY;
                    ui.add_sized(
                        [(ui.available_width() - 84.0).max(24.0), 24.0],
                        |ui: &mut egui::Ui| crate::resize::text_input(ui, "Search Gallery", query),
                    )
                })
                .inner;
            let filter_button = ui
                .add_sized(
                    [24.0, 24.0],
                    egui::Button::new(chrome::Icon::Filter.text().color(if filter.is_some() {
                        chrome::FOREGROUND
                    } else {
                        chrome::MUTED
                    }))
                    .stroke(egui::Stroke::NONE)
                    .frame_when_inactive(false),
                )
                .help_text("Filter media types");
            filter_button.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    filter_button.enabled(),
                    "Filter media types",
                )
            });
            if enabled {
                egui::Popup::menu(&filter_button).show(|ui| {
                    for (kind, label) in [
                        (None, "All"),
                        (Some(MediaKind::Image), "Images"),
                        (Some(MediaKind::Video), "Videos"),
                        (Some(MediaKind::Audio), "Audio"),
                    ] {
                        let available = kind.is_none()
                            || paths.iter().any(|path| MediaKind::from_path(path) == kind);
                        if ui
                            .add_enabled(
                                available,
                                egui::Button::selectable(*filter == kind, label),
                            )
                            .clicked()
                        {
                            *filter = kind;
                            ui.close();
                        }
                    }
                });
            }
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
                        egui::Button::new(icon.text())
                            .stroke(egui::Stroke::NONE)
                            .frame_when_inactive(false),
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
    ui.advance_cursor_after_rect(header.min_rect());
    ui.add_space(16.0);
    let content_style = ui.style().clone();
    let color = ui.visuals().widgets.inactive.fg_stroke.color;
    ui.visuals_mut().widgets.hovered.fg_stroke.color = color;
    ui.visuals_mut().widgets.active.fg_stroke.color = color;
    let body = ui.available_rect_before_wrap();
    let gutter_scroll = if ui.is_enabled()
        && ui
            .input(|input| input.pointer.hover_pos())
            .is_some_and(|p| {
                viewport.contains(p)
                    && p.y >= body.top()
                    && !inset.contains(p)
                    && ui.ctx().layer_id_at(p) == Some(ui.layer_id())
            }) {
        ui.input_mut(|input| std::mem::take(&mut input.smooth_scroll_delta.y))
    } else {
        0.0
    };
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt("welcome")
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .auto_shrink([false, false]);
    if search_changed || previous_filter != *filter {
        scroll = scroll.vertical_scroll_offset(0.0);
    }
    let mut output = scroll.show_styled(ui, |ui| {
        ui.set_style(content_style);
        if gutter_scroll != 0.0 {
            ui.scroll_with_delta_animation(
                egui::vec2(0.0, gutter_scroll),
                egui::style::ScrollAnimation::none(),
            );
        }
        ui.horizontal(|ui| {
            ui.add_space(left);
            ui.allocate_ui_with_layout(
                egui::vec2(width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let months = recent(ui, query, *filter);
                    if paths.is_empty() {
                        ui.add(
                            egui::Label::new(
                                RichText::new("Drop media files or a folder here to begin.")
                                    .color(chrome::MUTED),
                            )
                            .wrap(),
                        );
                    }
                    ui.add_space(16.0);
                    months
                },
            )
            .inner
        })
        .inner
    });
    let rail = egui::Rect::from_min_max(
        egui::pos2(body.right() - 32.0, inset.top()),
        egui::pos2(body.right(), inset.bottom()),
    );
    crate::gallery_rail::show(ui, &mut output, rail);
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
    fn gallery_tab_hover_preserves_text_origin_and_fills_the_whole_tab() {
        for density in [1.0, 1.25, 2.0] {
            for active in [false, true] {
                let context = crate::fonts::test_context();
                context.set_pixels_per_point(density);
                context.global_style_mut(chrome::style);
                let rect = egui::Rect::from_min_size(egui::pos2(8.0, 8.0), egui::vec2(160.0, 28.0));
                let frame = |position| {
                    context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(400.0, 100.0),
                            )),
                            events: vec![egui::Event::PointerMoved(position)],
                            ..Default::default()
                        },
                        |ui| {
                            tab(ui, rect, active, true);
                        },
                    )
                };
                let outside = egui::pos2(300.0, 80.0);
                frame(outside);
                let idle = frame(outside);
                let text = text_rect(&idle, "Gallery").expect("label");
                for position in [rect.center(), rect.right_center() - egui::vec2(8.0, 0.0)] {
                    frame(position);
                    let hovered = frame(position);
                    assert_eq!(text_rect(&hovered, "Gallery").expect("label"), text);
                    assert!(hovered.shapes.iter().any(|shape| matches!(&shape.shape,
                        egui::Shape::Rect(background) if background.rect == rect && background.fill == chrome::HOVER
                    )));
                }
            }
        }
    }

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
        super::show(
            ui,
            shortcuts,
            &mut String::new(),
            &mut None,
            &[],
            true,
            |ui, _, _| {
                recent(ui);
                vec![crate::gallery_rail::Month {
                    date: None,
                    offset: 0.0,
                }]
            },
        )
    }

    #[test]
    fn gallery_rail_is_inset_and_keeps_body_gutter_wheel_input() {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.enable_accesskit();
            context.set_pixels_per_point(density);
            context.global_style_mut(chrome::style);
            context.global_style_mut(|style| style.animation_time = 0.0);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(480.0, 300.0));
            let original = context.global_style().visuals.widgets.clone();
            let overlay = std::cell::Cell::new(false);
            let grid_clip = std::cell::Cell::new(egui::Rect::NOTHING);
            let frame = |events| {
                context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        if overlay.get() {
                            egui::Area::new("gutter-overlay".into())
                                .order(egui::Order::Foreground)
                                .fixed_pos(egui::pos2(0.0, 130.0))
                                .movable(false)
                                .show(&context, |ui| {
                                    ui.allocate_exact_size(
                                        egui::vec2(20.0, 40.0),
                                        egui::Sense::hover(),
                                    );
                                });
                        }
                        assert!(
                            show(ui, &ShortcutBindings::default(), |ui| {
                                assert_eq!(ui.visuals().widgets, original, "card style");
                                grid_clip.set(ui.clip_rect());
                                ui.set_min_height(1200.0);
                            })
                            .is_none()
                        );
                        assert_eq!(ui.visuals().widgets, original, "sibling style");
                    },
                )
            };
            let marker = |output: &egui::FullOutput| {
                output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::LineSegment { points, stroke }
                            if stroke.color == chrome::FOREGROUND && points[0].x > 400.0 =>
                        {
                            Some(points[0].y)
                        }
                        _ => None,
                    })
                    .expect("current-position bar")
            };
            for _ in 0..3 {
                frame(vec![]);
            }
            let idle = frame(vec![]);
            let track = node_rect(&idle, "Date unknown");
            assert!(
                (grid_clip.get().bottom() - screen.bottom()).abs() <= 1.0 / density,
                "cards can reach the media bottom: {:?}",
                grid_clip.get()
            );
            assert!(
                (track.top() - 8.0).abs() <= 1.0 / density,
                "date rail reaches the media top inset"
            );
            assert!((track.right() - 472.0).abs() <= 1.0 / density);
            assert!((track.bottom() - 292.0).abs() <= 1.0 / density);
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
                let before = marker(&frame(vec![]));
                frame(vec![
                    egui::Event::PointerMoved(gutter),
                    button(gutter, true),
                ]);
                let after = marker(&frame(vec![button(gutter, false)]));
                assert_eq!(before, after, "gutter clicks cannot jump the rail");
                frame(vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -80.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                }]);
                for _ in 0..30 {
                    frame(vec![]);
                }
                let scrolled = marker(&frame(vec![]));
                if gutter.y < track.top() {
                    assert_eq!(scrolled, after, "fixed header does not scroll");
                } else {
                    assert!(scrolled > after, "body gutter scrolls: {gutter:?}");
                }
            }
            overlay.set(true);
            for _ in 0..3 {
                frame(vec![]);
            }
            let before = marker(&frame(vec![]));
            frame(vec![
                egui::Event::PointerMoved(egui::pos2(2.0, 150.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -80.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            for _ in 0..30 {
                frame(vec![]);
            }
            assert_eq!(
                marker(&frame(vec![])),
                before,
                "covered gutter does not scroll Gallery"
            );
            overlay.set(false);
            frame(vec![]);
            let start = track.center();
            let end = track.center_bottom() + egui::vec2(0.0, 30.0);
            frame(vec![egui::Event::PointerMoved(start), button(start, true)]);
            frame(vec![egui::Event::PointerMoved(end)]);
            let bottom = marker(&frame(vec![button(end, false)]));
            assert!(
                (bottom - (track.bottom() - 2.0)).abs() <= 1.0 / density,
                "owned drag reaches the end outside the rail"
            );
        }
    }

    #[test]
    fn gallery_header_centers_square_buttons_and_highlights_the_active_filter() {
        for density in [1.0, 1.25, 2.0] {
            for width in [240.0, 480.0, 960.0] {
                for filter in [None, Some(MediaKind::Video)] {
                    let context = crate::fonts::test_context();
                    context.set_pixels_per_point(density);
                    context.global_style_mut(chrome::style);
                    context.enable_accesskit();
                    let mut output = egui::FullOutput::default();
                    for _ in 0..3 {
                        output = context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(width, 300.0),
                                )),
                                ..Default::default()
                            },
                            |ui| {
                                let mut selected = filter;
                                super::show(
                                    ui,
                                    &ShortcutBindings::default(),
                                    &mut String::new(),
                                    &mut selected,
                                    &[],
                                    true,
                                    |_, _, _| vec![],
                                );
                            },
                        );
                    }
                    let search = node_rect(&output, "Search Gallery");
                    for label in ["Filter media types", "Open File…", "Open Folder…"] {
                        let rect = node_rect(&output, label);
                        assert!(
                            (rect.width() - rect.height()).abs() <= 1.0 / density,
                            "square {label}: {rect:?}"
                        );
                        assert!(
                            (rect.center().y - search.center().y).abs() <= 1.0 / density,
                            "centered {label}: {rect:?}, search={search:?}"
                        );
                    }
                    let icon = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.text() == "\u{eaf1}" => {
                                Some(text)
                            }
                            _ => None,
                        })
                        .expect("filter icon");
                    let expected = if filter.is_some() {
                        chrome::FOREGROUND
                    } else {
                        chrome::MUTED
                    };
                    assert!(
                        icon.galley
                            .job
                            .sections
                            .iter()
                            .all(|section| section.format.color == expected)
                    );
                }
            }
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
        for (size, density) in [
            egui::vec2(960.0, 514.0),
            egui::vec2(480.0, 238.0),
            egui::vec2(240.0, 119.0),
        ]
        .into_iter()
        .flat_map(|size| [1.0, 1.25, 2.0].map(|density| (size, density)))
        {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            context.global_style_mut(chrome::style);
            let mut shortcuts = ShortcutBindings::default();
            context.enable_accesskit();
            shortcuts.set(
                CommandId::OpenFile,
                "Ctrl+K Ctrl+O".parse().expect("shortcut"),
            );
            let grid = std::cell::Cell::new(egui::Rect::NOTHING);
            let frame = |events| {
                let mut commands = Vec::new();
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        if let Some(command) = show(ui, &shortcuts, |ui| {
                            grid.set(ui.available_rect_before_wrap());
                        }) {
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
            let filter = node_rect(&output, "Filter media types");
            let open = node_rect(&output, "Open File…");
            let folder = node_rect(&output, "Open Folder…");
            assert!(
                search.right() < filter.left()
                    && filter.right() < open.left()
                    && open.right() < folder.left()
            );
            for rect in [filter, open, folder] {
                assert!((rect.width() - 24.0).abs() <= 1.0 / density);
                assert!((rect.height() - 24.0).abs() <= 1.0 / density);
                assert!((rect.center().y - search.center().y).abs() <= 1.0 / density);
            }
            assert!((search.height() - 24.0).abs() <= 1.0 / density);
            let hint = text_rect(&output, "Search Gallery").expect("Gallery placeholder");
            assert!((hint.center().y - search.center().y).abs() <= 1.0 / density);
            assert!((search.left() - grid.get().left()).abs() <= 1.0 / density);
            assert!((folder.right() - grid.get().right()).abs() <= 1.0 / density);
            let border = |output: &egui::FullOutput| {
                output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect)
                            if rect.rect.min.distance(search.min) <= 1.0 / density
                                && rect.rect.max.distance(search.max) <= 1.0 / density =>
                        {
                            Some(rect.stroke)
                        }
                        _ => None,
                    })
                    .expect("search field border")
            };
            let idle_border = border(&output);
            frame(vec![egui::Event::PointerMoved(search.center())]);
            let hovered = frame(vec![egui::Event::PointerMoved(search.center())]).0;
            assert_eq!(border(&hovered), idle_border);
            assert_eq!(idle_border, egui::Stroke::new(1.0, chrome::BORDER));
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
        // The type-filter button precedes Open File.
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
