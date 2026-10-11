use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use egui::{Color32, Context, Rect, Ui, Vec2};
use towavue_core::{FolderSnapshot, MediaKind};
use towavue_runtime_windows::{FolderBrowser, FolderListing};

use crate::chrome;
use crate::hover_help::HoverHelp;
use crate::localization::{self, Text};

const HOVER: Color32 = Color32::from_rgba_premultiplied(24, 24, 24, 24);
pub(super) const HEADER_HEIGHT: f32 = chrome::INPUT_HEIGHT + 36.0;
type SourceKey = (PathBuf, u64, SystemTime, usize);

fn menu_message(ui: &mut Ui, label: &str) {
    let width = egui::WidgetText::from(label)
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Button,
        )
        .size()
        .x
        + 2.0 * ui.spacing().button_padding.x;
    ui.set_width(
        width
            .ceil()
            .min(ui.ctx().content_rect().width() - 10.0)
            .max(1.0),
    );
    ui.add_enabled(false, egui::Button::new(label).truncate());
    chrome::remember_menu_bounds(ui);
}

#[derive(Default)]
pub(super) struct Browser {
    folder: PathBuf,
    pub query: String,
    pub filter: Option<MediaKind>,
    worker: Option<FolderBrowser>,
    cache: Vec<Arc<FolderListing>>,
    worker_failed: bool,
    menu_open: Option<egui::Id>,
    menu_floor: f32,
    blocked_frame: Option<u64>,
    blocked_gesture: bool,
    popup_was_open: bool,
    source: Option<SourceKey>,
    projected: Option<Arc<FolderSnapshot>>,
    projected_query: String,
    projected_filter: Option<MediaKind>,
    counts: crate::browser_controls::Counts,
}

impl Browser {
    pub fn hide(&mut self, context: Option<&Context>) {
        if let Some(id) = self.menu_open.take()
            && let Some(context) = context
        {
            egui::Popup::close_id(context, id);
        }
        if let Some(worker) = &mut self.worker {
            worker.request(None);
        }
        self.cache.clear();
    }

    pub fn sync_folder(&mut self, folder: &Path, context: &Context) {
        if self.folder != folder {
            self.hide(Some(context));
            self.folder = folder.to_owned();
            self.query.clear();
            self.source = None;
            self.projected = None;
        }
    }

    pub fn snapshot(&self) -> Option<&FolderSnapshot> {
        self.projected.as_deref()
    }

    /// Derive a UI-only snapshot on source/query changes, never on every paint.
    /// Paths and Shell identities remain intact; no filtered index enters dispatch.
    pub fn project(
        &mut self,
        source: Option<&FolderSnapshot>,
    ) -> (Option<Arc<FolderSnapshot>>, bool) {
        let Some(source) = source else {
            self.source = None;
            self.projected = None;
            self.counts.values = [0; 4];
            return (None, false);
        };
        let key = (
            source.folder_path.clone(),
            source.generation,
            source.captured_at,
            source.items.len(),
        );
        let mut changed =
            self.projected_query != self.query || self.projected_filter != self.filter;
        if self.source.as_ref() != Some(&key) || changed {
            self.source = Some(key);
            self.projected_query.clone_from(&self.query);
            let words: Vec<_> = self
                .query
                .split_whitespace()
                .map(str::to_lowercase)
                .collect();
            self.counts.values = [0; 4];
            let mut items: Vec<_> = source
                .items
                .iter()
                .filter(|item| {
                    let name = crate::display_name(&item.path).to_lowercase();
                    if !words.iter().all(|word| name.contains(word)) {
                        return false;
                    }
                    self.counts.add(item.kind);
                    true
                })
                .cloned()
                .collect();
            crate::browser_controls::normalize_filter(&self.counts.values, &mut self.filter);
            changed |= self.projected_filter != self.filter;
            self.projected_filter = self.filter;
            items.retain(|item| self.filter.is_none_or(|kind| item.kind == kind));
            let snapshot = FolderSnapshot {
                folder_identity: source.folder_identity.clone(),
                folder_path: source.folder_path.clone(),
                items,
                sort_columns: source.sort_columns.clone(),
                source: source.source,
                generation: source.generation,
                captured_at: source.captured_at,
            };
            self.projected = Some(Arc::new(snapshot));
        }
        (self.projected.clone(), changed)
    }

