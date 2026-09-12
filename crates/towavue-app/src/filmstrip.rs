use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::{Align2, Color32, Context, FontId, Rect, TextureHandle, Vec2};
use towavue_core::{FolderSnapshot, MediaKind, ReadingAxis};
use towavue_runtime_windows::{PreviewCache, PreviewLoader, VISIBLE_PREVIEW_LIMIT};

use crate::hover_help::HoverHelp;
use crate::{UiAction, display_name, format_time, media_time};

const STEP: f32 = 128.0;
const HEIGHT: f32 = 118.0;

type Preview = Result<(TextureHandle, Option<Duration>), String>;

mod drag;

#[cfg(test)]
pub(crate) mod drag_tests;

#[derive(Default)]
pub struct View {
    focus: Option<PathBuf>,
    offset: f32,
}

pub struct Filmstrip {
    loader: PreviewLoader,
    generation: u64,
    visible: Vec<PathBuf>,
    previews: HashMap<PathBuf, Preview>,
    focus: Option<PathBuf>,
    focus_requested: bool,
    scroll_offset: f32,
    drag: drag::State,
}

impl Filmstrip {
    pub fn new(cache: PreviewCache, notify: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        Ok(Self {
            loader: PreviewLoader::new(cache, notify)?,
            generation: 0,
            visible: Vec::new(),
            previews: HashMap::new(),
            focus: None,
            focus_requested: false,
            scroll_offset: 0.0,
            drag: drag::State::default(),
        })
    }

    pub fn clear(&mut self) {
        self.focus_requested = false;
        self.focus = None;
        self.scroll_offset = 0.0;
        self.clear_previews();
    }

    pub fn cancel_drag(&mut self) {
        self.drag.clear();
    }

    pub fn clear_previews(&mut self) {
        self.drag.clear();
        if !self.visible.is_empty() {
            self.generation = self.loader.request(Vec::new());
            self.visible.clear();
            self.previews.clear();
        }
    }

    pub fn take_view(&mut self) -> View {
        self.clear_previews();
        self.focus_requested = false;
        View {
            focus: self.focus.take(),
            offset: std::mem::take(&mut self.scroll_offset),
        }
    }

    pub fn restore_view(&mut self, view: View) {
        self.clear_previews();
        self.focus_requested = false;
        self.focus = view.focus;
        self.scroll_offset = view.offset;
    }

    pub fn focus_current(&mut self) {
        self.focus_requested = true;
    }

