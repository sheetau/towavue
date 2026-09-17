use crate::scroll_style::ScrollAreaStyle;
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

struct Preparation {
    current: PathBuf,
    snapshot: u64,
    next: usize,
}

mod drag;
mod swipe;

#[cfg(test)]
pub(crate) mod drag_tests;

#[cfg(test)]
mod duration_tests;

#[derive(Default)]
pub struct View {
    focus: Option<PathBuf>,
    offset: f32,
}

pub struct RecentGrid {
    columns: usize,
    row_height: f32,
}

impl RecentGrid {
    pub fn offset(&self, index: usize) -> f32 {
        (index / self.columns) as f32 * self.row_height
    }
}

pub struct Filmstrip {
    loader: PreviewLoader,
    generation: u64,
    visible: Vec<PathBuf>,
    previews: HashMap<PathBuf, Preview>,
    preview_order: Vec<PathBuf>,
    refreshing: Vec<PathBuf>,
    preparation: Option<Preparation>,
    warming: Option<PathBuf>,
    focus: Option<PathBuf>,
    focus_requested: bool,
    focused_card: Option<egui::Id>,
    recent_focus: Option<PathBuf>,
    pointer_position: Option<egui::Pos2>,
    card_paths: Vec<(egui::Id, PathBuf, usize)>,
    tab_navigation: Option<(u64, usize)>,
    scroll_offset: f32,
    drag: drag::State,
    swipe: swipe::State,
}

