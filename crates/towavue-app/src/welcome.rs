mod filter_menu;

use crate::scroll_style::ScrollAreaStyle;
use egui::RichText;
use towavue_core::{CommandId, MediaKind};

use crate::chrome;
use crate::hover_help::HoverHelp;
use crate::localization::{self, Text};
use towavue_core::localization::formatted;

#[derive(Clone, Default)]
struct GallerySwipe {
    gesture: crate::filmstrip::swipe::State,
    maximum: f32,
    projection: Option<egui::Id>,
}

pub(super) fn tab(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    active: bool,
    can_close: bool,
) -> (egui::Response, egui::Response) {
    named_tab(
        ui,
        rect,
        active,
        can_close,
        localization::text(ui.ctx(), Text::Gallery),
    )
}

pub(super) fn named_tab(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    active: bool,
    can_close: bool,
    title: &str,
) -> (egui::Response, egui::Response) {
    let language = localization::language(ui.ctx());
    let hovered = chrome::tab_hovered(ui, rect);
    let show_close = active || hovered;
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 3.0, chrome::BACKGROUND);
    let hover_background = painter.add(egui::Shape::Noop);
    chrome::tab_style(ui, rect, active, hovered);
    ui.spacing_mut().button_padding = egui::vec2(chrome::TAB_PADDING, 0.0);
    ui.visuals_mut().widgets.inactive.bg_stroke = egui::Stroke::NONE;
    ui.visuals_mut().widgets.hovered.bg_stroke = egui::Stroke::NONE;
    ui.visuals_mut().widgets.active.bg_stroke = egui::Stroke::NONE;
    let mut label_rect = rect;
    label_rect.max.x -= chrome::TAB_CLOSE_WIDTH;
    let text_rect = if show_close {
        label_rect
    } else {
        rect.with_max_x(rect.right() - chrome::TAB_PADDING)
    };
    let icon = crate::tab_icons::enabled(ui.ctx()).then(|| {
        if title == localization::text(ui.ctx(), Text::CommandOpenKeyboardSettings) {
            crate::fluent::Kind::Keyboard
        } else {
            crate::fluent::Kind::ImageMultiple
        }
    });
    if icon.is_some() {
        ui.spacing_mut().button_padding.x = 0.0;
    }
    let response = chrome::tab_title_layout(ui, label_rect, text_rect, title, active, None, icon);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            response.enabled(),
            formatted::tab_label(language, title),
        )
    });
    let close_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - chrome::TAB_CLOSE_WIDTH, rect.top()),
        rect.max,
    );
    let opacity = ui.opacity();
    let close = ui
        .add_enabled_ui(can_close, |ui| {
            ui.set_opacity(opacity);
            chrome::tab_close_visible(ui, close_rect, false, show_close)
        })
        .inner
        .help_text(formatted::close_title(language, title));
    close.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            close.enabled(),
            formatted::close_tab(language, title),
        )
    });
    crate::tab_focus::release_pointer_focus(&response);
    crate::tab_focus::release_pointer_button_focus(&response, egui::PointerButton::Middle);
    crate::tab_focus::release_pointer_focus(&close);
    chrome::tab_title_fade(ui, text_rect, active, hovered);
    if hovered && !active {
        painter.set(
            hover_background,
            egui::Shape::rect_filled(rect, 3.0, chrome::FLOATING_BACKGROUND),
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
    counts: &mut crate::browser_controls::Counts,
    query: &mut String,
    filter: &mut Option<MediaKind>,
    paths: &[std::path::PathBuf],
    enabled: bool,
    recent: impl FnOnce(&mut egui::Ui, &str, Option<MediaKind>) -> Vec<crate::gallery_rail::Month>,
) -> Option<CommandId> {
    let language = localization::language(ui.ctx());
    let previous_filter = *filter;
    let viewport = ui.available_rect_before_wrap();
    let inset = viewport.shrink(8.0);
    // Keep the header/rail insets, but let scrolling cards reach the media edge.
    let content_rect = egui::Rect::from_min_max(
        egui::pos2(inset.left(), viewport.top()),
        egui::pos2(inset.right(), viewport.bottom()),
    );
    let mut content = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
    ui.advance_cursor_after_rect(viewport);
    let ui = &mut content;
    if !enabled {
        let opacity = ui.opacity();
        ui.disable();
        ui.set_opacity(opacity);
    }
    // Center against the complete viewport, independently of the date rail.
    let side = crate::filmstrip::gallery_side_inset(viewport.width());
    let width = (viewport.width() - 2.0 * side).max(0.0);
    counts.update(paths, query);
    crate::browser_controls::normalize_filter(&counts.values, filter);
    let filters_size = crate::browser_controls::filter_size(ui, &counts.values);
    let header_width = width;
    // Spend the search gutter before collapsing filters, retaining a 100-point editor.
    let compact = header_width < filters_size.x + crate::browser_controls::SEARCH_MIN;
    let header_height = crate::browser_controls::text_height(ui);
    let header_rect = egui::Rect::from_min_size(
        egui::pos2(
            viewport.center().x - header_width * 0.5,
            viewport.top() + crate::browser_controls::INSET,
        ),
        egui::vec2(header_width, header_height),
    );
    let content_style = ui.style().clone();
    let color = ui.visuals().widgets.inactive.fg_stroke.color;
    ui.visuals_mut().widgets.hovered.fg_stroke.color = color;
    ui.visuals_mut().widgets.active.fg_stroke.color = color;
    let body = content_rect;
    let rail_top = if header_rect.right() > body.right() - 32.0 {
        header_rect.bottom() + 8.0
    } else {
        inset.top()
    };
    let rail = egui::Rect::from_min_max(
        egui::pos2(body.right() - 32.0, rail_top),
        egui::pos2(body.right(), inset.bottom()),
    );
    let swipe_id = ui.make_persistent_id("gallery-swipe");
    let mut swipe = ui.data_mut(|data| data.get_temp::<GallerySwipe>(swipe_id).unwrap_or_default());
    swipe.gesture.exclusions = vec![rail];
    let background = ui.interact(
        egui::Rect::from_min_max(egui::pos2(viewport.left(), body.top()), viewport.max),
        swipe_id,
        egui::Sense::drag(),
    );
    // ScrollArea wraps its salt before composing the persistent ID.
    let scroll_id = ui.make_persistent_id(egui::IdSalt::new("welcome"));
    crate::gallery_rail::advance(ui.ctx(), scroll_id);
    let mut offset = egui::scroll_area::State::load(ui.ctx(), scroll_id)
        .unwrap_or_default()
        .offset
        .y;
    let previous_offset = offset;
    let projection = swipe_id.with((query.as_str(), *filter, paths.len()));
    let reset = previous_filter != *filter || swipe.projection != Some(projection);
    swipe.projection = Some(projection);
    swipe.gesture.update_vertical(
        &background,
        projection,
        &mut offset,
        swipe.maximum,
        reset || egui::Popup::is_any_open(ui.ctx()),
    );
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
    let keyboard_offset = crate::list_navigation::unfocused_scroll(ui, "welcome", 40.0);
    let mut scroll = egui::ScrollArea::vertical()
        .content_margin(egui::Margin::ZERO)
        .id_salt("welcome")
        .scroll_source(egui::scroll_area::ScrollSource {
            drag: egui::scroll_area::DragScroll::Never,
            ..Default::default()
        })
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .auto_shrink([false, false]);
    if offset != previous_offset {
        scroll = scroll.vertical_scroll_offset(offset);
    }
    if reset {
        scroll = scroll.vertical_scroll_offset(0.0);
    }
    let mut output = scroll.show_styled(ui, |ui| {
        ui.set_style(content_style);
        if let Some(target) = keyboard_offset.filter(|_| !reset) {
            ui.scroll_with_delta(egui::vec2(0.0, previous_offset - target));
        }
        if gutter_scroll != 0.0 {
            ui.scroll_with_delta_animation(
                egui::vec2(0.0, gutter_scroll),
                egui::style::ScrollAnimation::none(),
            );
        }
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_size(
                    egui::pos2(
                        viewport.left() + side,
                        ui.cursor().top()
                            + header_height
                            + 2.0 * crate::browser_controls::INSET
                            + crate::filmstrip::GALLERY_GAP,
                    ),
                    egui::vec2(width, 0.0),
                ))
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                // Cards scroll behind the fixed, translucent header.
                ui.set_clip_rect(ui.clip_rect().intersect(body));
                let months = recent(ui, query, *filter);
                if paths.is_empty() {
                    ui.add(
                        egui::Label::new(
                            RichText::new(Text::GalleryDrop.in_language(language))
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
    });
    swipe.maximum = (output.content_size.y - output.inner_rect.height()).max(0.0);
    ui.data_mut(|data| data.insert_temp(swipe_id, swipe));
    let mut mesh = egui::Mesh::default();
    let fade = egui::Rect::from_min_max(
        viewport.min,
        egui::pos2(
            viewport.right(),
            header_rect.bottom() + crate::browser_controls::INSET,
        ),
    );
    for (pos, color) in [
        (fade.left_top(), egui::Color32::BLACK),
        (fade.right_top(), egui::Color32::BLACK),
        (fade.left_bottom(), egui::Color32::TRANSPARENT),
        (fade.right_bottom(), egui::Color32::TRANSPARENT),
    ] {
        mesh.colored_vertex(pos, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(2, 1, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
    let mut header = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("gallery-header")
            .max_rect(header_rect),
    );
    header.set_clip_rect(viewport);
    let filter_width = if compact {
        let visible = crate::browser_controls::visible_filters(&counts.values);
        let selected = if visible.iter().filter(|&&show| show).count() == 1 {
            crate::browser_controls::labels(&header)
                .into_iter()
                .enumerate()
                .find(|(index, _)| visible[*index])
                .map(|(_, (_, kind))| kind)
                .unwrap_or(*filter)
        } else {
            *filter
        };
        let label = crate::browser_controls::labels(&header)
            .into_iter()
            .find(|(_, kind)| *kind == selected)
            .expect("media kind")
            .0;
        let width = header
            .painter()
            .layout_no_wrap(
                label.into(),
                egui::TextStyle::Body.resolve(header.style()),
                egui::Color32::WHITE,
            )
            .size()
            .x;
        let response = crate::browser_controls::text_button(
            &header,
            egui::Rect::from_min_size(header_rect.min, egui::vec2(width, header_height)),
            header.id().with("compact-filter"),
            label,
            true,
        );
        filter_menu::show(&response, |ui, selected| {
            let mut rows = chrome::Dropdown::new(ui);
            for (index, (label, kind)) in
                crate::browser_controls::labels(ui).into_iter().enumerate()
            {
                if !visible[index] {
                    continue;
                }
                let row = rows.row(
                    ui,
                    kind.is_none() || counts.values[index] > 0,
                    *filter == kind,
                    label,
                );
                if selected(&row) {
                    *filter = kind;
                    ui.close();
                }
            }
            rows.finish(ui);
        });
        width
    } else {
        crate::browser_controls::filters(
            &mut header,
            egui::Rect::from_min_size(header_rect.min, filters_size),
            &counts.values,
            filter,
        );
        filters_size.x
    };
    let search_gap =
        (header_width - filter_width - crate::browser_controls::SEARCH_MIN).clamp(0.0, 16.0);
    let search_rect = egui::Rect::from_min_max(
        egui::pos2(
            (header_rect.left() + filter_width + search_gap)
                .max(header_rect.right() - crate::browser_controls::SEARCH_MAX),
            header_rect.top(),
        ),
        header_rect.max,
    );
    let search_id = header.make_persistent_id("Search Gallery");
    let search_changed = crate::browser_controls::search(
        &mut header,
        search_rect,
        search_id,
        Text::GallerySearch.in_language(language),
        query,
    );
    if search_changed || previous_filter != *filter {
        ui.ctx()
            .request_discard("Rebuild Gallery query projection before presenting");
    }
    // The header fade covers scrolling thumbnails, not the date rail or thumb.
    crate::gallery_rail::show(ui, &mut output, rail);
    None
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
    fn gallery_header_matches_grid_and_compact_filter_retains_dropdown_selection() {
        for density in [1.0, 1.25, 2.0] {
            for width in [240.0, 480.0, 960.0] {
                let context = crate::fonts::test_context();
                context.global_style_mut(chrome::style);
                context.set_pixels_per_point(density);
                context.enable_accesskit();
                let mut counts = crate::browser_controls::Counts::default();
                let mut query = String::new();
                let mut filter = None;
                let paths = ["photo.png", "video.mp4"].map(std::path::PathBuf::from);
                let mut frame = |events| {
                    context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 300.0),
                            )),
                            focused: true,
                            events,
                            ..Default::default()
                        },
                        |ui| {
                            super::show(
                                ui,
                                &mut counts,
                                &mut query,
                                &mut filter,
                                &paths,
                                true,
                                |ui, _, _| {
                                    ui.set_min_height(1000.0);
                                    vec![]
                                },
                            );
                        },
                    )
                };
                frame(vec![]);
                let output = frame(vec![]);
                let search = text_rect(&output, "Search").expect("Search button");
                let all = text_rect(&output, "All").expect("All filter");
                let side = crate::filmstrip::gallery_side_inset(width);
                assert!((all.left() - side).abs() <= 1.0 / density);
                assert!((search.right() - (width - side)).abs() <= 1.0 / density);
                assert!(all.right() < search.left());
                let fade = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Mesh(mesh)
                            if mesh.vertices.len() == 4
                                && mesh.vertices[0].color == egui::Color32::BLACK
                                && mesh.vertices[2].color == egui::Color32::TRANSPARENT =>
                        {
                            Some(mesh)
                        }
                        _ => None,
                    })
                    .expect("transparent gradient behind header controls");
                assert!(fade.vertices[0].pos.y < fade.vertices[2].pos.y);
                if width == 240.0 {
                    assert!(text_rect(&output, "Image").is_none());
                    let pos = all.center();
                    let button = |pos, pressed| egui::Event::PointerButton {
                        pos,
                        pressed,
                        button: egui::PointerButton::Primary,
                        modifiers: egui::Modifiers::NONE,
                    };
                    frame(vec![egui::Event::PointerMoved(pos), button(pos, true)]);
                    assert!(egui::Popup::is_any_open(&context));
                    let output = frame(vec![]);
                    let image = node_rect(&output, "Image").center();
                    frame(vec![egui::Event::PointerMoved(image), button(image, false)]);
                    let output = frame(vec![]);
                    assert!(!egui::Popup::is_any_open(&context));
                    assert!(text_rect(&output, "Image").is_some());
                    assert!(text_rect(&output, "All").is_none());
                } else {
                    for label in ["Image", "Video"] {
                        assert!(text_rect(&output, label).is_some());
                    }
                }
                let output = frame(vec![]);
                let position = node_rect(&output, "Search").center();
                frame(vec![egui::Event::PointerMoved(position)]);
                for pressed in [true, false] {
                    frame(vec![egui::Event::PointerButton {
                        pos: position,
                        pressed,
                        button: egui::PointerButton::Primary,
                        modifiers: egui::Modifiers::NONE,
                    }]);
                }
                let output = frame(vec![]);
                let input = node_rect(&output, "Search Gallery");
                assert!(
                    input.width() >= 100.0 - 1.0 / density
                        && input.width() <= 250.0 + 1.0 / density,
                    "search width at {width}: {input:?}"
                );
            }
        }
    }

    #[test]
    fn gallery_search_reaches_minimum_before_filters_collapse() {
        let mut checked_gutter_threshold = false;
        for density in [1.0, 1.25, 2.0] {
            for font_size in [10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0] {
                let context = crate::fonts::test_context();
                context.global_style_mut(|style| {
                    chrome::style(style);
                    style
                        .text_styles
                        .insert(egui::TextStyle::Body, egui::FontId::proportional(font_size));
                });
                context.set_pixels_per_point(density);
                context.enable_accesskit();
                let mut counts = crate::browser_controls::Counts::default();
                let mut query = String::from("file");
                let mut filter = Some(MediaKind::Video);
                let paths = ["file.png", "file.mp4", "file.wav"].map(std::path::PathBuf::from);
                let mut full_width = 0.0;
                let mut frame = || {
                    context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(480.0, 300.0),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            full_width = crate::browser_controls::filter_size(ui, &[3, 1, 1, 1]).x;
                            super::show(
                                ui,
                                &mut counts,
                                &mut query,
                                &mut filter,
                                &paths,
                                true,
                                |_, _, _| vec![],
                            );
                        },
                    )
                };
                frame();
                let output = frame();
                let width = 480.0 - 2.0 * crate::filmstrip::gallery_side_inset(480.0);
                let compact = width - full_width < 100.0;
                checked_gutter_threshold |= (100.0..116.0).contains(&(width - full_width));
                assert_eq!(text_rect(&output, "All").is_none(), compact);
                assert_eq!(filter, Some(MediaKind::Video));
                let input = node_rect(&output, "Search Gallery");
                assert!(input.width() >= 100.0 - 1.0 / density);
                assert!(input.width() <= 250.0 + 1.0 / density);
                if !compact && width - full_width < 116.0 {
                    assert!((input.width() - 100.0).abs() <= 1.0 / density);
                }
            }
        }
        assert!(
            checked_gutter_threshold,
            "exercise the formerly premature collapse"
        );
    }

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
                        egui::Shape::Rect(background) if background.rect == rect && background.fill == if active { chrome::BACKGROUND } else { chrome::FLOATING_BACKGROUND }
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
        counts: &mut crate::browser_controls::Counts,
        recent: impl FnOnce(&mut egui::Ui),
    ) -> Option<CommandId> {
        super::show(
            ui,
            counts,
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
            let grid_origin = std::cell::Cell::new(0.0);
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
                            show(ui, &mut crate::browser_controls::Counts::default(), |ui| {
                                assert_eq!(ui.visuals().widgets, original, "card style");
                                grid_clip.set(ui.clip_rect());
                                grid_origin.set(ui.max_rect().top());
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
                        egui::Shape::Rect(rect)
                            if rect.fill == chrome::FOREGROUND
                                && rect.rect.left() > 400.0
                                && rect.rect.height() <= 1.01
                                && rect.rect.width() > 20.0 =>
                        {
                            Some(rect.rect.center().y)
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
            let grid_top = grid_origin.get();
            let header_text_height = text_rect(&idle, "Search").expect("header").height();
            let idle_marker = marker(&idle);
            assert!(
                (grid_clip.get().top() + header_text_height + 36.0 + crate::filmstrip::GALLERY_GAP
                    - grid_top)
                    .abs()
                    <= 1.0 / density,
                "initial clip {:?}, grid top {grid_top}, density {density}",
                grid_clip.get()
            );
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
                assert!(
                    (grid_clip.get().top()
                        + header_text_height
                        + 36.0
                        + crate::filmstrip::GALLERY_GAP
                        - grid_top)
                        .abs()
                        <= 1.0 / density,
                    "scrolling retains the full viewport clip beneath the header"
                );
                assert!(scrolled > after, "body gutter scrolls: {gutter:?}");
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
            frame(vec![button(end, false)]);
            for _ in 0..70 {
                frame(vec![]);
            }
            let bottom = marker(&frame(vec![]));
            assert!(
                (bottom - (track.bottom() - (idle_marker - track.top()))).abs() <= 2.0 / density,
                "owned drag reaches the end outside the rail"
            );
        }
    }

    #[test]
    fn gallery_page_keys_scroll_smoothly_without_card_focus() {
        let context = crate::fonts::test_context();
        let mut time = 0.0;
        let mut frame = |key: Option<egui::Key>| {
            time += 1.0 / 60.0;
            let mut origin = 0.0;
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 300.0),
                    )),
                    time: Some(time),
                    events: key
                        .into_iter()
                        .map(|key| egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        })
                        .collect(),
                    ..Default::default()
                },
                |ui| {
                    show(ui, &mut crate::browser_controls::Counts::default(), |ui| {
                        origin = ui.max_rect().top();
                        ui.set_min_height(2000.0);
                    });
                },
            );
            origin
        };
        for _ in 0..3 {
            frame(None);
        }
        let before = frame(None);
        frame(Some(egui::Key::PageDown));
        frame(None); // ScrollArea applies a newly requested target on the next pass.
        let first = frame(None);
        for _ in 0..50 {
            frame(None);
        }
        let last = frame(None);
        assert!(
            last < first && first < before,
            "intermediate frame: {before} -> {first} -> {last}"
        );
        assert!((before - last - 300.0).abs() <= 1.0, "one body viewport");
        frame(Some(egui::Key::PageUp));
        for _ in 0..50 {
            frame(None);
        }
        assert!((frame(None) - before).abs() < 1.0);
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
        for label in ["Search", "Drop media files or a folder here to begin."] {
            assert!(
                text_rect(&last, label).is_some(),
                "{label} is present in idle CPU output"
            );
        }
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