    pub fn finish(&mut self, context: &Context) {
        for preview in self.loader.take_completed() {
            if preview.generation != self.generation || !self.visible.contains(&preview.path) {
                continue;
            }
            let result = preview.result.map(|media| {
                let image = media.image;
                let texture = context.load_texture(
                    format!("filmstrip:{}", preview.path.display()),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [image.width as usize, image.height as usize],
                        &image.rgba,
                    ),
                    egui::TextureOptions::LINEAR,
                );
                (texture, media.duration)
            });
            self.previews.insert(preview.path, result);
        }
    }

    pub fn show_seek_preview(
        &mut self,
        response: &egui::Response,
        ratio: f32,
        paths: &[PathBuf],
        caption: &str,
        axis: ReadingAxis,
    ) {
        self.set_visible(
            paths
                .iter()
                .map(|path| (path.clone(), MediaKind::Image))
                .collect(),
        );
        crate::seekbar::preview_tooltip(response, ratio).show(|ui| {
            let height =
                (response.rect.top() - response.ctx.viewport_rect().top() - 40.0).clamp(1.0, 108.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(160.0, height), egui::Sense::hover());
            let sizes: Vec<_> = paths
                .iter()
                .map(|path| match self.previews.get(path) {
                    Some(Ok((texture, _))) => texture.size_vec2(),
                    _ => Vec2::splat(1.0),
                })
                .collect();
            let cells = crate::reading_page_rects(rect, &sizes, axis, false);
            for (path, cell) in paths.iter().zip(cells) {
                match self.previews.get(path) {
                    Some(Ok((texture, _))) => {
                        ui.painter().image(
                            texture.id(),
                            cell,
                            Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    preview => {
                        ui.painter().with_clip_rect(cell).text(
                            cell.center(),
                            Align2::CENTER_CENTER,
                            if preview.is_some() {
                                "No preview"
                            } else {
                                "…"
                            },
                            FontId::proportional(11.0),
                            Color32::GRAY,
                        );
                    }
                }
            }
            ui.add(egui::Label::new(caption).truncate());
        });
    }

    pub fn show(
        &mut self,
        context: &Context,
        media_rect: Rect,
        snapshot: Option<&FolderSnapshot>,
        current: Option<&Path>,
        enabled: bool,
        actions: &mut Vec<UiAction>,
    ) {
        let screen = media_rect.intersect(context.content_rect());
        self.drag.begin(context, snapshot, current, enabled);
        context
            .layer_painter(egui::LayerId::new(
                egui::Order::Middle,
                "filmstrip-dim".into(),
            ))
            .rect_filled(screen, 0.0, Color32::from_black_alpha(191));
        let Some(snapshot) = snapshot else {
            context
                .layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    "filmstrip-loading".into(),
                ))
                .text(
                    screen.center(),
                    Align2::CENTER_CENTER,
                    "Loading folder order…",
                    FontId::proportional(14.0),
                    Color32::WHITE,
                );
            return;
        };
        let recenter = current != self.focus.as_deref();
        self.focus = current.map(Path::to_owned);
        let selected = snapshot
            .items
            .iter()
            .position(|item| Some(item.path.as_path()) == current);
        let mut wanted = Vec::new();
        egui::Area::new("filmstrip".into())
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .constrain(false)
            .show(context, |ui| {
                if !enabled || egui::Popup::is_any_open(context) {
                    let opacity = ui.opacity();
                    ui.disable();
                    ui.set_opacity(opacity);
                }
                ui.set_clip_rect(screen);
                ui.set_min_size(screen.size());
                ui.set_max_size(screen.size());
                ui.style_mut().always_scroll_the_only_direction = true;
                ui.spacing_mut().scroll.bar_width = 5.0;
                ui.spacing_mut().scroll.bar_outer_margin = 8.0;
                ui.spacing_mut().scroll.dormant_handle_opacity = 0.6;
                let mut scroll = egui::ScrollArea::horizontal()
                    .id_salt("filmstrip-scroll")
                    .horizontal_scroll_offset(self.scroll_offset)
                    .auto_shrink([false, false])
                    .max_height(screen.height())
                    .scroll_bar_rect(screen.shrink(8.0));
                if recenter || self.focus_requested {
                    scroll = scroll.horizontal_scroll_offset(selected.unwrap_or(0) as f32 * STEP);
                }
                let output = scroll.show_viewport(ui, |ui, viewport| {
                    let padding = ((screen.width() - STEP) / 2.0).max(0.0);
                    let origin = ui.min_rect().min;
                    ui.set_min_size(egui::vec2(
                        snapshot.items.len() as f32 * STEP + padding * 2.0,
                        viewport.height(),
                    ));
                    for index in visible_range(viewport, padding, snapshot.items.len()) {
                        let item = &snapshot.items[index];
                        wanted.push((item.path.clone(), item.kind));
                        let rect = Rect::from_min_size(
                            origin
                                + egui::vec2(
                                    padding + index as f32 * STEP + 4.0,
                                    (viewport.height() - HEIGHT) / 2.0 + 24.0,
                                ),
                            egui::vec2(120.0, 80.0),
                        );
                        let response = ui.interact(
                            rect,
                            ui.id().with(("filmstrip-item", &item.path)),
                            egui::Sense::click_and_drag(),
                        );
                        let active = selected == Some(index);
                        self.drag
                            .observe(&response, &item.path, self.previews.get(&item.path));
                        if active
                            && self.focus_requested
                            && response.enabled()
                            && !egui::Popup::is_any_open(context)
                        {
                            response.request_focus();
                            self.focus_requested = false;
                        }
                        crate::tab_focus::observe(&response, ("filmstrip-item", &item.path));
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                ui.is_enabled(),
                                display_name(&item.path),
                            )
                        });
                        context.accesskit_node_builder(response.id, |node| {
                            node.set_description(format!(
                                "{}{}",
                                item.path.display(),
                                if active { " (current item)" } else { "" }
                            ));
                        });
                        ui.painter().rect_filled(rect, 0.0, crate::chrome::BORDER);
                        match self.previews.get(&item.path) {
                            Some(Ok((texture, duration))) => {
                                let scale = (rect.width() / texture.size_vec2().x)
                                    .min(rect.height() / texture.size_vec2().y);
                                ui.painter().image(
                                    texture.id(),
                                    Rect::from_center_size(
                                        rect.center(),
                                        texture.size_vec2() * scale,
                                    ),
                                    Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                                    Color32::WHITE,
                                );
                                if let Some(duration) = duration {
                                    let label = format_time(media_time(*duration));
                                    let galley = ui.painter().layout_no_wrap(
                                        label,
                                        FontId::proportional(10.0),
                                        Color32::WHITE,
                                    );
                                    let label_rect = Rect::from_min_size(
                                        rect.right_bottom() - galley.size() - Vec2::splat(3.0),
                                        galley.size(),
                                    );
                                    ui.painter().rect_filled(
                                        label_rect.expand(1.0),
                                        1.0,
                                        Color32::from_black_alpha(210),
                                    );
                                    ui.painter().galley(label_rect.min, galley, Color32::WHITE);
                                }
                            }
                            value => {
                                ui.painter().text(
                                    rect.center(),
                                    Align2::CENTER_CENTER,
                                    if value.is_some() { "No preview" } else { "…" },
                                    FontId::proportional(12.0),
                                    Color32::GRAY,
                                );
                            }
                        }
                        if active || response.hovered() || response.has_focus() {
                            ui.painter().rect_stroke(
                                rect.expand(3.0),
                                2.0,
                                egui::Stroke::new(1.0, Color32::WHITE),
                                egui::StrokeKind::Inside,
                            );
                            let name_rect = Rect::from_min_size(
                                rect.min - egui::vec2(0.0, 22.0),
                                egui::vec2(rect.width(), 20.0),
                            );
                            ui.put(
                                name_rect,
                                egui::Label::new(
                                    egui::RichText::new(display_name(&item.path))
                                        .color(Color32::WHITE),
                                )
                                .truncate(),
                            );
                        }
                        if response.clicked() && !active {
                            actions.push(UiAction::OpenMedia(item.path.clone(), false));
                        }
                        if response.middle_clicked() {
                            actions.push(UiAction::OpenMedia(item.path.clone(), true));
                        }
                        let mut tooltip = item.path.display().to_string();
                        tooltip.push_str(
                            "\nDrag outside the window to open the source file in a new window.",
                        );
                        if let Some(Err(error)) = self.previews.get(&item.path) {
                            tooltip.push_str(&format!("\nPreview unavailable: {error}"));
                        }
                        response.help_text(tooltip);
                    }
                });
                self.scroll_offset = output.state.offset.x;
            });
        self.set_visible(wanted);
        self.drag.finish(context, actions);
    }

    pub fn show_recent(
        &mut self,
        ui: &mut egui::Ui,
        paths: &[PathBuf],
        enabled: bool,
        actions: &mut Vec<UiAction>,
    ) {
        let mut wanted = Vec::new();
        if paths.is_empty() {
            ui.label("No recent files yet.");
        }
        let width = ui.available_width();
        let columns = (((width + 8.0) / 164.0).floor() as usize).max(1);
        let cell_width = ((width - (columns - 1) as f32 * 8.0) / columns as f32).max(1.0);
        ui.add_enabled_ui(enabled, |ui| {
            for row in paths.chunks(columns) {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for path in row {
                        ui.push_id(path, |ui| {
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(cell_width, cell_width * 2.0 / 3.0 + 24.0),
                                egui::Sense::click(),
                            );
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    enabled,
                                    display_name(path),
                                )
                            });
                            ui.ctx().accesskit_node_builder(response.id, |node| {
                                node.set_description(path.display().to_string())
                            });
                            if response.gained_focus() {
                                response.scroll_to_me(None);
                            }
                            if !ui.is_rect_visible(rect) {
                                return;
                            }
                            if enabled && let Some(kind) = MediaKind::from_path(path) {
                                wanted.push((path.clone(), kind));
                            }
                            let image_rect = Rect::from_min_size(
                                rect.min,
                                egui::vec2(cell_width, cell_width * 2.0 / 3.0),
                            );
                            ui.painter()
                                .rect_filled(image_rect, 3.0, crate::chrome::BORDER);
                            match self.previews.get(path) {
                                Some(Ok((texture, duration))) => {
                                    let scale =
                                        (image_rect.size() / texture.size_vec2()).min_elem();
                                    let target = Rect::from_center_size(
                                        image_rect.center(),
                                        texture.size_vec2() * scale,
                                    );
                                    ui.painter().image(
                                        texture.id(),
                                        target,
                                        Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                                        Color32::WHITE,
                                    );
                                    if let Some(duration) = duration {
                                        ui.painter().text(
                                            image_rect.right_bottom() - egui::vec2(4.0, 3.0),
                                            Align2::RIGHT_BOTTOM,
                                            format_time(media_time(*duration)),
                                            FontId::proportional(11.0),
                                            Color32::WHITE,
                                        );
                                    }
                                }
                                value => {
                                    ui.painter().text(
                                        image_rect.center(),
                                        Align2::CENTER_CENTER,
                                        if value.is_some() { "No preview" } else { "…" },
                                        FontId::proportional(12.0),
                                        crate::chrome::MUTED,
                                    );
                                }
                            }
                            let name_rect = Rect::from_min_max(
                                egui::pos2(rect.left(), image_rect.bottom() + 4.0),
                                rect.max,
                            );
                            ui.scope_builder(
                                egui::UiBuilder::new()
                                    .max_rect(name_rect)
                                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(display_name(path))
                                                .color(crate::chrome::FOREGROUND)
                                                .size(12.0),
                                        )
                                        .halign(egui::Align::Min)
                                        .truncate(),
                                    );
                                },
                            );
                            if response.hovered() || response.has_focus() {
                                ui.painter().rect_stroke(
                                    image_rect,
                                    3.0,
                                    egui::Stroke::new(1.0, crate::chrome::FOREGROUND),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            if response.clicked() || response.middle_clicked() {
                                actions.push(UiAction::OpenMedia(path.clone(), true));
                            }
                            if enabled {
                                response.help_text(path.display().to_string());
                            }
                        });
                    }
                });
                ui.add_space(8.0);
            }
        });
        self.set_visible(wanted);
    }

    fn set_visible(&mut self, mut wanted: Vec<(PathBuf, MediaKind)>) {
        let visible: Vec<_> = wanted.iter().map(|(path, _)| path.clone()).collect();
        if visible != self.visible {
            self.previews.retain(|path, _| visible.contains(path));
            wanted.retain(|(path, _)| !self.previews.contains_key(path));
            self.generation = self.loader.request(wanted);
            self.visible = visible;
        }
    }
}