impl Filmstrip {
    pub fn new(cache: PreviewCache, notify: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        Ok(Self {
            loader: PreviewLoader::new(cache, notify)?,
            generation: 0,
            visible: Vec::new(),
            previews: HashMap::new(),
            preview_order: Vec::new(),
            refreshing: Vec::new(),
            preparation: None,
            warming: None,
            focus: None,
            focus_requested: false,
            focused_card: None,
            recent_focus: None,
            pointer_position: None,
            card_paths: Vec::new(),
            tab_navigation: None,
            scroll_offset: 0.0,
            drag: drag::State::default(),
            swipe: swipe::State::default(),
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
        self.swipe.clear();
    }

    pub(crate) fn cancel_native_drag(&mut self, context: &Context) -> bool {
        self.swipe.clear();
        self.drag.cancel(context)
    }

    pub(crate) fn active_drag(
        &self,
        context: &Context,
        current: Option<&Path>,
    ) -> Option<(&Path, u64, egui::Pos2)> {
        self.drag.active_pointer(context, current)
    }

    pub fn clear_previews(&mut self) {
        self.preparation = None;
        self.warming = None;
        self.drag.clear();
        self.swipe.clear();
        self.focused_card = None;
        self.card_paths.clear();
        self.tab_navigation = None;
        if !self.visible.is_empty() {
            self.generation = self.loader.request(Vec::new());
            self.visible.clear();
        }
        self.previews.clear();
        self.preview_order.clear();
        self.refreshing.clear();
    }

    pub fn pause_preparation(&mut self) {
        self.warming = None;
        self.cancel_drag();
        self.focus_requested = false;
        self.focused_card = None;
        self.card_paths.clear();
        self.tab_navigation = None;
        if !self.visible.is_empty() {
            self.generation = self.loader.request(Vec::new());
            // Reopening rebuilds the wanted set, retaining ready textures and
            // restarting missing/revalidation jobs without accepting old results.
            self.visible.clear();
        }
    }

    pub fn refresh_previews(&mut self, snapshot: &FolderSnapshot, open: bool) {
        self.preparation = None;
        self.warming = None;
        if !open {
            self.pause_preparation();
            // Keep ready pixels until use permits revalidation, including textures
            // retained while the folder refresh was still pending.
            self.refreshing = self.previews.keys().cloned().collect();
            return;
        }
        // Revalidate on the worker without replacing displayed textures with placeholders.
        // Cancel old completions as they may predate a source-file change.
        self.loader.request(Vec::new());
        // Retained offscreen textures must not bypass source validation on reuse.
        self.refreshing = self.previews.keys().cloned().collect();
        self.generation = self.loader.request(
            self.visible
                .iter()
                .filter_map(|path| {
                    snapshot
                        .items
                        .iter()
                        .find(|item| &item.path == path)
                        .map(|item| (path.clone(), item.kind))
                })
                .collect(),
        );
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

    pub fn prepare_neighbors(&mut self, snapshot: Option<&FolderSnapshot>, current: Option<&Path>) {
        let Some((snapshot, selected)) = snapshot.and_then(|snapshot| {
            snapshot
                .items
                .iter()
                .position(|item| Some(item.path.as_path()) == current)
                .map(|selected| (snapshot, selected))
        }) else {
            self.clear();
            return;
        };
        self.cancel_drag();
        self.focus_requested = false;
        self.focus = None;
        self.scroll_offset = 0.0;
        let count = snapshot.items.len();
        let wanted = (0..count.min(VISIBLE_PREVIEW_LIMIT))
            .map(|rank| {
                let item = &snapshot.items[neighbor_index(selected, count, rank)];
                (item.path.clone(), item.kind)
            })
            .collect();
        self.set_visible(wanted);
        if self.preparation.as_ref().is_none_or(|preparation| {
            preparation.current != snapshot.items[selected].path
                || preparation.snapshot != snapshot.generation
        }) {
            if self.warming.take().is_some() {
                self.generation = self.loader.request(Vec::new());
            }
            self.preparation = Some(Preparation {
                current: snapshot.items[selected].path.clone(),
                snapshot: snapshot.generation,
                next: count.min(VISIBLE_PREVIEW_LIMIT),
            });
        }
        if self.warming.is_some()
            || self
                .visible
                .iter()
                .any(|path| !self.previews.contains_key(path) || self.refreshing.contains(path))
        {
            return;
        }
        let preparation = self.preparation.as_mut().expect("preparation origin");
        while preparation.next < count {
            let item = &snapshot.items[neighbor_index(selected, count, preparation.next)];
            // Plain BMP previews are memory-only. Scanning beyond the retained
            // working set would just evict useful pixels without warming disk.
            if item
                .path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("bmp"))
            {
                preparation.next += 1;
                continue;
            }
            self.warming = Some(item.path.clone());
            self.generation = self.loader.request(vec![(item.path.clone(), item.kind)]);
            break;
        }
    }

    pub fn finish(&mut self, context: &Context) -> bool {
        let mut changed = false;
        for preview in self.loader.take_completed() {
            if preview.generation != self.generation {
                continue;
            }
            if self.warming.as_ref() == Some(&preview.path) {
                self.warming = None;
                if let Some(preparation) = &mut self.preparation {
                    preparation.next += 1;
                }
                // Cache-only completion: never evict/upload retained visible
                // textures or request a frame just to advance background work.
                continue;
            }
            if !self.visible.contains(&preview.path) {
                continue;
            }
            self.refreshing.retain(|path| path != &preview.path);
            let result = preview.result.map(|media| {
                let image = media.image;
                let texture = context.load_texture(
                    format!("filmstrip:{}", preview.path.display()),
                    crate::image_color::preview_color_image(&image),
                    egui::TextureOptions::LINEAR,
                );
                (texture, media.duration)
            });
            self.previews.insert(preview.path, result);
            changed = true;
        }
        changed
    }

    pub(super) fn image_tab_preview(
        &mut self,
        paths: &[PathBuf],
        settings: towavue_core::ReadingSettings,
    ) -> crate::tab_preview::RetainedPreview {
        self.set_visible(
            paths
                .iter()
                .map(|path| (path.clone(), MediaKind::Image))
                .collect(),
        );
        crate::tab_preview::RetainedPreview::Reading {
            pages: paths
                .iter()
                .map(|path| match self.previews.get(path) {
                    Some(Ok((texture, _))) => (Some(texture.clone()), texture.size_vec2()),
                    _ => (None, Vec2::splat(1.0)),
                })
                .collect(),
            settings,
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
                        crate::media_preview::image(
                            ui,
                            texture.id(),
                            cell,
                            Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                            rect,
                        );
                    }
                    Some(Err(_)) => {
                        ui.painter().with_clip_rect(cell).text(
                            cell.center(),
                            Align2::CENTER_CENTER,
                            "No preview",
                            FontId::proportional(11.0),
                            Color32::GRAY,
                        );
                    }
                    None => {}
                }
            }
            crate::media_preview::caption(ui, |ui| {
                ui.add(egui::Label::new(caption).truncate());
            });
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
        self.drag.begin(context, snapshot, current, screen, enabled);
        let pointer = context.input(|input| input.pointer.hover_pos());
        let pointer_moved = pointer != self.pointer_position
            && context.input(|input| {
                input.focused
                    && !input.pointer.any_down()
                    && input
                        .events
                        .iter()
                        .any(|event| matches!(event, egui::Event::PointerMoved(_)))
            });
        self.pointer_position = pointer;
        context
            .layer_painter(egui::LayerId::new(
                egui::Order::Middle,
                "filmstrip-dim".into(),
            ))
            .rect_filled(screen, 0.0, Color32::from_black_alpha(191));
        let recenter = current != self.focus.as_deref();
        self.focus = current.map(Path::to_owned);
        let selected = snapshot.and_then(|snapshot| {
            snapshot
                .items
                .iter()
                .position(|item| Some(item.path.as_path()) == current)
        });
        let frame = context.cumulative_frame_nr();
        let can_focus =
            enabled && !egui::Popup::is_any_open(context) && context.input(|input| input.focused);
        let relocated_focus = if can_focus && !recenter && !self.focus_requested {
            context
                .memory(|memory| memory.focused())
                .and_then(|focused| {
                    let (_, path, index) =
                        self.card_paths.iter().find(|(id, _, _)| *id == focused)?;
                    let snapshot = snapshot?;
                    // A stable index needs no folder scan and must not undo manual scrolling.
                    if snapshot
                        .items
                        .get(*index)
                        .is_some_and(|item| &item.path == path)
                    {
                        return None;
                    }
                    snapshot
                        .items
                        .iter()
                        .position(|item| &item.path == path)
                        .or(selected)
                })
        } else {
            None
        };
        let tab_target = if can_focus {
            if let Some((_, target)) = self.tab_navigation.filter(|(saved, _)| *saved == frame) {
                Some(target)
            } else if let Some(count) =
                snapshot
                    .map(|snapshot| snapshot.items.len())
                    .filter(|count| {
                        *count > 0 && context.input(|input| input.key_pressed(egui::Key::Tab))
                    })
            {
                let mut target = if recenter || self.focus_requested {
                    selected
                } else {
                    // Directional focus resolves after layout; read the actual focus
                    // against the last bounded card set, then resolve its path in the
                    // latest folder order. Stored indices become stale after refresh.
                    context
                        .memory(|memory| memory.focused())
                        .and_then(|focused| {
                            let (_, path, _) =
                                self.card_paths.iter().find(|(id, _, _)| *id == focused)?;
                            snapshot?.items.iter().position(|item| item.path == *path)
                        })
                        .or(selected)
                }
                .unwrap_or(0)
                .min(count - 1);
                let mut changed = false;
                context.input_mut(|input| {
                    input.events.retain(|event| {
                        if let egui::Event::Key {
                            key: egui::Key::Tab,
                            pressed: true,
                            modifiers,
                            ..
                        } = event
                            && (*modifiers == egui::Modifiers::NONE
                                || *modifiers == egui::Modifiers::SHIFT)
                        {
                            target = if modifiers.shift {
                                (target + count - 1) % count
                            } else {
                                (target + 1) % count
                            };
                            changed = true;
                            false
                        } else {
                            true
                        }
                    })
                });
                changed.then_some(target)
            } else {
                None
            }
        } else {
            None
        };
        if let Some(target) = tab_target {
            // Own Tab traversal instead of egui's global widget order. Reuse this
            // target across discarded passes so one key cannot advance twice.
            self.tab_navigation = Some((frame, target));
            context.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
        }
        let focus_target = tab_target
            .or(relocated_focus)
            .or_else(|| self.focus_requested.then_some(selected).flatten());
        let mut wanted = Vec::new();
        let mut band = None;
        egui::Area::new("filmstrip".into())
            .order(egui::Order::Foreground)
            .fade_in(false)
            .movable(false)
            .sense(egui::Sense::click())
            .fixed_pos(screen.min)
            .constrain(false)
            .show(context, |ui| {
                if ui.is_sizing_pass() {
                    context.request_discard("resolve filmstrip sizing before presentation");
                }
                if !enabled || egui::Popup::is_any_open(context) {
                    let opacity = ui.opacity();
                    ui.disable();
                    ui.set_opacity(opacity);
                }
                ui.set_clip_rect(screen);
                ui.set_min_size(screen.size());
                ui.set_max_size(screen.size());
                let Some(snapshot) = snapshot else {
                    ui.painter().text(
                        screen.center(),
                        Align2::CENTER_CENTER,
                        "Loading folder order…",
                        FontId::proportional(14.0),
                        Color32::WHITE,
                    );
                    return;
                };
                ui.style_mut().always_scroll_the_only_direction = true;
                ui.spacing_mut().scroll.bar_width = 5.0;
                let inset = screen.shrink(8.0_f32.min(screen.size().min_elem().max(0.0) * 0.25));
                // Keep wheel ownership across the full dimmed panel, including
                // the gutter outside the inset scroll area's hit regions.
                let gutter_scroll = if ui.is_enabled()
                    && ui.rect_contains_pointer(screen)
                    && ui.input(|input| {
                        input
                            .pointer
                            .hover_pos()
                            .is_some_and(|p| !inset.contains(p))
                    }) {
                    let delta =
                        ui.input_mut(|input| std::mem::take(&mut input.smooth_scroll_delta));
                    delta.x + delta.y
                } else {
                    0.0
                };
                let background = ui.interact(screen, ui.id().with("filmstrip-swipe"), egui::Sense::drag());
                let padding = ((inset.width() - STEP) / 2.0).max(0.0);
                let content_width = snapshot.items.len() as f32 * STEP + padding * 2.0;
                self.swipe.update(
                    &background, snapshot, current, &mut self.scroll_offset,
                    (content_width - inset.width()).max(0.0),
                    recenter || self.focus_requested || tab_target.is_some() || relocated_focus.is_some(),
                );
                let mut scroll_ui = ui.new_child(egui::UiBuilder::new().max_rect(inset));
                let ui = &mut scroll_ui;
                let mut scroll = egui::ScrollArea::horizontal()
                    .id_salt("filmstrip-scroll")
                    .scroll_source(egui::scroll_area::ScrollSource {
                        drag: egui::scroll_area::DragScroll::Never,
                        ..Default::default()
                    })
                    // The fixed card geometry determines overflow before the first sizing pass.
                    .scroll_bar_visibility(if content_width > inset.width() {
                        egui::scroll_area::ScrollBarVisibility::AlwaysVisible
                    } else {
                        egui::scroll_area::ScrollBarVisibility::AlwaysHidden
                    })
                    .horizontal_scroll_offset(self.scroll_offset - gutter_scroll)
                    .auto_shrink([false, false])
                    .max_height(inset.height());
                if recenter
                    || self.focus_requested
                    || tab_target.is_some()
                    || relocated_focus.is_some()
                {
                    scroll = scroll.horizontal_scroll_offset(
                        focus_target.or(selected).unwrap_or(0) as f32 * STEP,
                    );
                }
                let output = scroll.show_viewport_styled(ui, |ui, viewport| {
                    let origin = ui.min_rect().min;
                    ui.set_min_size(egui::vec2(content_width, viewport.height()));
                    let range = visible_range(viewport, padding, snapshot.items.len());
                    let card_rect = |index| {
                        Rect::from_min_size(
                            origin
                                + egui::vec2(
                                    padding + index as f32 * STEP + 4.0,
                                    (viewport.height() - HEIGHT) / 2.0 + 24.0,
                                ),
                            egui::vec2(120.0, 80.0),
                        )
                    };
                    // Set the shared target before creating any button so queued
                    // pointer or Tab navigation + Enter cannot activate the old target.
                    let pointer_target =
                        (pointer_moved && ui.is_enabled() && focus_target.is_none())
                            .then(|| {
                                range
                                    .clone()
                                    .find(|index| ui.rect_contains_pointer(card_rect(*index)))
                            })
                            .flatten()
                            .map(|index| {
                                ui.id()
                                    .with(("filmstrip-item", &snapshot.items[index].path))
                            });
                    let requested_target = focus_target
                        .filter(|index| ui.is_enabled() && range.contains(index))
                        .map(|index| {
                            ui.id()
                                .with(("filmstrip-item", &snapshot.items[index].path))
                        })
                        .or(pointer_target);
                    if let Some(id) = requested_target {
                        context.memory_mut(|memory| memory.request_focus(id));
                    }
                    let mut cards = Vec::new();
                    let mut focused_card = None;
                    self.card_paths.clear();
                    for index in range {
                        let item = &snapshot.items[index];
                        wanted.push((item.path.clone(), item.kind));
                        let rect = card_rect(index);
                        let response = ui
                            .interact(
                                rect,
                                ui.id().with(("filmstrip-item", &item.path)),
                                egui::Sense::click_and_drag(),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        self.card_paths
                            .push((response.id, item.path.clone(), index));
                        let active = selected == Some(index);
                        self.drag.observe(&response, &item.path);
                        if focus_target == Some(index)
                            && response.enabled()
                            && !egui::Popup::is_any_open(context)
                        {
                            self.focus_requested = false;
                        }
                        crate::tab_focus::observe(&response, ("filmstrip-item", &item.path));
                        if response.has_focus() {
                            focused_card = Some(response.id);
                            // Arrow navigation resolves after layout, so gained_focus alone
                            // cannot observe every transition on the following frame.
                            if self.focused_card != focused_card && pointer_target != focused_card {
                                response.scroll_to_me(None);
                            }
                        }
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
                        cards.push((index, response));
                    }
                    self.focused_card = focused_card;
                    let highlighted = cards
                        .iter()
                        .find(|(_, response)| response.has_focus())
                        .map(|(index, _)| *index)
                        .or(selected);
                    for (index, response) in cards {
                        let item = &snapshot.items[index];
                        let rect = response.rect;
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
                                    draw_preview_duration(
                                        ui,
                                        rect,
                                        *duration,
                                        10.0,
                                        Vec2::splat(3.0),
                                    );
                                }
                            }
                            Some(Err(_)) => {
                                ui.painter().text(
                                    rect.center(),
                                    Align2::CENTER_CENTER,
                                    "No preview",
                                    FontId::proportional(12.0),
                                    Color32::GRAY,
                                );
                            }
                            None => {}
                        }
                        if highlighted == Some(index) {
                            ui.painter().rect_stroke(
                                rect.expand(3.0),
                                2.0,
                                egui::Stroke::new(1.0, Color32::WHITE),
                                egui::StrokeKind::Inside,
                            );
                            let mut label = egui::text::LayoutJob::simple(
                                display_name(&item.path),
                                egui::TextStyle::Body.resolve(ui.style()),
                                Color32::WHITE,
                                screen.width().min(192.0),
                            );
                            label.wrap.max_rows = 2;
                            label.wrap.break_anywhere = true;
                            label.halign = egui::Align::Center;
                            let label = ui.fonts_mut(|fonts| fonts.layout_job(label));
                            ui.painter().galley(
                                egui::pos2(rect.center().x, rect.top() - 10.0 - label.size().y),
                                label,
                                Color32::WHITE,
                            );
                        }
                        if response.clicked() {
                            actions.push(UiAction::OpenFilmstripMedia(item.path.clone(), false));
                        }
                        if response.middle_clicked() {
                            actions.push(UiAction::OpenFilmstripMedia(item.path.clone(), true));
                        }
                        let mut tooltip = item.path.display().to_string();
                        tooltip.push_str(
                            "\nDrag beyond the thumbnail band, then outside the window to open in a new window.",
                        );
                        if let Some(Err(error)) = self.previews.get(&item.path) {
                            tooltip.push_str(&format!("\nPreview unavailable: {error}"));
                        }
                        response.help_text(tooltip);
                    }
                });
                self.scroll_offset = output.state.offset.x;
                self.swipe.exclusions.clear();
                self.swipe.exclusions.extend(self.card_paths.iter().filter_map(|(id, _, _)| {
                    context.read_response(*id).map(|response| response.interact_rect)
                }));
                if let Some(bar) = context.read_response(output.id.with(0usize)) {
                    self.swipe.exclusions.push(bar.interact_rect);
                }
                band = Some(Rect::from_center_size(
                    egui::pos2(screen.center().x, output.inner_rect.center().y),
                    egui::vec2(screen.width(), HEIGHT),
                ).intersect(screen));
            });
        self.set_visible(wanted);
        self.drag.finish(context, band, actions);
    }

    pub fn show_recent(
        &mut self,
        ui: &mut egui::Ui,
        paths: &[PathBuf],
        enabled: bool,
        actions: &mut Vec<UiAction>,
    ) -> RecentGrid {
        let mut wanted = Vec::new();
        let origin = ui.cursor().top();
        let width = ui.available_width();
        let columns = (((width + 8.0) / 164.0).floor() as usize).max(1);
        let cell_width = ((width - (columns - 1) as f32 * 8.0) / columns as f32).max(1.0);
        let cell_height = cell_width * 2.0 / 3.0 + 24.0;
        let row_height = cell_height + 8.0 + ui.spacing().item_spacing.y;
        let row_count = paths.len().div_ceil(columns);
        let focus = self
            .recent_focus
            .as_ref()
            .and_then(|path| paths.iter().position(|candidate| candidate == path));
        let rows = recent_rows(
            ui.clip_rect(),
            origin,
            row_height,
            row_count,
            focus.map(|i| i / columns),
        );
        let mut focused_card = None;
        let mut recent_focus = None;
        let opacity = ui.opacity();
        ui.add_enabled_ui(enabled, |ui| {
            // Covered cards keep their appearance; only their input is disabled.
            ui.set_opacity(opacity);
            let (grid, _) = ui.allocate_exact_size(
                egui::vec2(width, row_count as f32 * row_height),
                egui::Sense::hover(),
            );
            // Only visible rows and the focused row's neighbors need widgets. Keeping
            // those neighbors lets egui's normal arrow/Tab navigation cross viewports.
            for row in rows.into_iter().flatten() {
                let start = row * columns;
                for (column, path) in paths[start..(start + columns).min(paths.len())]
                    .iter()
                    .enumerate()
                {
                    let rect = Rect::from_min_size(
                        grid.min
                            + egui::vec2(
                                column as f32 * (cell_width + 8.0),
                                row as f32 * row_height,
                            ),
                        egui::vec2(cell_width, cell_height),
                    );
                    let mut card_ui =
                        ui.new_child(egui::UiBuilder::new().id_salt(path).max_rect(rect));
                    let ui = &mut card_ui;
                    let response = ui.interact(rect, ui.id().with("card"), egui::Sense::click());
                    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
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
                    if response.has_focus() {
                        focused_card = Some(response.id);
                        recent_focus = Some(path.clone());
                        if self.focused_card != focused_card {
                            response.scroll_to_me(None);
                        }
                    }
                    if !ui.is_rect_visible(rect) {
                        continue;
                    }
                    if wanted.len() < VISIBLE_PREVIEW_LIMIT
                        && let Some(kind) = MediaKind::from_path(path)
                        && !wanted.iter().any(|(existing, _)| existing == path)
                    {
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
                            let scale = (image_rect.size() / texture.size_vec2()).min_elem();
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
                                draw_preview_duration(
                                    ui,
                                    image_rect,
                                    *duration,
                                    11.0,
                                    egui::vec2(4.0, 3.0),
                                );
                            }
                        }
                        Some(Err(_)) => {
                            ui.painter().text(
                                image_rect.center(),
                                Align2::CENTER_CENTER,
                                "No preview",
                                FontId::proportional(12.0),
                                crate::chrome::MUTED,
                            );
                        }
                        None => {}
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
                    if response.middle_clicked() {
                        actions.push(UiAction::OpenGalleryBackground(path.clone()));
                    } else if response.clicked() {
                        actions.push(UiAction::OpenMedia(path.clone(), true));
                    }
                    if enabled {
                        response.help_text(path.display().to_string());
                    }
                }
            }
        });
        self.focused_card = focused_card;
        self.recent_focus = recent_focus;
        // Keep the same bounded preparation set while a menu or picker covers the grid.
        for path in paths {
            if wanted.len() == VISIBLE_PREVIEW_LIMIT {
                break;
            }
            if let Some(kind) = MediaKind::from_path(path)
                && !wanted.iter().any(|(existing, _)| existing == path)
            {
                wanted.push((path.clone(), kind));
            }
        }
        self.set_visible(wanted);
        RecentGrid {
            columns,
            row_height,
        }
    }

    fn set_visible(&mut self, mut wanted: Vec<(PathBuf, MediaKind)>) {
        let visible: Vec<_> = wanted.iter().map(|(path, _)| path.clone()).collect();
        if visible != self.visible {
            self.warming = None;
            // Reserve slots for current requests, then reuse recently wanted ready
            // textures. A brief seek hover or narrow strip must not undo idle work.
            self.preview_order.retain(|path| {
                !visible.is_empty()
                    && !visible.contains(path)
                    && self.previews.get(path).is_some_and(Result::is_ok)
                    && !self.refreshing.contains(path)
            });
            self.preview_order.splice(0..0, visible.iter().cloned());
            self.preview_order.truncate(VISIBLE_PREVIEW_LIMIT);
            self.previews
                .retain(|path, _| self.preview_order.contains(path));
            self.refreshing
                .retain(|path| self.preview_order.contains(path));
            wanted.retain(|(path, _)| {
                !self.previews.contains_key(path) || self.refreshing.contains(path)
            });
            self.generation = self.loader.request(wanted);
            self.visible = visible;
        }
    }
}