    pub fn show(
        &mut self,
        context: &Context,
        screen: Rect,
        _source: Option<&FolderSnapshot>,
        enabled: bool,
    ) -> Option<PathBuf> {
        let mut target = None;
        // A popup can close before these widgets run, or trigger a second pass.
        // Keep the outside click owned through release, even if the popup has
        // disappeared before the address widgets are evaluated again.
        if egui::Popup::is_any_open(context) || self.popup_was_open || self.blocked_gesture {
            self.blocked_frame = Some(context.cumulative_frame_nr());
            self.blocked_gesture = context.input(|input| input.pointer.any_down());
        }
        let header = Rect::from_min_size(
            screen.min,
            egui::vec2(screen.width(), HEADER_HEIGHT.min(screen.height())),
        );
        egui::Area::new("filmstrip-browser".into())
            .order(egui::Order::Foreground)
            .fade_in(false)
            .movable(false)
            .fixed_pos(header.min)
            .constrain(false)
            .show(context, |ui| {
                ui.set_min_size(header.size());
                ui.set_max_size(header.size());
                ui.set_clip_rect(header);

                if !enabled {
                    let opacity = ui.opacity();
                    ui.disable();
                    ui.set_opacity(opacity);
                }
                let content = screen.shrink2(egui::vec2(crate::browser_controls::INSET, 0.0));
                let total = content.width().max(1.0);
                let gap = 16.0_f32.min(total);
                let desired_address: f32 = self
                    .folder
                    .ancestors()
                    .map(|path| {
                        ui.painter()
                            .layout_no_wrap(
                                crate::folder_navigation::folder_name(path),
                                egui::TextStyle::Body.resolve(ui.style()),
                                Color32::WHITE,
                            )
                            .size()
                            .x
                            + if self.has_children(path) {
                                chrome::SEARCH_BUTTON_SIZE
                            } else {
                                0.0
                            }
                    })
                    .sum();
                let search_width = (total - gap - desired_address).clamp(
                    crate::browser_controls::SEARCH_MIN,
                    crate::browser_controls::SEARCH_MAX,
                );
                let address = Rect::from_min_size(
                    content.min + egui::vec2(0.0, crate::browser_controls::INSET),
                    egui::vec2(
                        (total - gap - search_width).max(0.0),
                        crate::browser_controls::text_height(ui),
                    ),
                );
                let search = Rect::from_min_max(
                    egui::pos2(address.right() + gap, address.top()),
                    egui::pos2(content.right(), address.bottom()),
                );
                self.address_bar(ui, address, &mut target);
                crate::browser_controls::search(
                    ui,
                    search,
                    ui.id().with("filmstrip-search"),
                    &format!(
                        "{} {}",
                        localization::text(context, Text::FilmstripSearch),
                        crate::folder_navigation::folder_name(&self.folder)
                    ),
                    &mut self.query,
                );
            });
        if self
            .menu_open
            .is_some_and(|id| !egui::Popup::is_id_open(context, id))
        {
            self.menu_open = None;
            if let Some(worker) = &mut self.worker {
                worker.request(None);
            }
        }
        self.popup_was_open = egui::Popup::is_any_open(context);
        if !self.popup_was_open && self.blocked_frame == Some(context.cumulative_frame_nr()) {
            // Rebuild enabled hit regions after the owned closing gesture,
            // even if the pointer remains still until the next click.
            context.request_repaint();
        }
        target
    }