fn visible_range(viewport: Rect, padding: f32, count: usize) -> Range<usize> {
    let start = (((viewport.left() - padding) / STEP).floor().max(0.0) as usize).min(count);
    let end = (((viewport.right() - padding) / STEP).ceil().max(0.0) as usize).min(count);
    start..end.min(start + VISIBLE_PREVIEW_LIMIT)
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use towavue_core::{FolderMediaItem, FolderSnapshotSource, MediaKind, ShellIdentity};

    use super::*;

    #[test]
    fn recent_grid_uses_visible_shared_previews_and_blocks_background_actions() {
        let root = std::env::temp_dir().join(format!("towavue-recent-grid-{}", std::process::id()));
        let mut filmstrip = Filmstrip::new(PreviewCache::new(root.clone()).expect("cache"), || {})
            .expect("preview worker");
        let paths: Vec<_> = (0..40)
            .map(|index| root.join(format!("{index:02}-image.png")))
            .collect();
        let context = crate::fonts::test_context();
        context.enable_accesskit();
        let frame = |filmstrip: &mut Filmstrip, enabled, events| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(660.0, 260.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        filmstrip.show_recent(ui, &paths, enabled, &mut actions)
                    });
                },
            );
            (output, actions)
        };
        for _ in 0..3 {
            frame(&mut filmstrip, true, vec![]);
        }
        assert!(!filmstrip.visible.is_empty() && filmstrip.visible.len() < paths.len());
        let texture = context.load_texture(
            "recent-fixture",
            egui::ColorImage::filled([2, 1], Color32::RED),
            egui::TextureOptions::LINEAR,
        );
        let texture_id = texture.id();
        filmstrip
            .previews
            .insert(paths[0].clone(), Ok((texture, None)));
        let (output, actions) = frame(&mut filmstrip, true, vec![]);
        assert!(actions.is_empty());
        assert!(output.shapes.iter().any(
            |shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == texture_id)
        ));
        let image_left = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id == texture_id => {
                    Some(mesh.calc_bounds().left())
                }
                _ => None,
            })
            .expect("image mesh");
        let label_left = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "00-image.png" => Some(text.pos.x),
                _ => None,
            })
            .expect("filename label");
        assert!(
            (image_left - label_left).abs() < 1.0,
            "filename aligns with the card edge"
        );
        let tree = output
            .platform_output
            .accesskit_update
            .expect("accessible grid");
        let (node_id, node) = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                node.role() == egui::accesskit::Role::Button && node.label() == Some("00-image.png")
            })
            .expect("recent button");
        assert_eq!(
            node.description(),
            Some(paths[0].to_string_lossy().as_ref())
        );
        let event = egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
            action: egui::accesskit::Action::Click,
            target_tree: egui::accesskit::TreeId::ROOT,
            target_node: *node_id,
            data: None,
        });
        let (_, actions) = frame(&mut filmstrip, true, vec![event.clone()]);
        assert!(actions == [UiAction::OpenMedia(paths[0].clone(), true)]);
        let (_, actions) = frame(&mut filmstrip, false, vec![event]);
        assert!(actions.is_empty());
        assert!(filmstrip.visible.is_empty() && filmstrip.previews.is_empty());
        drop(filmstrip);
        std::fs::remove_dir(root).expect("remove empty owned cache");
    }

    #[test]
    fn keyboard_focus_request_reveals_current_once_and_waits_for_items() {
        let root =
            std::env::temp_dir().join(format!("towavue-filmstrip-focus-{}", std::process::id()));
        let mut strip =
            Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                .expect("worker");
        let snapshot = FolderSnapshot {
            folder_identity: ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: (0..50_000)
                .map(|index| FolderMediaItem {
                    identity: ShellIdentity::new(vec![]),
                    path: root.join(format!("{index}.png")),
                    kind: MediaKind::Image,
                })
                .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: SystemTime::now(),
        };
        let context = Context::default();
        context.enable_accesskit();
        let frame = |strip: &mut Filmstrip, snapshot, current, events| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events,
                    ..Default::default()
                },
                |_| {
                    strip.show(
                        &context,
                        context.content_rect(),
                        snapshot,
                        current,
                        true,
                        &mut actions,
                    )
                },
            );
            assert!(actions.is_empty(), "focus requests do not open media");
            output.platform_output.accesskit_update.expect("tree")
        };
        strip.focus_current();
        frame(&mut strip, None, None, vec![]);
        assert!(strip.focus_requested, "wait for the folder snapshot");
        for _ in 0..3 {
            frame(
                &mut strip,
                Some(&snapshot),
                Some(&snapshot.items[0].path),
                vec![],
            );
        }
        let tree = frame(
            &mut strip,
            Some(&snapshot),
            Some(&snapshot.items[0].path),
            vec![],
        );
        assert_eq!(
            tree.nodes
                .iter()
                .find(|(id, _)| *id == tree.focus)
                .expect("focused first item")
                .1
                .label(),
            Some("0.png")
        );
        strip.focus_current();
        let tree = frame(
            &mut strip,
            Some(&snapshot),
            Some(&snapshot.items[49_999].path),
            vec![],
        );
        assert_eq!(
            tree.nodes
                .iter()
                .find(|(id, _)| *id == tree.focus)
                .expect("focused last item")
                .1
                .label(),
            Some("49999.png")
        );
        assert!(!strip.focus_requested);
        assert!(strip.visible.len() <= 9);
        let other = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                node.role() == egui::accesskit::Role::Button && node.label() == Some("49998.png")
            })
            .expect("previous item")
            .0;
        frame(
            &mut strip,
            Some(&snapshot),
            Some(&snapshot.items[49_999].path),
            vec![egui::Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Focus,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: other,
                    data: None,
                },
            )],
        );
        assert_eq!(
            frame(
                &mut strip,
                Some(&snapshot),
                Some(&snapshot.items[49_999].path),
                vec![]
            )
            .focus,
            other,
            "ordinary redraw does not steal focus"
        );
        strip.focus_current();
        strip.clear();
        assert!(
            !strip.focus_requested,
            "closing clears a pending focus request"
        );
    }

    #[test]
    fn accessible_items_follow_paths_and_keep_current_item_noop() {
        let root = std::env::temp_dir().join(format!(
            "towavue-filmstrip-accessibility-{}",
            std::process::id()
        ));
        let mut strip =
            Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                .expect("worker");
        let mut snapshot = FolderSnapshot {
            folder_identity: ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: (0..3)
                .map(|index| FolderMediaItem {
                    identity: ShellIdentity::new(vec![index]),
                    path: root.join(format!("{index}.png")),
                    kind: MediaKind::Image,
                })
                .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: SystemTime::now(),
        };
        let context = Context::default();
        context.enable_accesskit();
        let first = snapshot.items[0].path.clone();
        let target = snapshot.items[1].path.clone();
        let enabled = std::cell::Cell::new(true);
        let mut frame = |snapshot: &FolderSnapshot, current: &Path, events| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events,
                    ..Default::default()
                },
                |_| {
                    strip.show(
                        &context,
                        context.content_rect(),
                        Some(snapshot),
                        Some(current),
                        enabled.get(),
                        &mut actions,
                    )
                },
            );
            (
                output.platform_output.accesskit_update.expect("tree"),
                actions,
            )
        };
        frame(&snapshot, &first, vec![]);
        let (tree, _) = frame(&snapshot, &first, vec![]);
        let (id, node) = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                node.role() == egui::accesskit::Role::Button && node.label() == Some("1.png")
            })
            .expect("named filmstrip button");
        assert_eq!(node.description(), Some(target.to_string_lossy().as_ref()));
        let id = *id;
        let bounds = node.bounds().expect("item bounds");
        let position = egui::pos2(
            ((bounds.x0 + bounds.x1) / 2.0) as f32,
            ((bounds.y0 + bounds.y1) / 2.0) as f32,
        );
        frame(
            &snapshot,
            &first,
            vec![egui::Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Focus,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: id,
                    data: None,
                },
            )],
        );
        assert_eq!(frame(&snapshot, &first, vec![]).0.focus, id);
        enabled.set(false);
        assert_ne!(frame(&snapshot, &first, vec![]).0.focus, id);
        for button in [egui::PointerButton::Primary, egui::PointerButton::Middle] {
            for pressed in [true, false] {
                let (_, actions) = frame(
                    &snapshot,
                    &first,
                    vec![
                        egui::Event::PointerMoved(position),
                        egui::Event::PointerButton {
                            pos: position,
                            button,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
                assert!(
                    actions.is_empty(),
                    "disabled pointer action cannot select or open a tab"
                );
            }
        }
        for key in [egui::Key::Enter, egui::Key::Space] {
            let (_, actions) = frame(
                &snapshot,
                &first,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert!(actions.is_empty());
        }
        enabled.set(true);
        frame(&snapshot, &first, vec![]);
        let click = || {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Click,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: id,
                data: None,
            })
        };
        snapshot.items.swap(0, 1);
        let mut focus = click();
        if let egui::Event::AccessKitActionRequest(request) = &mut focus {
            request.action = egui::accesskit::Action::Focus;
        }
        frame(&snapshot, &first, vec![focus]);
        assert_eq!(frame(&snapshot, &first, vec![]).0.focus, id);
        assert!(
            frame(&snapshot, &first, vec![click()]).1
                == [UiAction::OpenMedia(target.clone(), false)]
        );
        frame(&snapshot, &target, vec![]);
        assert!(frame(&snapshot, &target, vec![click()]).1.is_empty());
        snapshot.items.remove(0);
        frame(&snapshot, &first, vec![]);
        assert!(frame(&snapshot, &first, vec![click()]).1.is_empty());
    }

    #[test]
    fn seek_preview_reuses_visible_results_and_preserves_page_layout() {
        let root =
            std::env::temp_dir().join(format!("towavue-seek-preview-{}", std::process::id()));
        let cache = PreviewCache::new(root.join("cache")).expect("cache");
        let mut strip = Filmstrip::new(cache, || {}).expect("worker");
        let context = Context::default();
        context.global_style_mut(|style| style.interaction.tooltip_delay = 0.0);
        let first = root.join("first.png");
        let second = root.join("second.png");
        let textures =
            [(Color32::RED, [80, 160]), (Color32::BLUE, [240, 120])].map(|(color, size)| {
                context.load_texture(
                    format!("{color:?}"),
                    egui::ColorImage::filled(size, color),
                    Default::default(),
                )
            });
        for (path, texture) in [(&first, &textures[0]), (&second, &textures[1])] {
            strip
                .previews
                .insert(path.clone(), Ok((texture.clone(), None)));
        }
        let track = Rect::from_min_max(egui::pos2(8.0, 260.0), egui::pos2(472.0, 272.0));
        let mut time = 0.0;
        let mut draw = |strip: &mut Filmstrip, paths: &[PathBuf], axis| {
            let mut output = egui::FullOutput::default();
            for _ in 0..5 {
                time += 1.0;
                output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(480.0, 300.0),
                        )),
                        time: Some(time),
                        events: vec![egui::Event::PointerMoved(track.center())],
                        ..Default::default()
                    },
                    |ui| {
                        let response = ui.allocate_rect(track, egui::Sense::hover());
                        strip.show_seek_preview(&response, 0.5, paths, "2–3 / 4  first.png", axis);
                    },
                );
            }
            output
        };
        for axis in [ReadingAxis::Horizontal, ReadingAxis::Vertical] {
            for reversed in [false, true] {
                let paths = if reversed {
                    [second.clone(), first.clone()]
                } else {
                    [first.clone(), second.clone()]
                };
                let output = draw(&mut strip, &paths, axis);
                assert_eq!(strip.visible, paths);
                let bounds = textures.each_ref().map(|texture| {
                    output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Mesh(mesh) if mesh.texture_id == texture.id() => {
                                Some(mesh.calc_bounds())
                            }
                            _ => None,
                        })
                        .expect("page preview")
                });
                let [first, second] = if reversed {
                    [bounds[1], bounds[0]]
                } else {
                    bounds
                };
                let seam = match axis {
                    ReadingAxis::Horizontal => first.right() - second.left(),
                    ReadingAxis::Vertical => first.bottom() - second.top(),
                };
                assert!(seam.abs() < 0.001, "preview pages must touch");
                assert!((bounds[0].aspect_ratio() - 0.5).abs() < 0.001);
                assert!((bounds[1].aspect_ratio() - 2.0).abs() < 0.001);
                assert!(bounds.iter().all(|rect| rect.bottom() < track.top()
                    && rect.width() <= 160.0
                    && rect.height() <= 108.0));
                let coordinate = |rect: Rect| match axis {
                    ReadingAxis::Horizontal => rect.center().x,
                    ReadingAxis::Vertical => rect.center().y,
                };
                assert_eq!(coordinate(bounds[0]) > coordinate(bounds[1]), reversed);
                let generation = strip.generation;
                draw(&mut strip, &paths, axis);
                assert_eq!(
                    strip.generation, generation,
                    "unchanged hover must not request again"
                );
            }
        }
        let snapshot = FolderSnapshot {
            folder_identity: ShellIdentity::new(Vec::new()),
            folder_path: root.clone(),
            items: [first.clone(), second.clone()]
                .into_iter()
                .map(|path| FolderMediaItem {
                    identity: ShellIdentity::new(Vec::new()),
                    path,
                    kind: MediaKind::Image,
                })
                .collect(),
            sort_columns: Vec::new(),
            source: FolderSnapshotSource::NaturalNameFallback,
            generation: 1,
            captured_at: SystemTime::now(),
        };
        for pass in 0..3 {
            let output = context.run_ui(Default::default(), |_| {
                strip.show(
                    &context,
                    context.content_rect(),
                    Some(&snapshot),
                    Some(&first),
                    true,
                    &mut Vec::new(),
                );
            });
            if pass < 2 {
                continue;
            }
            for (texture, ratio) in [(&textures[0], 0.5), (&textures[1], 2.0)] {
                let rect = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Mesh(mesh) if mesh.texture_id == texture.id() => {
                            Some(mesh.calc_bounds())
                        }
                        _ => None,
                    })
                    .expect("filmstrip card");
                assert!(
                    (rect.aspect_ratio() - ratio).abs() < 0.001,
                    "unpadded card must not stretch"
                );
            }
        }
        strip
            .previews
            .insert(first.clone(), Err("broken page".into()));
        let output = draw(
            &mut strip,
            &[first.clone(), second],
            ReadingAxis::Horizontal,
        );
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "No preview")));
        let generation = strip.generation;
        let paths = strip.visible.clone();
        draw(&mut strip, &paths, ReadingAxis::Horizontal);
        assert_eq!(strip.generation, generation, "failed page must not loop");
        strip.clear();
        assert!(strip.visible.is_empty() && strip.previews.is_empty());
        drop(strip);
        std::fs::remove_dir_all(root).expect("remove owned preview cache");
    }

    #[test]
    fn visible_range_clamps_empty_edges_and_large_viewports() {
        let viewport =
            |left, width| Rect::from_min_size(egui::pos2(left, 0.0), egui::vec2(width, HEIGHT));
        assert_eq!(visible_range(viewport(0.0, 960.0), 416.0, 0), 0..0);
        assert_eq!(visible_range(viewport(0.0, 960.0), 416.0, 50_000), 0..5);
        assert_eq!(
            visible_range(viewport(128_000.0, 960.0), 416.0, 50_000),
            996..1005
        );
        assert_eq!(
            visible_range(viewport(128_000.0, 960.0), 416.0, 1000),
            996..1000
        );
        assert_eq!(visible_range(viewport(128_000.0, 960.0), 416.0, 10), 10..10);
        assert_eq!(
            visible_range(viewport(0.0, 100_000.0), 0.0, 50_000),
            0..VISIBLE_PREVIEW_LIMIT
        );
    }

    #[test]
    fn large_folder_virtualizes_recenters_and_preserves_click_intents() {
        let root = std::env::temp_dir().join(format!(
            "towavue-filmstrip-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let cache = PreviewCache::new(root.join("cache")).expect("test cache");
        let mut filmstrip = Filmstrip::new(cache, || {}).expect("preview worker");
        let snapshot = FolderSnapshot {
            folder_identity: ShellIdentity::new(Vec::new()),
            folder_path: root.clone(),
            items: (0..50_000)
                .map(|index| FolderMediaItem {
                    identity: ShellIdentity::new(Vec::new()),
                    path: root.join(format!("{index}.png")),
                    kind: MediaKind::Image,
                })
                .collect(),
            sort_columns: Vec::new(),
            source: FolderSnapshotSource::NaturalNameFallback,
            generation: 1,
            captured_at: SystemTime::now(),
        };
        let context = Context::default();
        let mut time = 0.0;
        let mut frame = |filmstrip: &mut Filmstrip, selected: usize, events| {
            let mut actions = Vec::new();
            let _ = context.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events,
                    ..Default::default()
                },
                |_| {
                    filmstrip.show(
                        &context,
                        context.content_rect(),
                        Some(&snapshot),
                        Some(&snapshot.items[selected].path),
                        true,
                        &mut actions,
                    )
                },
            );
            time += 0.1;
            actions
        };
        for selected in [25_000, 30_000] {
            for _ in 0..3 {
                frame(&mut filmstrip, selected, Vec::new());
            }
            assert!(filmstrip.visible.len() <= 9);
            assert!(filmstrip.visible.contains(&snapshot.items[selected].path));
        }
        for (button, x, expected) in [
            (egui::PointerButton::Primary, 480.0, None),
            (egui::PointerButton::Primary, 608.0, Some((30_001, false))),
            (egui::PointerButton::Middle, 480.0, Some((30_000, true))),
        ] {
            let pos = egui::pos2(x, 293.0);
            frame(&mut filmstrip, 30_000, vec![egui::Event::PointerMoved(pos)]);
            let mut actions = Vec::new();
            for pressed in [true, false] {
                actions.extend(frame(
                    &mut filmstrip,
                    30_000,
                    vec![egui::Event::PointerButton {
                        pos,
                        button,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    }],
                ));
            }
            if let Some((index, new_tab)) = expected {
                assert!(
                    matches!(actions.as_slice(), [UiAction::OpenMedia(path, actual)] if path == &snapshot.items[index].path && *actual == new_tab)
                );
            } else {
                assert!(actions.is_empty());
            }
        }
        let before_scroll = filmstrip.visible.clone();
        let above = egui::pos2(480.0, 80.0);
        frame(
            &mut filmstrip,
            30_000,
            vec![egui::Event::PointerMoved(above)],
        );
        frame(
            &mut filmstrip,
            30_000,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -STEP * 3.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        for _ in 0..20 {
            frame(&mut filmstrip, 30_000, Vec::new());
        }
        assert_ne!(filmstrip.visible, before_scroll);
        assert_eq!(filmstrip.focus.as_ref(), Some(&snapshot.items[30_000].path));
        let retained_visible = filmstrip.visible.clone();
        let retained_offset = filmstrip.scroll_offset;
        let retained_view = filmstrip.take_view();
        frame(&mut filmstrip, 5000, Vec::new());
        assert_ne!(filmstrip.scroll_offset, retained_offset);
        filmstrip.restore_view(retained_view);
        frame(&mut filmstrip, 30_000, Vec::new());
        assert_eq!(filmstrip.scroll_offset, retained_offset);
        assert_eq!(filmstrip.visible, retained_visible);
        filmstrip.clear_previews();
        frame(&mut filmstrip, 30_000, Vec::new());
        assert_eq!(filmstrip.scroll_offset, retained_offset);
        assert_eq!(filmstrip.visible, retained_visible);
        filmstrip
            .previews
            .insert(snapshot.items[30_000].path.clone(), Err("fixture".into()));
        frame(&mut filmstrip, 40_000, Vec::new());
        assert!(filmstrip.previews.is_empty());
        filmstrip.clear();
        assert!(filmstrip.visible.is_empty());
        assert!(filmstrip.focus.is_none());
        let generation = filmstrip.generation;
        filmstrip.clear();
        assert_eq!(filmstrip.generation, generation);
        drop(filmstrip);
        std::fs::remove_dir_all(root).expect("remove test cache");
    }
}