fn draw_preview_duration(
    ui: &egui::Ui,
    rect: Rect,
    duration: Duration,
    font_size: f32,
    inset: Vec2,
) {
    let galley = ui.painter().layout_no_wrap(
        format_time(media_time(duration)),
        FontId::proportional(font_size),
        Color32::WHITE,
    );
    let label_rect =
        Rect::from_min_size(rect.right_bottom() - galley.size() - inset, galley.size());
    ui.painter()
        .rect_filled(label_rect.expand(1.0), 1.0, Color32::from_black_alpha(210));
    ui.painter().galley(label_rect.min, galley, Color32::WHITE);
}

fn recent_rows(
    clip: Rect,
    origin: f32,
    height: f32,
    count: usize,
    focused: Option<usize>,
) -> [Range<usize>; 2] {
    let start = (((clip.top() - origin) / height).floor().max(0.0) as usize)
        .saturating_sub(1)
        .min(count);
    let end = ((((clip.bottom() - origin) / height).ceil().max(0.0) as usize) + 1).min(count);
    let mut ranges = [start..end, 0..0];
    if let Some(row) = focused {
        let focus = row.saturating_sub(1)..(row + 2).min(count);
        if focus.start <= end && focus.end >= start {
            ranges[0] = start.min(focus.start)..end.max(focus.end);
        } else if focus.start < start {
            ranges = [focus, start..end];
        } else {
            ranges[1] = focus;
        }
    }
    ranges
}