    fn address_bar(&mut self, ui: &mut Ui, outer: Rect, target: &mut Option<PathBuf>) {
        let language = localization::language(ui.ctx());
        self.menu_floor = outer.bottom();
        // Ancestors necessarily contain the next path component. Only the
        // current folder needs an asynchronous listing to determine its arrow.
        if ui.is_enabled() && self.menu_open.is_none() {
            self.listing(ui, &self.folder.clone());
        }
        let blocked = self.blocked_frame == Some(ui.ctx().cumulative_frame_nr());
        let available = outer.width().max(0.0);
        let mut paths: Vec<_> = self.folder.ancestors().map(Path::to_owned).collect();
        paths.reverse();
        let mut widths: Vec<_> = paths
            .iter()
            .map(|path| {
                let text = crate::folder_navigation::folder_name(path);
                ui.painter()
                    .layout_no_wrap(
                        text,
                        egui::TextStyle::Body.resolve(ui.style()),
                        chrome::FOREGROUND,
                    )
                    .size()
                    .x
                    + if self.has_children(path) {
                        chrome::SEARCH_BUTTON_SIZE
                    } else {
                        0.0
                    }
            })
            .collect();
        let mut first = 0;
        while first + 1 < paths.len()
            && widths[first..].iter().sum::<f32>()
                + if first > 0 {
                    chrome::SEARCH_BUTTON_SIZE
                } else {
                    0.0
                }
                > available
        {
            first += 1;
        }
        let mut x = outer.left();
        if first > 0 && available > 60.0 {
            let rect = Rect::from_min_size(
                egui::pos2(x, outer.top()),
                egui::vec2(chrome::SEARCH_BUTTON_SIZE, outer.height()),
            );
            let response = ui
                .scope(|ui| {
                    if blocked {
                        let opacity = ui.opacity();
                        ui.disable();
                        ui.set_opacity(opacity);
                    }
                    crate::browser_controls::text_button_color(
                        ui,
                        rect,
                        ui.id().with("ancestors"),
                        "…",
                        false,
                        Color32::WHITE,
                    )
                })
                .inner;
            if response.clicked() {
                self.menu_open = Some(egui::Popup::default_response_id(&response));
            }
            egui::Popup::menu(&response)
                .style(chrome::menu_style)
                .align_alternatives(&[])
                .show(|ui| {
                    let entries: Vec<_> = paths[..first]
                        .iter()
                        .rev()
                        .map(|path| (path.as_path(), true))
                        .collect();
                    self.folder_rows(ui, &entries, 0, target);
                });
            x = rect.right();
        }
        for index in first..paths.len() {
            widths[index] = widths[index].min((outer.left() + available - x).max(0.0));
            let rect = Rect::from_min_size(
                egui::pos2(x, outer.top()),
                egui::vec2(widths[index], outer.height()),
            );
            if rect.width() < 2.0 {
                break;
            }
            let path = &paths[index];
            let has_children = self.has_children(path);
            let split = (rect.right()
                - if has_children {
                    chrome::SEARCH_BUTTON_SIZE
                } else {
                    0.0
                })
            .max(rect.left());
            let name = Rect::from_min_max(rect.min, egui::pos2(split, rect.bottom()));
            let arrow = Rect::from_min_max(egui::pos2(split, rect.top()), rect.max);
            let label = crate::folder_navigation::folder_name(path);
            let name_response = ui
                .push_id(("crumb", path), |ui| {
                    ui.scope(|ui| {
                        if blocked {
                            let opacity = ui.opacity();
                            ui.disable();
                            ui.set_opacity(opacity);
                        }
                        crate::browser_controls::text_button_color(
                            ui,
                            name,
                            ui.id().with("name"),
                            &label,
                            false,
                            Color32::WHITE,
                        )
                    })
                    .inner
                })
                .inner
                .help_text(path.display().to_string());
            let arrow_response = has_children.then(|| {
                ui.push_id(("children", path), |ui| {
                    ui.scope(|ui| {
                        if blocked {
                            let opacity = ui.opacity();
                            ui.disable();
                            ui.set_opacity(opacity);
                        }
                        ui.interact(arrow, ui.id().with("arrow"), egui::Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                    })
                    .inner
                })
                .inner
            });
            if let Some(arrow_response) = &arrow_response {
                arrow_response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        arrow_response.enabled(),
                        format!(
                            "{}: {label}",
                            Text::FilmstripSubfolders.in_language(language)
                        ),
                    )
                });
                let popup_id = egui::Popup::default_response_id(arrow_response);
                if arrow_response.clicked() {
                    self.menu_open = Some(popup_id);
                }
                let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
                let glyph = if open { '\u{eab4}' } else { '\u{eab6}' };
                ui.painter().text(
                    arrow.center(),
                    egui::Align2::CENTER_CENTER,
                    glyph,
                    crate::fonts::icon_font(),
                    Color32::WHITE,
                );
            }
            if name_response.clicked() {
                *target = Some(path.clone());
            }
            if let Some(arrow_response) = &arrow_response {
                egui::Popup::menu(arrow_response)
                    .style(chrome::menu_style)
                    .align_alternatives(&[])
                    .show(|ui| {
                        self.directory_menu(ui, path, 0, target);
                    });
            }
            x = rect.right();
        }
    }

    fn has_children(&self, path: &Path) -> bool {
        path != self.folder
            || self
                .cache
                .iter()
                .find(|listing| listing.path == path)
                .is_some_and(|listing| !listing.folders.is_empty())
    }

    fn listing(&mut self, ui: &Ui, path: &Path) -> Option<Arc<FolderListing>> {
        if let Some(result) = self.worker.as_ref().and_then(FolderBrowser::result)
            && !self.cache.iter().any(|saved| saved.path == result.path)
        {
            if self.cache.len() == 64 {
                self.cache.remove(0);
            }
            self.cache.push(result);
        }
        if let Some(result) = self.cache.iter().find(|result| result.path == path) {
            return Some(result.clone());
        }
        if self.worker.is_none() && !self.worker_failed {
            let context = ui.ctx().clone();
            match FolderBrowser::new(move || context.request_repaint()) {
                Ok(worker) => self.worker = Some(worker),
                Err(_) => self.worker_failed = true,
            }
        }
        if let Some(worker) = &mut self.worker {
            worker.request(Some(path.to_owned()));
        }
        None
    }

    fn directory_menu(
        &mut self,
        ui: &mut Ui,
        path: &Path,
        depth: usize,
        target: &mut Option<PathBuf>,
    ) {
        let language = localization::language(ui.ctx());
        let Some(listing) = self.listing(ui, path) else {
            menu_message(
                ui,
                if self.worker_failed {
                    Text::FilmstripFolderUnavailable
                } else {
                    Text::FilmstripLoadingFolders
                }
                .in_language(language),
            );
            return;
        };
        if listing.unavailable {
            menu_message(ui, Text::FilmstripFolderUnavailable.in_language(language));
            return;
        }
        if listing.folders.is_empty() {
            menu_message(ui, Text::FilmstripNoFolders.in_language(language));
            return;
        }
        let entries: Vec<_> = listing
            .folders
            .iter()
            .map(|entry| (entry.path.as_path(), entry.has_children))
            .collect();
        self.folder_rows(ui, &entries, depth, target);
        if listing.truncated {
            ui.label(Text::FilmstripFolderLimit.in_language(language));
        }
    }

    fn folder_rows(
        &mut self,
        ui: &mut Ui,
        entries: &[(&Path, bool)],
        depth: usize,
        target: &mut Option<PathBuf>,
    ) {
        let language = localization::language(ui.ctx());
        let margin = egui::Frame::popup(ui.style()).total_margin().sum();
        let screen = ui.ctx().content_rect();
        let available = egui::vec2(
            ui.available_width().min(screen.width() - margin.x),
            screen.bottom() - self.menu_floor - margin.y,
        )
        .max(Vec2::splat(1.0));
        let labels: Vec<_> = entries
            .iter()
            .map(|(path, _)| crate::folder_navigation::folder_name(path))
            .collect();
        let width = entries
            .iter()
            .zip(&labels)
            .map(|((_, children), label)| {
                ui.painter()
                    .layout_no_wrap(
                        label.clone(),
                        egui::TextStyle::Button.resolve(ui.style()),
                        chrome::FOREGROUND,
                    )
                    .size()
                    .x
                    + 2.0 * ui.spacing().button_padding.x
                    + if *children && depth < 63 {
                        chrome::SEARCH_BUTTON_SIZE
                    } else {
                        0.0
                    }
            })
            .fold(32.0, f32::max)
            .min(if depth == 0 {
                screen.width() - margin.x
            } else {
                available.x
            });
        // A loading popup may have been only one row high on the previous pass.
        // Restore the full constraint before ScrollArea computes its viewport.
        ui.set_max_height(available.y);
        ui.set_width(width);
        egui::ScrollArea::vertical()
            .id_salt("folder-cascade")
            .max_height(available.y)
            .show_rows(
                ui,
                chrome::SEARCH_BUTTON_SIZE,
                entries.len(),
                |ui, range| {
                    let rows = chrome::MenuRows::new(ui);
                    let mut ids = Vec::new();
                    for index in range {
                        let (path, children) = entries[index];
                        let label = &labels[index];
                        let response = if children && depth < 63 {
                            let row = ui
                                .allocate_exact_size(
                                    egui::vec2(ui.available_width(), chrome::SEARCH_BUTTON_SIZE),
                                    egui::Sense::hover(),
                                )
                                .0;
                            let arrow_rect = Rect::from_min_max(
                                egui::pos2(row.right() - chrome::SEARCH_BUTTON_SIZE, row.top()),
                                row.max,
                            );
                            let name_rect = Rect::from_min_max(
                                row.min,
                                egui::pos2(arrow_rect.left(), row.bottom()),
                            );
                            let background = ui.painter().add(egui::Shape::Noop);
                            let response = ui.put(
                                name_rect,
                                egui::Button::new(label)
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE)
                                    .right_text(())
                                    .gap(0.0)
                                    .truncate()
                                    .min_size(name_rect.size()),
                            );
                            let arrow = ui.put(
                                arrow_rect,
                                egui::Button::new(chrome::chevron())
                                    .frame(false)
                                    .min_size(arrow_rect.size()),
                            );
                            arrow.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    arrow.enabled(),
                                    format!(
                                        "{}: {label}",
                                        Text::FilmstripSubfolders.in_language(language)
                                    ),
                                )
                            });
                            chrome::submenu(
                                ui,
                                &arrow,
                                Some(self.menu_floor),
                                Some(arrow.rect.left()),
                                |ui| self.directory_menu(ui, path, depth + 1, target),
                            );
                            let open =
                                egui::containers::menu::MenuState::from_ui(ui, |state, _| {
                                    state.open_item
                                }) == Some(egui::containers::menu::SubMenu::id_from_widget_id(
                                    arrow.id,
                                ));
                            ui.painter().set(
                                background,
                                split_background(
                                    row,
                                    arrow_rect.left(),
                                    response.hovered() || response.has_focus(),
                                    open || arrow.hovered() || arrow.has_focus(),
                                    HOVER,
                                ),
                            );
                            response
                        } else {
                            ui.add(egui::Button::new(label).right_text(()).gap(0.0).truncate())
                        };
                        if !children || depth >= 63 {
                            ids.push(response.id);
                        }
                        if response.clicked() {
                            *target = Some(path.to_owned());
                        }
                    }
                    rows.finish(ui, &ids);
                },
            );
        chrome::remember_menu_bounds(ui);
        if target.is_some() {
            egui::Popup::close_all(ui.ctx());
        }
    }

    pub fn show_filters(&mut self, context: &Context, screen: Rect, enabled: bool) {
        let inset = screen.shrink(crate::browser_controls::INSET);
        egui::Area::new("filmstrip-filters".into())
            .order(egui::Order::Foreground)
            .fade_in(false)
            .movable(false)
            .pivot(egui::Align2::RIGHT_BOTTOM)
            .fixed_pos(inset.max)
            .sense(egui::Sense::hover())
            .constrain(false)
            .show(context, |ui| {
                context.set_sublayer(
                    egui::LayerId::new(egui::Order::Foreground, "filmstrip".into()),
                    ui.layer_id(),
                );
                ui.set_clip_rect(screen);
                if !enabled || egui::Popup::is_any_open(context) {
                    ui.disable();
                }
                let size = crate::browser_controls::filter_size(ui, &self.counts.values, true);
                let caption =
                    context.data(|data| data.get_temp::<Rect>(egui::Id::new("filmstrip-caption")));
                let vertical =
                    caption.is_none_or(|caption| inset.bottom() - size.y > caption.bottom());
                let mut size =
                    crate::browser_controls::filter_size(ui, &self.counts.values, vertical);
                size.x = size.x.min(inset.width());
                let size_id = ui.id().with("content-size");
                if ui.data(|data| data.get_temp::<Vec2>(size_id)) != Some(size) {
                    ui.data_mut(|data| data.insert_temp(size_id, size));
                    // Resolve the bottom-right pivot against this content width before
                    // presenting a resize or a changed set of superscript counts.
                    context.request_discard("Resolve Filmstrip filter bounds");
                }
                ui.set_min_size(size);
                ui.set_max_size(size);
                let rect = Rect::from_min_size(ui.min_rect().min, size);
                crate::browser_controls::filters(
                    ui,
                    rect,
                    &self.counts.values,
                    &mut self.filter,
                    vertical,
                );
            });
    }
}

