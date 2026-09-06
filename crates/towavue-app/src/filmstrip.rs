use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::{Align2, Color32, Context, FontId, Rect, TextureHandle, Vec2};
use towavue_core::{FolderSnapshot, MediaKind, ReadingAxis};
use towavue_runtime_windows::{PreviewCache, PreviewLoader, VISIBLE_PREVIEW_LIMIT};

use crate::{UiAction, display_name, format_time, media_time};

const STEP: f32 = 128.0;
const HEIGHT: f32 = 118.0;

type Preview = Result<(TextureHandle, Option<Duration>), String>;

pub struct Filmstrip {
    loader: PreviewLoader,
    generation: u64,
    visible: Vec<PathBuf>,
    previews: HashMap<PathBuf, Preview>,
    focus: Option<PathBuf>,
}

impl Filmstrip {
    pub fn new(cache: PreviewCache, notify: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        Ok(Self {
            loader: PreviewLoader::new(cache, notify)?,
            generation: 0,
            visible: Vec::new(),
            previews: HashMap::new(),
            focus: None,
        })
    }

    pub fn clear(&mut self) {
        if !self.visible.is_empty() || self.focus.is_some() {
            self.generation = self.loader.request(Vec::new());
            self.visible.clear();
            self.previews.clear();
            self.focus = None;
        }
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
        snapshot: Option<&FolderSnapshot>,
        current: Option<&Path>,
        actions: &mut Vec<UiAction>,
    ) {
        let screen = context.content_rect();
        context
            .layer_painter(egui::LayerId::new(
                egui::Order::Middle,
                "filmstrip-dim".into(),
            ))
            .rect_filled(screen, 0.0, Color32::from_black_alpha(140));
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
            .fixed_pos(egui::pos2(screen.left(), screen.center().y - HEIGHT / 2.0))
            .constrain(false)
            .show(context, |ui| {
                ui.set_width(screen.width());
                ui.style_mut().always_scroll_the_only_direction = true;
                let mut scroll = egui::ScrollArea::horizontal()
                    .id_salt("filmstrip-scroll")
                    .auto_shrink([false, false])
                    .max_height(HEIGHT)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
                if recenter {
                    scroll = scroll.horizontal_scroll_offset(selected.unwrap_or(0) as f32 * STEP);
                }
                scroll.show_viewport(ui, |ui, viewport| {
                    let padding = ((screen.width() - STEP) / 2.0).max(0.0);
                    let origin = ui.min_rect().min;
                    ui.set_min_size(egui::vec2(
                        snapshot.items.len() as f32 * STEP + padding * 2.0,
                        HEIGHT,
                    ));
                    for index in visible_range(viewport, padding, snapshot.items.len()) {
                        let item = &snapshot.items[index];
                        wanted.push((item.path.clone(), item.kind));
                        let rect = Rect::from_min_size(
                            origin + egui::vec2(padding + index as f32 * STEP + 4.0, 24.0),
                            egui::vec2(120.0, 80.0),
                        );
                        let response = ui.interact(
                            rect,
                            ui.id().with(("filmstrip-item", &item.path)),
                            egui::Sense::click(),
                        );
                        let active = selected == Some(index);
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
                        ui.painter().rect_filled(rect, 0.0, Color32::from_gray(28));
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
                        if let Some(Err(error)) = self.previews.get(&item.path) {
                            tooltip.push_str(&format!("\nPreview unavailable: {error}"));
                        }
                        response.on_hover_text(tooltip);
                    }
                });
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
                |_| strip.show(&context, Some(snapshot), Some(current), &mut actions),
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
                strip.show(&context, Some(&snapshot), Some(&first), &mut Vec::new());
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
                        Some(&snapshot),
                        Some(&snapshot.items[selected].path),
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