// Exact current, previous, next ordering, with O(1) access even in large folders.
fn neighbor_index(selected: usize, count: usize, rank: usize) -> usize {
    let paired = selected.min(count - selected - 1);
    if rank <= paired * 2 {
        if rank.is_multiple_of(2) {
            selected + rank / 2
        } else {
            selected - rank.div_ceil(2)
        }
    } else if selected > count - selected - 1 {
        selected - (rank - paired)
    } else {
        selected + (rank - paired)
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
    fn neighbor_ranks_match_distance_order_without_duplicates() {
        for count in 1_usize..100 {
            for selected in 0..count {
                let expected: Vec<_> = (0..count)
                    .flat_map(|distance| {
                        [
                            selected.checked_sub(distance),
                            (distance != 0 && selected + distance < count)
                                .then_some(selected + distance),
                        ]
                    })
                    .flatten()
                    .collect();
                let actual: Vec<_> = (0..count)
                    .map(|rank| neighbor_index(selected, count, rank))
                    .collect();
                assert_eq!(actual, expected);
            }
        }
    }

    #[test]
    fn idle_preparation_warms_beyond_retained_textures_and_resumes_after_pause() {
        warm_folder_trial(
            "filmstrip::tests::idle_preparation_warms_beyond_retained_textures_and_resumes_after_pause",
            false,
        );
    }

    #[test]
    #[ignore = "generates 96 large PNGs and checks full-folder thumbnail preparation"]
    fn large_folder_preparation_retains_pixels_without_ui_uploads() {
        warm_folder_trial(
            "filmstrip::tests::large_folder_preparation_retains_pixels_without_ui_uploads",
            true,
        );
    }

    fn warm_folder_trial(name: &str, large: bool) {
        use std::os::windows::process::CommandExt;
        let Some(root) = crate::tests::isolated_test_root(name) else {
            return;
        };
        let source_size = if large { (4096, 2304) } else { (32, 24) };
        let source_filter = format!("testsrc2=size={}x{}:rate=30", source_size.0, source_size.1);
        let output = std::process::Command::new(
            PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
                .join("bin/ffmpeg.exe"),
        )
        .creation_flags(0x08000000)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            &source_filter,
            "-frames:v",
            "96",
            "-threads",
            "1",
        ])
        .arg(root.join("image-%03d.png"))
        .output()
        .expect("generate owned PNGs");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let paths: Vec<_> = (1..=96)
            .map(|index| root.join(format!("image-{index:03}.png")))
            .collect();
        let before: Vec<_> = paths
            .iter()
            .map(|path| std::fs::read(path).expect("source bytes"))
            .collect();
        let cache_path = root.join("cache");
        let cache = PreviewCache::new(cache_path.clone()).expect("owned cache");
        let (notify, ready) = std::sync::mpsc::channel();
        let mut app = crate::Application::new_with_preview_cache(
            None,
            move |event| {
                let _ = notify.send(event);
            },
            cache,
        )
        .expect("app");
        app.tabs.open_new(paths[48].clone(), MediaKind::Image);
        app.path = Some(paths[48].clone());
        app.media_kind = Some(MediaKind::Image);
        app.state = towavue_core::PlaybackState::Paused;
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: paths
                .iter()
                .enumerate()
                .map(|(index, path)| FolderMediaItem {
                    identity: ShellIdentity::new(vec![index as u8]),
                    path: path.clone(),
                    kind: MediaKind::Image,
                })
                .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: SystemTime::now(),
        });
        let context = crate::fonts::test_context();
        app.ui_context = Some(context.clone());
        let preparation_started = std::time::Instant::now();
        let mut longest_handler = Duration::ZERO;
        app.prepare_filmstrip(&context);
        let deadline =
            std::time::Instant::now() + Duration::from_secs(if large { 120 } else { 15 });
        let mut advance = |app: &mut crate::Application<_>| {
            let event = ready
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("preparation completion");
            let started = std::time::Instant::now();
            app.handle_app_event(event);
            longest_handler = longest_handler.max(started.elapsed());
        };
        while app.filmstrip.previews.len() < VISIBLE_PREVIEW_LIMIT {
            advance(&mut app);
        }
        let nearest_elapsed = preparation_started.elapsed();
        let retained: Vec<_> = app
            .filmstrip
            .previews
            .iter()
            .map(|(path, preview)| (path.clone(), preview.as_ref().expect("prepared PNG").0.id()))
            .collect();
        assert!(app.filmstrip.warming.is_some());
        let next = app.filmstrip.preparation.as_ref().expect("progress").next;
        for pointer in [false, true] {
            app.palette_open = !pointer;
            let _ = context.run_ui(
                egui::RawInput {
                    events: if pointer {
                        vec![egui::Event::PointerButton {
                            pos: egui::pos2(20.0, 20.0),
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        }]
                    } else {
                        vec![]
                    },
                    ..Default::default()
                },
                |_| {},
            );
            app.prepare_filmstrip(&context);
            assert!(app.filmstrip.warming.is_none());
            app.filmstrip.finish(&context);
            assert_eq!(
                app.filmstrip
                    .preparation
                    .as_ref()
                    .expect("paused progress")
                    .next,
                next
            );
            app.palette_open = false;
            let _ = context.run_ui(
                egui::RawInput {
                    events: vec![egui::Event::PointerButton {
                        pos: egui::pos2(20.0, 20.0),
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..Default::default()
                },
                |_| {},
            );
            app.prepare_filmstrip(&context);
        }
        let _ = context.tex_manager().write().take_delta();
        while app.filmstrip.preparation.as_ref().expect("progress").next < paths.len()
            || app.filmstrip.warming.is_some()
        {
            advance(&mut app);
        }
        let preparation_elapsed = preparation_started.elapsed();
        assert_eq!(app.filmstrip.previews.len(), VISIBLE_PREVIEW_LIMIT);
        let delta = context.tex_manager().write().take_delta();
        assert!(
            delta.set.is_empty() && delta.free.is_empty(),
            "warming neither uploads nor frees UI textures"
        );
        for (path, id) in retained {
            assert_eq!(
                app.filmstrip.previews[&path]
                    .as_ref()
                    .expect("retained PNG")
                    .0
                    .id(),
                id
            );
        }
        let generation = app.filmstrip.generation;
        app.prepare_filmstrip(&context);
        assert_eq!(
            app.filmstrip.generation, generation,
            "idle completion must not start another pass"
        );
        // A fresh cache cannot borrow memory: every source must now have a reusable
        // disk thumbnail, not just the last 64 memory entries or UI textures.
        let fresh = PreviewCache::new(cache_path).expect("fresh cache");
        for (path, original) in paths.iter().zip(before) {
            let preview = fresh
                .cached_image(path)
                .expect("cached lookup")
                .expect("durable preview");
            assert_eq!(preview.source_size, source_size);
            assert!(preview.image.width <= 240 && preview.image.height <= 160);
            assert_eq!(std::fs::read(path).expect("unchanged source"), original);
        }
        // The actual foreground entry must cancel preparation before any redraw,
        // not merely rely on a future frame noticing image_loading.
        app.request_image_paths(vec![paths[48].clone()], 0);
        assert!(app.filmstrip.warming.is_none() && app.filmstrip.preparation.is_none());
        let foreground_started = std::time::Instant::now();
        let original = loop {
            if let Some(result) = app.image_loader.take_completed() {
                break result;
            }
            ready
                .recv_timeout(Duration::from_secs(15))
                .expect("foreground completion");
        };
        assert_eq!(original.generation, app.image_generation);
        assert_eq!(original.images.len(), 1);
        assert_eq!(original.images[0].0, paths[48]);
        let original = original.images[0].1.as_ref().expect("full original");
        assert_eq!(
            (original.frames[0].width, original.frames[0].height),
            source_size
        );
        if large {
            eprintln!(
                "PREVIEW_WARM files=96 source={}x{} nearest_64_ms={:.3} all_96_ms={:.3} max_completion_handler_ms={:.3} foreground_decode_ms={:.3}; warm source files, empty thumbnail cache, pause/resume included; 64 retained UI textures, zero tail uploads/frees; no physical input, GPU presentation, Release-speed or peak-memory claim",
                source_size.0,
                source_size.1,
                nearest_elapsed.as_secs_f64() * 1000.0,
                preparation_elapsed.as_secs_f64() * 1000.0,
                longest_handler.as_secs_f64() * 1000.0,
                foreground_started.elapsed().as_secs_f64() * 1000.0
            );
        }
    }

    #[test]
    fn recent_grid_arrow_navigation_reveals_each_row_without_opening_files() {
        let root =
            std::env::temp_dir().join(format!("towavue-recent-keyboard-{}", std::process::id()));
        let paths: Vec<_> = (0..40).map(|i| root.join(format!("{i:02}.png"))).collect();
        for density in [1.0, 1.5, 2.0] {
            for (width, columns) in [(240.0, 1), (420.0, 2)] {
                let mut strip =
                    Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                        .expect("worker");
                let context = crate::fonts::test_context();
                context.set_pixels_per_point(density);
                context.enable_accesskit();
                let mut time = 0.0;
                let mut frame = |strip: &mut Filmstrip, events| {
                    time += 0.1;
                    let mut actions = Vec::new();
                    let mut bounds = Rect::NOTHING;
                    let mut offset = 0.0;
                    let output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 240.0),
                            )),
                            time: Some(time),
                            events,
                            ..Default::default()
                        },
                        |ui| {
                            let scroll = egui::ScrollArea::vertical().show_styled(ui, |ui| {
                                strip.show_recent(ui, &paths, true, &mut actions)
                            });
                            bounds = scroll.inner_rect;
                            offset = scroll.state.offset.y;
                        },
                    );
                    assert!(actions.is_empty(), "navigation must not open a file");
                    assert_eq!(strip.visible.len(), 40);
                    (
                        output.platform_output.accesskit_update.expect("tree"),
                        bounds,
                        offset,
                    )
                };
                for _ in 0..3 {
                    frame(&mut strip, vec![]);
                }
                let (tree, _, _) = frame(&mut strip, vec![]);
                let first = tree
                    .nodes
                    .iter()
                    .find(|(_, n)| {
                        n.label() == Some("00.png") && n.role() == egui::accesskit::Role::Button
                    })
                    .expect("first card")
                    .0;
                frame(
                    &mut strip,
                    vec![egui::Event::AccessKitActionRequest(
                        egui::accesskit::ActionRequest {
                            action: egui::accesskit::Action::Focus,
                            target_tree: egui::accesskit::TreeId::ROOT,
                            target_node: first,
                            data: None,
                        },
                    )],
                );
                for (key, rows) in [
                    (egui::Key::ArrowDown, (1..40 / columns).collect::<Vec<_>>()),
                    (egui::Key::ArrowUp, (0..40 / columns - 1).rev().collect()),
                ] {
                    for row in rows {
                        for pressed in [true, false] {
                            frame(
                                &mut strip,
                                vec![egui::Event::Key {
                                    key,
                                    physical_key: None,
                                    pressed,
                                    repeat: false,
                                    modifiers: egui::Modifiers::NONE,
                                }],
                            );
                        }
                        for _ in 0..5 {
                            frame(&mut strip, vec![]);
                        }
                        let (tree, viewport, _) = frame(&mut strip, vec![]);
                        let node = &tree
                            .nodes
                            .iter()
                            .find(|(id, _)| *id == tree.focus)
                            .expect("focused card")
                            .1;
                        assert_eq!(
                            node.label(),
                            Some(format!("{:02}.png", row * columns).as_str()),
                            "width {width}, density {density}, {key:?}"
                        );
                        let bounds = node.bounds().expect("card bounds");
                        let tolerance = f64::from(1.0 / density);
                        assert!(
                            bounds.y0 >= f64::from(viewport.top()) - tolerance
                                && bounds.y1 <= f64::from(viewport.bottom()) + tolerance,
                            "width {width}, density {density}, row {row}: {bounds:?} vs {viewport:?}"
                        );
                    }
                }
                frame(
                    &mut strip,
                    vec![
                        egui::Event::PointerMoved(egui::pos2(20.0, 20.0)),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -60.0),
                            phase: egui::TouchPhase::Move,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
                for _ in 0..8 {
                    frame(&mut strip, vec![]);
                }
                assert!(
                    frame(&mut strip, vec![]).2 > 40.0,
                    "manual scrolling survives unchanged focus"
                );
            }
        }
    }

    #[test]
    fn recent_grid_prepares_all_recent_previews_and_blocks_background_actions() {
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
                    crate::welcome::show(
                        ui,
                        &towavue_core::ShortcutBindings::default(),
                        &mut String::new(),
                        &mut None,
                        &paths,
                        enabled,
                        |ui, _, _| {
                            filmstrip.show_recent(ui, &paths, enabled, &mut actions);
                            Vec::new()
                        },
                    );
                },
            );
            (output, actions)
        };
        for _ in 0..3 {
            frame(&mut filmstrip, true, vec![]);
        }
        assert_eq!(filmstrip.visible, paths);
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
        let bounds = node.bounds().expect("card bounds");
        let position = egui::pos2((bounds.x0 + 20.0) as f32, (bounds.y0 + 20.0) as f32);
        for (step, enabled) in [true, false, true].into_iter().enumerate() {
            if step == 2 {
                // egui hit testing uses the previous frame's disabled widget list.
                let (output, actions) = frame(&mut filmstrip, true, vec![]);
                assert!(actions.is_empty());
                assert_eq!(
                    output.platform_output.cursor_icon,
                    egui::CursorIcon::Default
                );
            }
            let (output, actions) = frame(
                &mut filmstrip,
                enabled,
                vec![egui::Event::PointerMoved(position)],
            );
            assert!(actions.is_empty());
            let mesh = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.texture_id == texture_id => Some(mesh),
                    _ => None,
                })
                .expect("covered Gallery retains ready thumbnails");
            assert!(
                mesh.vertices
                    .iter()
                    .all(|vertex| vertex.color == Color32::WHITE),
                "overlay input guards must not dim the Gallery"
            );
            assert_eq!(
                output.platform_output.cursor_icon,
                if enabled {
                    egui::CursorIcon::PointingHand
                } else {
                    egui::CursorIcon::Default
                },
                "recent card step={step}, enabled={enabled}, position={position:?}"
            );
        }
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
        assert_eq!(filmstrip.visible, paths);
        assert!(filmstrip.previews.contains_key(&paths[0]));
        drop(filmstrip);
        std::fs::remove_dir(root).expect("remove empty owned cache");
    }

    #[test]
    fn gallery_overlays_keep_ready_thumbnail_geometry_color_and_requests() {
        let Some(root) = crate::tests::isolated_test_root(
            "filmstrip::tests::gallery_overlays_keep_ready_thumbnail_geometry_color_and_requests",
        ) else {
            return;
        };
        for density in [1.0, 1.25, 2.0] {
            let mut app = crate::Application::new(None, |_| {}).expect("app");
            let context = crate::fonts::test_context();
            context.enable_accesskit();
            context.set_pixels_per_point(density);
            context.global_style_mut(crate::chrome::style);
            app.ui_context = Some(context.clone());
            let path = root.join("gallery-overlay.png");
            app.recent_paths.push(path.clone());
            let size = egui::vec2(640.0, 480.0);
            for _ in 0..3 {
                crate::audio_export::tests::frame(&mut app, size, vec![]);
            }
            let texture = context.load_texture(
                "gallery-overlay-fixture",
                egui::ColorImage::filled([2, 1], Color32::RED),
                egui::TextureOptions::LINEAR,
            );
            let texture_id = texture.id();
            app.filmstrip
                .previews
                .insert(path.clone(), Ok((texture, None)));
            let generation = app.filmstrip.generation;
            let mut bounds = None;
            for overlay in 0..4 {
                app.palette_open = overlay == 1;
                app.grid_open = overlay == 2;
                for _ in 0..3 {
                    if overlay == 3 {
                        // No real menu widget keeps this synthetic popup open between frames.
                        egui::Popup::open_id(&context, egui::Id::new("gallery-overlay-menu"));
                    }
                    let output = crate::audio_export::tests::frame(&mut app, size, vec![]);
                    let mesh = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Mesh(mesh) if mesh.texture_id == texture_id => Some(mesh),
                            _ => None,
                        })
                        .expect("covered Gallery still paints the prepared texture");
                    assert!(
                        mesh.vertices
                            .iter()
                            .all(|vertex| vertex.color == Color32::WHITE)
                    );
                    assert_eq!(
                        *bounds.get_or_insert(mesh.calc_bounds()),
                        mesh.calc_bounds()
                    );
                    assert_eq!(app.filmstrip.generation, generation, "no request restart");
                    assert_eq!(app.filmstrip.visible, std::slice::from_ref(&path));
                    let tree = output.platform_output.accesskit_update.expect("tree");
                    let card = tree
                        .nodes
                        .iter()
                        .find(|(_, node)| {
                            node.role() == egui::accesskit::Role::Button
                                && node.label() == Some("gallery-overlay.png")
                        })
                        .expect("card remains in accessibility tree");
                    assert_eq!(card.1.is_disabled(), overlay == 2, "overlay {overlay}");
                    assert!(app.tabs.tabs().is_empty() && app.path.is_none());
                }
                app.palette_open = false;
                app.grid_open = false;
                egui::Popup::close_all(&context);
            }
        }
    }

    #[test]
    fn recent_grid_preparation_is_bounded_unique_and_visible_first() {
        let root =
            std::env::temp_dir().join(format!("towavue-recent-limit-{}", std::process::id()));
        let mut strip =
            Filmstrip::new(PreviewCache::new(root.clone()).expect("cache"), || {}).expect("worker");
        let mut paths: Vec<_> = (0..100)
            .map(|index| root.join(format!("{index:03}.png")))
            .collect();
        paths.insert(0, root.join("unsupported.txt"));
        paths.push(paths[0].clone());
        paths.extend_from_within(1..3);
        let context = crate::fonts::test_context();
        let mut offset = 0.0;
        for _ in 0..3 {
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(660.0, 260.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    let scroll = egui::ScrollArea::vertical()
                        .vertical_scroll_offset(offset)
                        .show_styled(ui, |ui| strip.show_recent(ui, &paths, true, &mut vec![]));
                    offset = (scroll.content_size.y - scroll.inner_rect.height()).max(0.0);
                },
            );
        }
        assert_eq!(strip.visible.len(), VISIBLE_PREVIEW_LIMIT);
        let unique: std::collections::HashSet<_> = strip.visible.iter().collect();
        assert_eq!(unique.len(), strip.visible.len());
        assert!(
            !strip.visible.contains(&paths[0]),
            "unsupported entries are skipped"
        );
        assert!(
            paths[90..101].contains(&strip.visible[0]),
            "visible bottom rows precede offscreen preparation: {:?}",
            strip.visible[0]
        );
        assert!(
            strip.visible.contains(&paths[3]),
            "prepare earlier offscreen entries too"
        );
        drop(strip);
        std::fs::remove_dir(root).expect("remove empty owned cache");
    }

    #[test]
    fn large_recent_grid_virtualizes_top_middle_and_last_rows() {
        let root =
            std::env::temp_dir().join(format!("towavue-recent-archive-{}", std::process::id()));
        let paths: Vec<_> = (0..10_000)
            .map(|index| root.join(format!("{index:05}.png")))
            .collect();
        let mut strip =
            Filmstrip::new(PreviewCache::new(root.clone()).expect("cache"), || {}).expect("worker");
        for density in [1.0, 1.5, 2.0] {
            for width in [240.0, 660.0] {
                let context = crate::fonts::test_context();
                context.enable_accesskit();
                let mut max_offset = 0.0;
                for fraction in [0.0, 0.5, 1.0] {
                    let mut first = None;
                    let mut last = None;
                    for _ in 0..3 {
                        let mut grid = None;
                        let output = context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(width, 400.0),
                                )),
                                // Supply native DPI from the first frame. Changing egui
                                // zoom before any frame rescales its default 10k viewport
                                // and deliberately replaces this frame's screen rectangle.
                                viewports: [(
                                    egui::ViewportId::ROOT,
                                    egui::ViewportInfo {
                                        native_pixels_per_point: Some(density),
                                        ..Default::default()
                                    },
                                )]
                                .into_iter()
                                .collect(),
                                ..Default::default()
                            },
                            |ui| {
                                let scroll = egui::ScrollArea::vertical()
                                    .vertical_scroll_offset(max_offset * fraction)
                                    .show_styled(ui, |ui| {
                                        grid =
                                            Some(strip.show_recent(ui, &paths, true, &mut vec![]));
                                    });
                                max_offset =
                                    (scroll.content_size.y - scroll.inner_rect.height()).max(0.0);
                            },
                        );
                        let grid = grid.expect("grid");
                        assert_eq!(grid.offset(0), 0.0);
                        assert!(
                            grid.offset(9999) > max_offset - 400.0,
                            "last row matches the scroll extent at {width}/{density}: {} vs {max_offset}",
                            grid.offset(9999)
                        );
                        let tree = output.platform_output.accesskit_update.expect("tree");
                        let mut cards: Vec<_> = tree
                            .nodes
                            .iter()
                            .filter_map(|(_, node)| {
                                (node.role() == egui::accesskit::Role::Button)
                                    .then(|| node.label())
                                    .flatten()
                            })
                            .collect();
                        cards.sort_unstable();
                        assert!(
                            (1..=32).contains(&cards.len()),
                            "widget bound at {width}/{density}/{fraction}: {}",
                            cards.len()
                        );
                        first = cards.first().map(|name| name.to_string());
                        last = cards.last().map(|name| name.to_string());
                        assert_eq!(strip.visible.len(), VISIBLE_PREVIEW_LIMIT);
                    }
                    if fraction == 0.0 {
                        assert_eq!(first.as_deref(), Some("00000.png"));
                    } else if fraction == 1.0 {
                        assert_eq!(last.as_deref(), Some("09999.png"));
                        assert!(
                            strip.visible[0]
                                .file_name()
                                .expect("name")
                                .to_string_lossy()
                                .starts_with("099")
                        );
                    } else {
                        let first = first.expect("middle row");
                        let index: usize = first.trim_end_matches(".png").parse().expect("index");
                        assert!(
                            (4900..5100).contains(&index),
                            "middle at {width}/{density}: {first}"
                        );
                    }
                }
            }
        }
        drop(strip);
        std::fs::remove_dir(root).expect("remove empty owned cache");
    }

    fn bitmap_paths(root: &Path, count: usize) -> Vec<PathBuf> {
        let paths: Vec<_> = (0..count)
            .map(|index| root.join(format!("{index:02}.bmp")))
            .collect();
        let mut bitmap = vec![0_u8; 62];
        bitmap[..2].copy_from_slice(b"BM");
        bitmap[2..6].copy_from_slice(&62_u32.to_le_bytes());
        bitmap[10..14].copy_from_slice(&54_u32.to_le_bytes());
        bitmap[14..18].copy_from_slice(&40_u32.to_le_bytes());
        bitmap[18..22].copy_from_slice(&2_u32.to_le_bytes());
        bitmap[22..26].copy_from_slice(&1_u32.to_le_bytes());
        bitmap[26..28].copy_from_slice(&1_u16.to_le_bytes());
        bitmap[28..30].copy_from_slice(&24_u16.to_le_bytes());
        bitmap[34..38].copy_from_slice(&8_u32.to_le_bytes());
        for (index, path) in paths.iter().enumerate() {
            bitmap[54..].copy_from_slice(&[12, 34, index as u8, 12, 34, index as u8, 0, 0]);
            std::fs::write(path, &bitmap).expect("owned bitmap");
        }
        paths
    }

    #[test]
    fn recent_grid_has_offscreen_textures_ready_before_scrolling() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "towavue-recent-preparation-{}-{unique}",
            std::process::id()
        ));
        let cache = PreviewCache::new(root.join("cache")).expect("owned cache");
        let paths = bitmap_paths(&root, 40);
        let (notify, ready) = std::sync::mpsc::channel();
        let mut strip = Filmstrip::new(cache, move || {
            let _ = notify.send(());
        })
        .expect("worker");
        let context = crate::fonts::test_context();
        let frame = |strip: &mut Filmstrip, offset| {
            let mut max_scroll = 0.0;
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(660.0, 260.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    let scroll = egui::ScrollArea::vertical()
                        .vertical_scroll_offset(offset)
                        .show_styled(ui, |ui| strip.show_recent(ui, &paths, true, &mut vec![]));
                    max_scroll = (scroll.content_size.y - scroll.inner_rect.height()).max(0.0);
                },
            );
            (output, max_scroll)
        };
        for _ in 0..3 {
            frame(&mut strip, 0.0);
        }
        assert_eq!(strip.visible.len(), paths.len(), "prepare offscreen rows");
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while strip.previews.len() != paths.len() {
            ready
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("all recent previews complete without scrolling");
            strip.finish(&context);
        }
        for path in &paths {
            assert!(strip.previews[path].is_ok(), "{}", path.display());
        }
        let texture = &strip.previews[paths.last().expect("last path")]
            .as_ref()
            .expect("offscreen texture")
            .0;
        let texture_id = texture.id();
        let contains_texture = |output: &egui::FullOutput| {
            output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == texture_id)
            })
        };
        let (before, max_scroll) = frame(&mut strip, 0.0);
        assert!(!contains_texture(&before));
        assert!(contains_texture(&frame(&mut strip, max_scroll).0));
        assert_eq!(
            strip.previews.len(),
            paths.len(),
            "retain prepared textures"
        );
        drop(strip);
        std::fs::remove_dir_all(root).expect("remove owned fixtures and cache");
    }

    #[test]
    fn preview_completions_redraw_only_displayed_consumers() {
        use std::sync::Arc;
        use winit::application::ApplicationHandler;
        use winit::event::WindowEvent;
        use winit::event_loop::{ActiveEventLoop, EventLoop};
        use winit::platform::pump_events::EventLoopExtPumpEvents;
        use winit::platform::windows::EventLoopBuilderExtWindows;
        use winit::window::{Window, WindowId};

        let Some(root) = crate::tests::isolated_test_root(
            "filmstrip::tests::preview_completions_redraw_only_displayed_consumers",
        ) else {
            return;
        };
        #[derive(Default)]
        struct Redraws {
            window: Option<Arc<Window>>,
            count: usize,
        }
        impl ApplicationHandler for Redraws {
            fn resumed(&mut self, event_loop: &ActiveEventLoop) {
                if self.window.is_none() {
                    self.window = Some(Arc::new(
                        event_loop
                            .create_window(
                                Window::default_attributes()
                                    .with_active(false)
                                    .with_inner_size(winit::dpi::PhysicalSize::new(64, 64))
                                    .with_position(winit::dpi::PhysicalPosition::new(
                                        -32000, -32000,
                                    )),
                            )
                            .expect("offscreen redraw control"),
                    ));
                }
            }

            fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
                self.count += usize::from(matches!(event, WindowEvent::RedrawRequested));
            }
        }
        let mut event_loop = EventLoop::builder()
            .with_any_thread(true)
            .build()
            .expect("isolated event loop");
        let mut redraws = Redraws::default();
        event_loop.pump_app_events(Some(Duration::ZERO), &mut redraws);
        let window = redraws.window.as_ref().expect("window").clone();
        let mut pump = |redraws: &mut Redraws| {
            for _ in 0..3 {
                event_loop.pump_app_events(Some(Duration::from_millis(20)), redraws);
            }
            std::mem::take(&mut redraws.count)
        };
        pump(&mut redraws);
        window.request_redraw();
        assert!(pump(&mut redraws) > 0, "native redraw delivery control");

        let (notify, ready) = std::sync::mpsc::channel();
        let mut app = crate::Application::new(None, move |event| {
            let _ = notify.send(event);
        })
        .expect("app");
        let path = bitmap_paths(&root, 1).remove(0);
        let context = crate::fonts::test_context();
        app.ui_context = Some(context.clone());
        app.window = Some(window);
        app.state = towavue_core::PlaybackState::Paused;
        app.media_kind = Some(MediaKind::Image);
        for missing in [false, true] {
            let requested = if missing {
                root.join("missing.bmp")
            } else {
                path.clone()
            };
            app.folder_snapshot = Some(FolderSnapshot {
                folder_identity: ShellIdentity::new(vec![]),
                folder_path: root.clone(),
                items: vec![FolderMediaItem {
                    identity: ShellIdentity::new(vec![0]),
                    path: requested.clone(),
                    kind: MediaKind::Image,
                }],
                sort_columns: vec![],
                source: FolderSnapshotSource::LiveExplorerView,
                generation: 1,
                captured_at: SystemTime::now(),
            });
            for consumer in 0..4 {
                app.path = (consumer != 3).then(|| requested.clone());
                app.filmstrip_open = consumer == 1;
                app.image_seek_preview_active = consumer == 2;
                app.filmstrip.clear();
                for _ in ready.try_iter() {}
                app.filmstrip
                    .set_visible(vec![(requested.clone(), MediaKind::Image)]);
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                while !app.filmstrip.previews.contains_key(&requested) {
                    let event = ready
                        .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                        .expect("real preview completion");
                    app.handle_app_event(event);
                }
                assert_eq!(app.filmstrip.previews[&requested].is_err(), missing);
                let count = pump(&mut redraws);
                if consumer == 0 {
                    assert_eq!(count, 0, "closed preparation needs no native redraw");
                } else {
                    assert!(count > 0, "displayed consumer {consumer} updates");
                }
                app.handle_app_event(crate::AppEvent::FilmstripReady);
                assert_eq!(
                    pump(&mut redraws),
                    0,
                    "drained notification needs no redraw"
                );
            }
        }
        app.filmstrip.clear();
        app.handle_app_event(crate::AppEvent::FilmstripReady);
        assert_eq!(
            pump(&mut redraws),
            0,
            "cancelled notification needs no redraw"
        );

        // Original-image workers use the same wakeup when their final lease retires.
        // An empty completion queue must still start eligible closed preparation.
        let snapshot = app.folder_snapshot.as_mut().expect("folder");
        snapshot.items[0].path = path.clone();
        app.path = Some(path.clone());
        app.filmstrip_open = false;
        app.image_seek_preview_active = false;
        app.palette_open = true;
        app.handle_app_event(crate::AppEvent::FilmstripReady);
        assert!(app.filmstrip.visible.is_empty(), "overlay defers work");
        app.palette_open = false;
        app.handle_app_event(crate::AppEvent::FilmstripReady);
        assert_eq!(app.filmstrip.visible, vec![path.clone()]);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !app.filmstrip.previews.contains_key(&path) {
            let event = ready
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("idle wakeup starts preparation without a draw");
            app.handle_app_event(event);
        }
        assert!(app.filmstrip.previews[&path].is_ok());
        assert_eq!(pump(&mut redraws), 0, "idle preparation does not render");
    }

    #[test]
    fn closed_filmstrip_prepares_neighbors_and_reuses_them_on_first_open() {
        let Some(root) = crate::tests::isolated_test_root(
            "filmstrip::tests::closed_filmstrip_prepares_neighbors_and_reuses_them_on_first_open",
        ) else {
            return;
        };
        let (notify, ready) = std::sync::mpsc::channel();
        let mut app = crate::Application::new(None, move |event| {
            let _ = notify.send(event);
        })
        .expect("app");
        let paths = bitmap_paths(&root, 80);
        let current = paths[40].clone();
        app.tabs.open_new(current.clone(), MediaKind::Image);
        app.path = Some(current.clone());
        app.media_kind = Some(MediaKind::Image);
        app.state = towavue_core::PlaybackState::Paused;
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: paths
                .iter()
                .enumerate()
                .map(|(index, path)| FolderMediaItem {
                    identity: ShellIdentity::new(vec![index as u8]),
                    path: path.clone(),
                    kind: MediaKind::Image,
                })
                .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: SystemTime::now(),
        });
        let context = crate::fonts::test_context();
        app.ui_context = Some(context.clone());
        let draw = |app: &mut crate::Application<_>| {
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut vec![]),
            )
        };
        draw(&mut app);
        assert!(!app.filmstrip_open);
        assert_eq!(app.filmstrip.visible.len(), VISIBLE_PREVIEW_LIMIT);
        assert_eq!(
            &app.filmstrip.visible[..5],
            &[
                paths[40].clone(),
                paths[39].clone(),
                paths[41].clone(),
                paths[38].clone(),
                paths[42].clone()
            ]
        );
        let generation = app.filmstrip.generation;
        draw(&mut app);
        assert_eq!(
            app.filmstrip.generation, generation,
            "idle draws do not restart preparation"
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while app.filmstrip.previews.len() != VISIBLE_PREVIEW_LIMIT {
            ready
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("previews finish without opening the strip");
            app.filmstrip.finish(&context);
        }
        let texture_id = app.filmstrip.previews[&current]
            .as_ref()
            .expect("prepared current")
            .0
            .id();
        // Publish the initial allocations before measuring redundant uploads.
        draw(&mut app);
        let prepared: Vec<_> = app
            .filmstrip
            .previews
            .iter()
            .map(|(path, preview)| {
                (
                    path.clone(),
                    preview.as_ref().expect("prepared bitmap").0.id(),
                )
            })
            .collect();
        for count in [1, 3, 9] {
            app.filmstrip.set_visible(
                paths[40..40 + count]
                    .iter()
                    .map(|path| (path.clone(), MediaKind::Image))
                    .collect(),
            );
            assert_eq!(
                app.filmstrip.previews.len(),
                prepared.len(),
                "a smaller strip or seek preview retains bounded prepared neighbors"
            );
            let resumed = draw(&mut app);
            for (path, id) in &prepared {
                assert_eq!(
                    app.filmstrip.previews[path]
                        .as_ref()
                        .expect("retained preview")
                        .0
                        .id(),
                    *id
                );
                assert!(
                    !resumed
                        .textures_delta
                        .set
                        .iter()
                        .any(|(uploaded, _)| uploaded == id),
                    "returning to idle preparation does not reupload neighbors"
                );
            }
        }
        for blocked in 0..5 {
            app.palette_open = blocked == 0;
            app.grid_open = blocked == 1;
            app.pending_guard = (blocked == 2).then_some(crate::GuardedAction::Exit);
            if blocked == 3 {
                egui::Popup::open_id(&context, "preparation-test-menu".into());
            }
            if blocked == 4 {
                let _ = context.run_ui(
                    egui::RawInput {
                        hovered_files: vec![egui::HoveredFile::default()],
                        ..Default::default()
                    },
                    |_| {},
                );
            }
            app.prepare_filmstrip(&context);
            assert!(app.filmstrip.visible.is_empty(), "overlay pauses new work");
            assert_eq!(
                app.filmstrip.previews.len(),
                prepared.len(),
                "overlay retains prepared textures"
            );
            let paused = app.filmstrip.generation;
            app.prepare_filmstrip(&context);
            assert_eq!(
                app.filmstrip.generation, paused,
                "idle overlay does not cancel repeatedly"
            );
            app.palette_open = false;
            app.grid_open = false;
            app.pending_guard = None;
            egui::Popup::close_all(&context);
            let resumed = draw(&mut app);
            assert_eq!(app.filmstrip.visible.len(), VISIBLE_PREVIEW_LIMIT);
            for (path, id) in &prepared {
                assert_eq!(
                    app.filmstrip.previews[path]
                        .as_ref()
                        .expect("retained preview")
                        .0
                        .id(),
                    *id
                );
                assert!(
                    !resumed
                        .textures_delta
                        .set
                        .iter()
                        .any(|(uploaded, _)| uploaded == id),
                    "resuming does not upload retained pixels"
                );
            }
        }
        app.filmstrip.set_visible(
            paths[..VISIBLE_PREVIEW_LIMIT]
                .iter()
                .map(|path| (path.clone(), MediaKind::Image))
                .collect(),
        );
        assert_eq!(app.filmstrip.preview_order, paths[..VISIBLE_PREVIEW_LIMIT]);
        assert!(
            app.filmstrip
                .previews
                .keys()
                .all(|path| { paths[..VISIBLE_PREVIEW_LIMIT].contains(path) }),
            "new visible requests reserve slots before retained neighbors"
        );
        for return_to_neighbors in [false, true] {
            if return_to_neighbors {
                draw(&mut app);
            }
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while app.filmstrip.previews.len() != VISIBLE_PREVIEW_LIMIT {
                ready
                    .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                    .expect("new working set finishes");
                app.filmstrip.finish(&context);
                assert!(app.filmstrip.previews.len() <= VISIBLE_PREVIEW_LIMIT);
            }
            assert_eq!(
                app.filmstrip.previews[&current]
                    .as_ref()
                    .expect("retained current")
                    .0
                    .id(),
                texture_id
            );
        }
        app.pending_folder = Some((1, crate::FolderIntent::Refresh(current.clone())));
        app.prepare_filmstrip(&context);
        assert!(
            app.filmstrip.visible.is_empty(),
            "refresh pauses background work"
        );
        assert_eq!(app.filmstrip.previews.len(), VISIBLE_PREVIEW_LIMIT);
        let paused_generation = app.filmstrip.generation;
        app.prepare_filmstrip(&context);
        assert_eq!(
            app.filmstrip.generation, paused_generation,
            "no cancellation churn"
        );
        app.pending_folder = None;
        app.apply_folder_snapshot(app.folder_snapshot.clone().expect("same listing"));
        assert_eq!(
            app.filmstrip.previews.len(),
            VISIBLE_PREVIEW_LIMIT,
            "a refresh completing while closed must retain ready previews"
        );
        assert_eq!(
            app.filmstrip.refreshing.len(),
            VISIBLE_PREVIEW_LIMIT,
            "retained previews still require source validation"
        );
        app.filmstrip_open = true;
        let output = draw(&mut app);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Mesh(mesh) if mesh.texture_id == texture_id)));
        assert!(!app.filmstrip.refreshing.is_empty());
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !app.filmstrip.refreshing.is_empty() {
            ready
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("deferred source validation resumes on opening");
            app.filmstrip.finish(&context);
        }
        let mut retained: Vec<_> = app
            .filmstrip
            .visible
            .iter()
            .map(|path| {
                (
                    path.clone(),
                    app.filmstrip.previews[path]
                        .as_ref()
                        .expect("prepared")
                        .0
                        .id(),
                )
            })
            .collect();
        let removed = retained
            .iter()
            .find(|(path, _)| path != &current)
            .expect("neighbor")
            .0
            .clone();
        let mut bitmap = std::fs::read(&current).expect("owned bitmap");
        bitmap[56] = 255;
        bitmap[59] = 255;
        let modified = std::fs::metadata(&current)
            .expect("metadata")
            .modified()
            .expect("mtime");
        std::fs::write(&current, bitmap).expect("change owned pixels");
        std::fs::File::options()
            .write(true)
            .open(&current)
            .expect("owned file")
            .set_times(std::fs::FileTimes::new().set_modified(modified + Duration::from_secs(2)))
            .expect("distinct source stamp");
        std::fs::remove_file(&removed).expect("remove owned neighbor");
        let snapshot = app.folder_snapshot.clone().expect("snapshot");
        app.apply_folder_snapshot(snapshot);
        for (path, id) in &retained {
            assert_eq!(
                app.filmstrip.previews[path]
                    .as_ref()
                    .expect("held preview")
                    .0
                    .id(),
                *id
            );
        }
        // Pausing and then changing the viewport must resume source revalidation.
        app.filmstrip.pause_preparation();
        assert!(app.filmstrip.visible.is_empty());
        assert!(!app.filmstrip.refreshing.is_empty());
        let (offscreen, _) = retained.pop().expect("last visible neighbor");
        app.filmstrip.set_visible(
            retained
                .iter()
                .map(|(path, _)| (path.clone(), MediaKind::Image))
                .collect(),
        );
        assert!(!app.filmstrip.previews.contains_key(&offscreen));
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while retained.iter().any(|(path, id)| {
            app.filmstrip.previews[path]
                .as_ref()
                .is_ok_and(|preview| preview.0.id() == *id)
        }) {
            ready
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("refreshed previews finish");
            app.filmstrip.finish(&context);
            assert_eq!(
                app.filmstrip.previews.len(),
                retained.len(),
                "no empty slots while refreshing"
            );
        }
        assert!(
            app.filmstrip.previews[&removed].is_err(),
            "missing source replaces stale preview"
        );
        assert!(app.filmstrip.refreshing.is_empty());
        let replacement = app.filmstrip.previews[&current]
            .as_ref()
            .expect("changed source")
            .0
            .id();
        let refreshed = draw(&mut app);
        let upload = &refreshed
            .textures_delta
            .set
            .iter()
            .find(|(id, _)| *id == replacement)
            .expect("replacement upload")
            .1;
        let egui::ImageData::Color(pixels) = &upload.image;
        assert!(
            pixels
                .pixels
                .iter()
                .all(|pixel| *pixel == Color32::from_rgb(255, 34, 12)),
            "fresh source pixels replace the held texture"
        );
        assert!(refreshed.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Mesh(mesh) if mesh.texture_id == replacement)));
        let mut reordered = app.folder_snapshot.clone().expect("snapshot");
        reordered.items.reverse();
        app.filmstrip_open = false;
        app.filmstrip.pause_preparation();
        app.apply_folder_snapshot(reordered);
        assert!(
            app.filmstrip.previews.is_empty(),
            "changed order resets a closed strip"
        );
        assert!(
            context.tex_manager().read().meta(replacement).is_none(),
            "clear releases paused textures"
        );
        app.filmstrip_open = false;
        for blocked in 0..7 {
            app.image_loading = blocked == 0;
            app.image_edit_pending = blocked == 1;
            app.palette_open = blocked == 2;
            app.grid_open = blocked == 3;
            app.state = if blocked == 4 {
                towavue_core::PlaybackState::Playing
            } else {
                towavue_core::PlaybackState::Paused
            };
            app.pending_folder = (blocked == 5).then_some((1, crate::FolderIntent::Open));
            if blocked == 6 {
                app.image_sequence.steps.push_back(true);
            }
            app.prepare_filmstrip(&context);
            assert!(app.filmstrip.visible.is_empty(), "blocked case {blocked}");
            assert!(
                app.filmstrip.previews.is_empty(),
                "release speculative textures"
            );
            app.image_sequence.steps.clear();
        }
        app.pending_folder = None;
        app.prepare_filmstrip(&context);
        assert_eq!(app.filmstrip.visible.len(), VISIBLE_PREVIEW_LIMIT);
        app.request_image_paths(vec![paths[0].clone()], 0);
        assert!(
            app.filmstrip.visible.is_empty(),
            "foreground request cancels immediately"
        );
    }

    #[test]
    fn keyboard_navigation_reveals_cards_beyond_the_initial_viewport() {
        let root = std::env::temp_dir().join(format!(
            "towavue-filmstrip-keyboard-scroll-{}",
            std::process::id()
        ));
        let snapshot = FolderSnapshot {
            folder_identity: ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: (0..100)
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
        for density in [1.0, 1.5, 2.0] {
            let mut strip =
                Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                    .expect("worker");
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            context.enable_accesskit();
            let screen = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 240.0));
            let mut time = 0.0;
            let mut frame = |strip: &mut Filmstrip, events| {
                time += 0.1;
                let mut actions = Vec::new();
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        time: Some(time),
                        events,
                        ..Default::default()
                    },
                    |_| {
                        strip.show(
                            &context,
                            screen,
                            Some(&snapshot),
                            Some(&snapshot.items[0].path),
                            true,
                            &mut actions,
                        )
                    },
                );
                assert!(
                    actions.is_empty(),
                    "focus navigation must not open or close media"
                );
                assert!(strip.visible.len() <= 5, "retain bounded virtualization");
                assert_eq!(
                    strip.card_paths.len(),
                    strip.visible.len(),
                    "focus lookup tracks only the materialized cards"
                );
                output.platform_output.accesskit_update.expect("tree")
            };
            strip.focus_current();
            for _ in 0..3 {
                frame(&mut strip, vec![]);
            }
            for (key, shift, indices) in [
                (egui::Key::ArrowRight, false, (1..100).collect::<Vec<_>>()),
                (egui::Key::ArrowLeft, false, (0..99).rev().collect()),
                (egui::Key::Tab, false, (1..100).chain([0]).collect()),
                (egui::Key::Tab, true, (0..100).rev().collect()),
            ] {
                for index in indices {
                    for pressed in [true, false] {
                        frame(
                            &mut strip,
                            vec![egui::Event::Key {
                                key,
                                physical_key: None,
                                pressed,
                                repeat: false,
                                modifiers: egui::Modifiers {
                                    shift,
                                    ..Default::default()
                                },
                            }],
                        );
                    }
                    for _ in 0..if key == egui::Key::Tab { 0 } else { 5 } {
                        frame(&mut strip, vec![]);
                    }
                    let tree = frame(&mut strip, vec![]);
                    let node = &tree
                        .nodes
                        .iter()
                        .find(|(id, _)| *id == tree.focus)
                        .expect("focused card")
                        .1;
                    assert_eq!(
                        node.label(),
                        Some(format!("{index}.png").as_str()),
                        "density {density}, {key:?}"
                    );
                    let bounds = node.bounds().expect("card bounds");
                    assert!(
                        bounds.x0 >= 8.0 && bounds.x1 <= 312.0,
                        "density {density}, {index}: {bounds:?}"
                    );
                }
            }
            frame(
                &mut strip,
                vec![
                    egui::Event::PointerMoved(egui::pos2(160.0, 30.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(-60.0, 0.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            for _ in 0..8 {
                frame(&mut strip, vec![]);
            }
            assert!(
                strip.scroll_offset > 40.0,
                "unchanged focus must not undo manual scrolling"
            );
        }
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
    fn highlighted_title_is_centered_above_the_thumbnail_and_wraps_to_two_rows() {
        let root =
            std::env::temp_dir().join(format!("towavue-filmstrip-title-{}", std::process::id()));
        let names = [
            "short.png",
            "A long image filename that should wrap across two lines without moving the thumbnail.png",
        ];
        let snapshot = FolderSnapshot {
            folder_identity: ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: names
                .iter()
                .enumerate()
                .map(|(index, name)| FolderMediaItem {
                    identity: ShellIdentity::new(vec![index as u8]),
                    path: root.join(name),
                    kind: MediaKind::Image,
                })
                .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: SystemTime::now(),
        };
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            context.enable_accesskit();
            context.global_style_mut(crate::chrome::style);
            let mut strip =
                Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                    .expect("worker");
            for size in [egui::vec2(320.0, 240.0), egui::vec2(960.0, 576.0)] {
                let mut thumbnail_y = None;
                for (index, name) in names.iter().enumerate() {
                    let mut output = egui::FullOutput::default();
                    for _ in 0..3 {
                        output = context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
                                ..Default::default()
                            },
                            |_| {
                                let mut actions = Vec::new();
                                strip.show(
                                    &context,
                                    context.content_rect(),
                                    Some(&snapshot),
                                    Some(&snapshot.items[index].path),
                                    true,
                                    &mut actions,
                                );
                                assert!(actions.is_empty());
                            },
                        );
                    }
                    let tree = output
                        .platform_output
                        .accesskit_update
                        .as_ref()
                        .expect("tree");
                    let bounds = tree
                        .nodes
                        .iter()
                        .find(|(_, node)| node.label() == Some(*name))
                        .expect("thumbnail")
                        .1
                        .bounds()
                        .expect("bounds");
                    let titles: Vec<_> = output
                        .shapes
                        .iter()
                        .filter_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if names.contains(&text.galley.text()) => {
                                Some((shape.clip_rect, text))
                            }
                            _ => None,
                        })
                        .collect();
                    assert_eq!(titles.len(), 1, "only the highlighted item has a title");
                    let (clip, text) = titles[0];
                    assert_eq!(text.galley.text(), *name);
                    assert_eq!(text.galley.rows.len(), index + 1);
                    let title = text.galley.rect.translate(text.pos.to_vec2());
                    assert!(
                        (title.center().x - ((bounds.x0 + bounds.x1) * 0.5) as f32).abs() < 1.0
                    );
                    assert!(
                        (title.bottom() - (bounds.y0 as f32 - 10.0)).abs() < 1.0,
                        "title above thumbnail: {title:?}, {bounds:?}"
                    );
                    assert!(
                        clip.contains_rect(title),
                        "title remains visible: {clip:?}, {title:?}"
                    );
                    assert!(title.width() <= 192.0);
                    if let Some(y) = thumbnail_y {
                        assert_eq!(bounds.y0, y, "wrapping does not move thumbnail geometry");
                    }
                    thumbnail_y = Some(bounds.y0);
                }
            }
        }
    }

    #[test]
    fn accessible_items_follow_paths_and_current_item_dismisses_without_navigation() {
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
        let cursor = std::cell::Cell::new(egui::CursorIcon::Default);
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
            cursor.set(output.platform_output.cursor_icon);
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
        frame(&snapshot, &first, vec![egui::Event::PointerMoved(position)]);
        assert_eq!(cursor.get(), egui::CursorIcon::PointingHand);
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
        assert_eq!(cursor.get(), egui::CursorIcon::Default);
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
                == [UiAction::OpenFilmstripMedia(target.clone(), false)]
        );
        frame(&snapshot, &target, vec![]);
        assert!(
            frame(&snapshot, &target, vec![click()]).1
                == [UiAction::OpenFilmstripMedia(target.clone(), false)]
        );
        snapshot.items.remove(0);
        frame(&snapshot, &first, vec![]);
        assert!(frame(&snapshot, &first, vec![click()]).1.is_empty());
    }

    #[test]
    fn pending_preview_cards_are_quiet_but_failures_and_ready_pixels_remain() {
        let root = std::env::temp_dir().join(format!(
            "towavue-preview-placeholder-{}",
            std::process::id()
        ));
        let path = root.join("fixture.png");
        let paths = [path.clone()];
        let snapshot = FolderSnapshot {
            folder_identity: ShellIdentity::new(Vec::new()),
            folder_path: root.clone(),
            items: vec![FolderMediaItem {
                identity: ShellIdentity::new(Vec::new()),
                path: path.clone(),
                kind: MediaKind::Image,
            }],
            sort_columns: Vec::new(),
            source: FolderSnapshotSource::NaturalNameFallback,
            generation: 1,
            captured_at: SystemTime::now(),
        };
        let mut strip =
            Filmstrip::new(PreviewCache::new(root.clone()).expect("cache"), || {}).expect("worker");
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.global_style_mut(|style| {
                crate::chrome::style(style);
                style.interaction.tooltip_delay = 0.0;
                style.interaction.show_tooltips_only_when_still = false;
            });
            let texture = context.load_texture(
                "ready",
                egui::ColorImage::filled([2, 1], Color32::RED),
                egui::TextureOptions::LINEAR,
            );
            for mode in 0..3 {
                for state in 0..3 {
                    strip.previews.clear();
                    match state {
                        1 => {
                            strip
                                .previews
                                .insert(path.clone(), Err("fixture failure".into()));
                        }
                        2 => {
                            strip
                                .previews
                                .insert(path.clone(), Ok((texture.clone(), None)));
                        }
                        _ => {}
                    }
                    let track =
                        Rect::from_min_size(egui::pos2(20.0, 260.0), egui::vec2(440.0, 20.0));
                    for pass in 0..4 {
                        let output = context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(480.0, 300.0),
                                )),
                                viewports: [(
                                    egui::ViewportId::ROOT,
                                    egui::ViewportInfo {
                                        native_pixels_per_point: Some(density),
                                        ..Default::default()
                                    },
                                )]
                                .into_iter()
                                .collect(),
                                events: vec![egui::Event::PointerMoved(track.center())],
                                ..Default::default()
                            },
                            |ui| match mode {
                                0 => strip.show(
                                    &context,
                                    context.content_rect(),
                                    Some(&snapshot),
                                    Some(&path),
                                    true,
                                    &mut vec![],
                                ),
                                1 => {
                                    let response = ui.allocate_rect(track, egui::Sense::hover());
                                    strip.show_seek_preview(
                                        &response,
                                        0.5,
                                        &paths,
                                        "1 / 1  fixture.png",
                                        ReadingAxis::Horizontal,
                                    );
                                }
                                _ => {
                                    strip.show_recent(ui, &paths, true, &mut vec![]);
                                }
                            },
                        );
                        if pass < 3 {
                            continue;
                        }
                        let text: Vec<_> = output
                            .shapes
                            .iter()
                            .filter_map(|shape| match &shape.shape {
                                egui::Shape::Text(text) => Some(text.galley.text()),
                                _ => None,
                            })
                            .collect();
                        assert!(
                            !text
                                .iter()
                                .any(|text| matches!(*text, "…" | "Loading preview…")),
                            "mode {mode}, state {state}"
                        );
                        assert_eq!(text.contains(&"No preview"), state == 1, "mode {mode}");
                        assert!(
                            text.iter().any(|text| text.contains("fixture.png")),
                            "caption remains: mode {mode}"
                        );
                        assert_eq!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == texture.id())), state == 2);
                        assert_eq!(output.pixels_per_point, density);
                    }
                }
            }
        }
        drop(strip);
        std::fs::remove_dir(root).expect("remove owned empty cache");
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
            (egui::PointerButton::Primary, 480.0, Some((30_000, false))),
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
                    matches!(actions.as_slice(), [UiAction::OpenFilmstripMedia(path, actual)] if path == &snapshot.items[index].path && *actual == new_tab)
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