#[cfg(test)]
mod tests;

fn split_background(
    rect: Rect,
    split: f32,
    left: bool,
    right: bool,
    color: Color32,
) -> egui::Shape {
    if !left && !right {
        return egui::Shape::Noop;
    }
    let has_arrow = split < rect.right();
    let half = if right {
        Rect::from_min_max(egui::pos2(split, rect.top()), rect.max)
    } else {
        Rect::from_min_max(rect.min, egui::pos2(split, rect.bottom()))
    };
    let radius = egui::CornerRadius {
        nw: if left && !right { 2 } else { 0 },
        sw: if left && !right { 2 } else { 0 },
        ne: if right || !has_arrow { 2 } else { 0 },
        se: if right || !has_arrow { 2 } else { 0 },
    };
    let fill = Rect::from_min_max(
        egui::pos2(half.left().max(rect.left() + 1.0), rect.top() + 1.0),
        egui::pos2(half.right().min(rect.right() - 1.0), rect.bottom() - 1.0),
    );
    // Disjoint stroke and fill apply translucent feedback exactly once.
    egui::Shape::Vec(vec![
        egui::Shape::rect_stroke(
            rect,
            3,
            egui::Stroke::new(1.0, color),
            egui::StrokeKind::Inside,
        ),
        egui::Shape::rect_filled(fill, radius, color),
    ])
}
