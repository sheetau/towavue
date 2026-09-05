use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::{Align2, Color32, Context, FontId, Rect, TextureHandle, Vec2};
use towavue_core::FolderSnapshot;
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
                        let response = ui.allocate_rect(rect, egui::Sense::click());
                        let active = selected == Some(index);
                        ui.painter().rect_filled(rect, 0.0, Color32::from_gray(28));
                        match self.previews.get(&item.path) {
                            Some(Ok((texture, duration))) => {
                                ui.painter().image(
                                    texture.id(),
                                    rect,
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
                        if active || response.hovered() {
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