#[cfg(test)]
#[test]
fn snapshot_comparison_reuse_preserves_sequence_and_preview_refresh_decisions() {
    use crate::{Application, image_navigation::ImageSequence};
    use towavue_core::{FolderMediaItem, FolderSnapshotSource, ShellIdentity};
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::snapshot_comparison_reuse_preserves_sequence_and_preview_refresh_decisions",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    let base = FolderSnapshot {
        folder_identity: ShellIdentity::new(vec![0]),
        folder_path: root.clone(),
        items: (0..24_512_u32)
            .map(|index| FolderMediaItem {
                identity: ShellIdentity::new(index.to_le_bytes().to_vec()),
                path: root.join(format!("generated-{index:08}.png")),
                kind: if index == 2 {
                    MediaKind::Video
                } else {
                    MediaKind::Image
                },
            })
            .collect(),
        sort_columns: Vec::new(),
        source: FolderSnapshotSource::LiveExplorerView,
        generation: 1,
        captured_at: std::time::SystemTime::UNIX_EPOCH,
    };
    for open in [false, true] {
        for change in 0..10 {
            let previous = (change != 9).then(|| base.clone());
            let mut next = base.clone();
            next.generation = 2;
            next.source = FolderSnapshotSource::PersistedShellView;
            match change {
                0 | 9 => (),
                1 => next.items.swap(0, 1),
                2 => next.items[1].path = root.join("renamed-image.png"),
                3 => next.items[2].path = root.join("renamed-video.mp4"),
                4 => next.items[24_511].identity = ShellIdentity::new(vec![42]),
                5 => next.items.swap(1, 2),
                6 => next.items[1].kind = MediaKind::Audio,
                7 => {
                    next.items.pop();
                }
                8 => next.folder_path = root.join("another-folder"),
                _ => unreachable!(),
            }
            // Historical decisions, independently evaluated before calling the app.
            let cancel_sequence = previous.as_ref().is_some_and(|previous| {
                previous.folder_path != next.folder_path
                    || !previous
                        .items_of_kind(MediaKind::Image)
                        .map(|item| &item.path)
                        .eq(next.items_of_kind(MediaKind::Image).map(|item| &item.path))
            });
            let retain_previews = previous.as_ref().is_some_and(|previous| {
                previous.folder_path == next.folder_path && (previous.items == next.items || open)
            });
            let current = base.items[0].path.clone();
            app.path = Some(current.clone());
            app.media_kind = Some(MediaKind::Image);
            app.image_loading = true; // No missing-file decode/prefetch jobs in this metadata control.
            app.folder_snapshot = previous;
            app.image_sequence = ImageSequence {
                awaiting: Some(7),
                steps: [true, false].into(),
            };
            app.filmstrip_open = open;
            app.filmstrip.focus = Some(current.clone());
            app.filmstrip.scroll_offset = 123.0;
            app.filmstrip
                .previews
                .insert(current.clone(), Err("fixture".into()));
            app.apply_folder_snapshot(next);
            assert_eq!(
                app.image_sequence.awaiting.is_none(),
                cancel_sequence,
                "change {change}, open {open}"
            );
            assert_eq!(app.image_sequence.steps.is_empty(), cancel_sequence);
            assert_eq!(
                app.filmstrip.previews.contains_key(&current),
                retain_previews
            );
            assert_eq!(app.filmstrip.focus.is_some(), retain_previews);
            assert_eq!(
                app.filmstrip.scroll_offset,
                if retain_previews { 123.0 } else { 0.0 }
            );
            let installed = app.folder_snapshot.as_ref().expect("fresh snapshot");
            assert_eq!(installed.generation, 2);
            assert_eq!(installed.source, FolderSnapshotSource::PersistedShellView);
        }
    }
}
