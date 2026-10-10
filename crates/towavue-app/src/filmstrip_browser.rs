use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use egui::{Color32, Context, Rect, Ui, Vec2};
use towavue_core::{FolderSnapshot, MediaKind};
use towavue_runtime_windows::{FolderBrowser, FolderListing};

use crate::chrome;
use crate::hover_help::HoverHelp;
use crate::localization::{self, Text};

fn background() -> Color32 {
    Color32::from_rgba_unmultiplied(0x2c, 0x2c, 0x2c, 112)
}
const HOVER: Color32 = Color32::from_rgba_premultiplied(24, 24, 24, 24);
pub(super) const HEADER_HEIGHT: f32 = chrome::INPUT_HEIGHT + 16.0;
type SourceKey = (PathBuf, u64, SystemTime, usize);

#[derive(Default)]
pub(super) struct Browser {
    folder: PathBuf,
    pub query: String,
    pub filter: Option<MediaKind>,
    editing: bool,
    address: String,
    select_address: bool,
    worker: Option<FolderBrowser>,
    cache: Vec<Arc<FolderListing>>,
    worker_failed: bool,
    menu_open: Option<egui::Id>,
    blocked_frame: Option<u64>,
    blocked_gesture: bool,
    popup_was_open: bool,
    source: Option<SourceKey>,
    projected: Option<Arc<FolderSnapshot>>,
    projected_query: String,
    projected_filter: Option<MediaKind>,
    revision: u64,
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
        self.editing = false;
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
            return (None, false);
        };
        let key = (
            source.folder_path.clone(),
            source.generation,
            source.captured_at,
            source.items.len(),
        );
        let changed = self.projected_query != self.query || self.projected_filter != self.filter;
        if self.source.as_ref() != Some(&key) || changed {
            self.source = Some(key);
            self.projected_query.clone_from(&self.query);
            self.projected_filter = self.filter;
            self.revision = self.revision.wrapping_add(1);
            let words: Vec<_> = self
                .query
                .split_whitespace()
                .map(str::to_lowercase)
                .collect();
            let items = source
                .items
                .iter()
                .filter(|item| {
                    if self.filter.is_some_and(|kind| item.kind != kind) {
                        return false;
                    }
                    let name = crate::display_name(&item.path).to_lowercase();
                    words.iter().all(|word| name.contains(word))
                })
                .cloned()
                .collect();
            let snapshot = FolderSnapshot {
                folder_identity: source.folder_identity.clone(),
                folder_path: source.folder_path.clone(),
                items,
                sort_columns: source.sort_columns.clone(),
                source: source.source,
                generation: self.revision,
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
        source: Option<&FolderSnapshot>,
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
            // Menus use Foreground. Keep the header and its dimming below them,
            // including the popup shadow overlapping the bottom of this band.
            .order(egui::Order::Middle)
            .fade_in(false)
            .movable(false)
            .fixed_pos(header.min)
            .constrain(false)
            .show(context, |ui| {
                ui.set_min_size(header.size());
                ui.set_max_size(header.size());
                ui.set_clip_rect(header);
                ui.painter()
                    .rect_filled(header, 0.0, Color32::from_black_alpha(191));
                if !enabled {
                    let opacity = ui.opacity();
                    ui.disable();
                    ui.set_opacity(opacity);
                }
                // Reuse Gallery's minimum gutter, without its grid rounding.
                let side = crate::filmstrip::gallery_minimum_inset(screen.width())
                    .min(((screen.width() - 284.0) * 0.5).max(8.0));
                let total = (screen.width() - side * 2.0).max(1.0);
                let gap = 4.0_f32.min(total);
                let address_width = (total - gap) * 2.5 / 3.5;
                let address = Rect::from_min_size(
                    screen.min + egui::vec2(side, 8.0),
                    egui::vec2(address_width, chrome::INPUT_HEIGHT),
                );
                let search = Rect::from_min_max(
                    egui::pos2(address.right() + gap, address.top()),
                    egui::pos2(screen.right() - side, address.bottom()),
                );
                self.address_bar(ui, address, &mut target);
                self.search_bar(ui, search, source);
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
        ui.painter()
            .rect_filled(outer, chrome::INPUT_RADIUS, background());
        let language = localization::language(ui.ctx());
        // Ancestors necessarily contain the next path component. Only the
        // current folder needs an asynchronous listing to determine its arrow.
        if ui.is_enabled() && self.menu_open.is_none() {
            self.listing(ui, &self.folder.clone());
        }
        let blocked = self.blocked_frame == Some(ui.ctx().cumulative_frame_nr());
        if self.editing {
            let clear_rect = end_button(outer, 0);
            let text_rect = Rect::from_min_max(
                outer.min,
                egui::pos2(clear_rect.left() - 2.0, outer.bottom()),
            );
            let response = input(
                ui,
                text_rect,
                "filmstrip-address",
                Text::FilmstripAddress.in_language(language),
                &mut self.address,
            );
            if self.select_address {
                self.select_address = false;
                response.request_focus();
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(self.address.chars().count()),
                        )));
                    state.store(ui.ctx(), response.id);
                }
                ui.ctx().request_discard("select address on entry");
            }
            let clear = clear_button(ui, clear_rect, !self.address.is_empty());
            if clear.clicked() {
                self.address.clear();
                response.request_focus();
            }
            if response.has_focus() || response.lost_focus() {
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                    self.editing = false;
                    response.surrender_focus();
                } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                    let path = PathBuf::from(self.address.trim().trim_matches('"'));
                    if !path.as_os_str().is_empty() {
                        *target = Some(if path.is_absolute() {
                            path
                        } else {
                            self.folder.join(path)
                        });
                        self.editing = false;
                        response.surrender_focus();
                    }
                }
            }
            if response.has_focus() {
                input_focus(ui, outer);
            }
            if response.lost_focus() && !clear.clicked() {
                self.editing = false;
            }
            return;
        }
        // Reserve blank space even for a very long final component, so address
        // entry always has a direct pointer target. Ancestors elide first.
        let available = (outer.width() - 26.0).max(0.0);
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
                    + 12.0
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
        let mut x = outer.left() + 2.0;
        if first > 0 && available > 60.0 {
            let rect = Rect::from_min_size(
                egui::pos2(x, outer.top() + 2.0),
                Vec2::splat(chrome::SEARCH_BUTTON_SIZE),
            );
            let response = ui
                .add_enabled_ui(!blocked, |ui| {
                    chrome::input_button_at(
                        ui,
                        rect,
                        egui::Button::new("…").frame_when_inactive(false),
                        HOVER,
                    )
                })
                .inner;
            if response.clicked() {
                self.menu_open = Some(egui::Popup::default_response_id(&response));
            }
            egui::Popup::menu(&response)
                .style(chrome::menu_style)
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
                egui::pos2(x, outer.top() + 2.0),
                egui::vec2(widths[index], chrome::SEARCH_BUTTON_SIZE),
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
            let background = ui.painter().add(egui::Shape::Noop);
            let label = crate::folder_navigation::folder_name(path);
            let name_response = ui
                .push_id(("crumb", path), |ui| {
                    ui.add_enabled_ui(!blocked, |ui| {
                        ui.put(
                            name,
                            egui::Button::new(&label)
                                .frame(false)
                                .truncate()
                                .min_size(name.size()),
                        )
                    })
                    .inner
                })
                .inner
                .help_text(path.display().to_string());
            let arrow_response = has_children.then(|| {
                ui.push_id(("children", path), |ui| {
                    ui.add_enabled_ui(!blocked, |ui| {
                        ui.put(
                            arrow,
                            egui::Button::new("").frame(false).min_size(arrow.size()),
                        )
                    })
                    .inner
                })
                .inner
            });
            let mut open = false;
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
                open = egui::Popup::is_id_open(ui.ctx(), popup_id);
                let glyph = if open { '\u{eab4}' } else { '\u{eab6}' };
                ui.painter().text(
                    arrow.center(),
                    egui::Align2::CENTER_CENTER,
                    glyph,
                    crate::fonts::icon_font(),
                    chrome::MUTED,
                );
            }
            let left_hover = !blocked && (name_response.hovered() || name_response.has_focus());
            let right_hover = open
                || (!blocked
                    && arrow_response
                        .as_ref()
                        .is_some_and(|r| r.hovered() || r.has_focus()));
            if left_hover || right_hover {
                // Stroke and fill occupy disjoint geometry: alpha is applied once.
                let half = if right_hover { arrow } else { name };
                let radius = egui::CornerRadius {
                    nw: if left_hover && !right_hover { 2 } else { 0 },
                    sw: if left_hover && !right_hover { 2 } else { 0 },
                    ne: if right_hover || !has_children { 2 } else { 0 },
                    se: if right_hover || !has_children { 2 } else { 0 },
                };
                let fill = Rect::from_min_max(
                    egui::pos2(half.left().max(rect.left() + 1.0), rect.top() + 1.0),
                    egui::pos2(half.right().min(rect.right() - 1.0), rect.bottom() - 1.0),
                );
                ui.painter().set(
                    background,
                    egui::Shape::Vec(vec![
                        egui::Shape::rect_stroke(
                            rect,
                            3,
                            egui::Stroke::new(1.0, HOVER),
                            egui::StrokeKind::Inside,
                        ),
                        egui::Shape::rect_filled(fill, radius, HOVER),
                    ]),
                );
            }
            if name_response.clicked() {
                *target = Some(path.clone());
            }
            if let Some(arrow_response) = &arrow_response {
                egui::Popup::menu(arrow_response)
                    .style(chrome::menu_style)
                    .show(|ui| {
                        self.directory_menu(ui, path, 0, target);
                    });
            }
            x = rect.right();
        }
        let blank = Rect::from_min_max(egui::pos2(x.min(outer.right()), outer.top()), outer.max);
        if ui
            .add_enabled_ui(!blocked, |ui| {
                ui.interact(
                    blank,
                    ui.id().with("edit-filmstrip-address"),
                    egui::Sense::click(),
                )
            })
            .inner
            .clicked()
        {
            self.address = self.folder.display().to_string();
            self.editing = true;
            self.select_address = true;
            ui.ctx().request_repaint();
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
            ui.label(
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
            ui.label(Text::FilmstripFolderUnavailable.in_language(language));
            return;
        }
        if listing.folders.is_empty() {
            ui.label(Text::FilmstripNoFolders.in_language(language));
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
        let available = (ui.ctx().content_rect().size() - margin).max(Vec2::splat(1.0));
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
                    + if *children && depth < 63 { 20.0 } else { 0.0 }
            })
            .fold(32.0, f32::max)
            .min(available.x);
        // A loading popup may have been only one row high on the previous pass.
        // Restore the full constraint before ScrollArea computes its viewport.
        ui.set_max_height(available.y);
        ui.set_width(width);
        egui::ScrollArea::vertical()
            .id_salt("folder-cascade")
            .max_height(available.y)
            .show_rows(ui, 18.0, entries.len(), |ui, range| {
                let rows = chrome::MenuRows::new(ui);
                let mut ids = Vec::new();
                for index in range {
                    let (path, children) = entries[index];
                    let label = &labels[index];
                    let response = if children && depth < 63 {
                        let row = ui
                            .allocate_exact_size(
                                egui::vec2(ui.available_width(), 18.0),
                                egui::Sense::hover(),
                            )
                            .0;
                        let arrow_rect =
                            Rect::from_min_max(egui::pos2(row.right() - 20.0, row.top()), row.max);
                        let name_rect = Rect::from_min_max(
                            row.min,
                            egui::pos2(arrow_rect.left(), row.bottom()),
                        );
                        let response = ui.put(
                            name_rect,
                            egui::Button::new(label)
                                .right_text(())
                                .gap(0.0)
                                .truncate()
                                .min_size(name_rect.size()),
                        );
                        let arrow = ui.put(
                            arrow_rect,
                            egui::Button::new("⏵").min_size(arrow_rect.size()),
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
                        egui::containers::menu::SubMenu::new().show(ui, &arrow, |ui| {
                            self.directory_menu(ui, path, depth + 1, target)
                        });
                        ids.push(arrow.id);
                        response
                    } else {
                        ui.add(egui::Button::new(label).right_text(()).gap(0.0).truncate())
                    };
                    ids.push(response.id);
                    if response.clicked() {
                        *target = Some(path.to_owned());
                    }
                }
                rows.finish(ui, &ids);
            });
        if target.is_some() {
            egui::Popup::close_all(ui.ctx());
        }
    }

    fn search_bar(&mut self, ui: &mut Ui, outer: Rect, source: Option<&FolderSnapshot>) {
        ui.painter()
            .rect_filled(outer, chrome::INPUT_RADIUS, background());
        let language = localization::language(ui.ctx());
        let filter_rect = end_button(outer, 0);
        let clear_rect = end_button(outer, 1);
        let text_rect = Rect::from_min_max(
            outer.min,
            egui::pos2(clear_rect.left().max(outer.left()), outer.bottom()),
        );
        let hint = format!(
            "{} {}",
            Text::FilmstripSearch.in_language(language),
            crate::folder_navigation::folder_name(&self.folder)
        );
        let response = input(ui, text_rect, "filmstrip-search", &hint, &mut self.query);
        if clear_button(ui, clear_rect, !self.query.is_empty()).clicked() {
            self.query.clear();
            response.request_focus();
        }
        let previous = self.filter;
        crate::welcome::media_filter_button(
            ui,
            filter_rect,
            &mut self.filter,
            |kind| source.is_some_and(|s| s.items.iter().any(|item| item.kind == kind)),
            HOVER,
        );
        if previous != self.filter {
            ui.ctx().request_repaint();
        }
        if response.has_focus()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.query.clear();
        }
        if response.has_focus() {
            input_focus(ui, outer);
        }
    }
}

fn end_button(outer: Rect, index: usize) -> Rect {
    Rect::from_min_size(
        outer.right_top()
            + egui::vec2(
                -((index + 1) as f32) * (chrome::SEARCH_BUTTON_SIZE + 2.0),
                2.0,
            ),
        Vec2::splat(chrome::SEARCH_BUTTON_SIZE),
    )
    .intersect(outer)
}

fn input(ui: &mut Ui, rect: Rect, id: &str, hint: &str, value: &mut String) -> egui::Response {
    let input_id = ui.id().with(id);
    let response = ui.put(rect, |ui: &mut Ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(rect));
        ui.spacing_mut().text_edit_width = f32::INFINITY;
        crate::resize::unframed_text_input_with_id(ui, input_id, hint, value)
    });
    if response.has_focus() {
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                    ..Default::default()
                },
            )
        });
    }
    response
}

fn input_focus(ui: &Ui, rect: Rect) {
    ui.painter().rect_stroke(
        rect,
        chrome::INPUT_RADIUS,
        ui.visuals().selection.stroke,
        egui::StrokeKind::Inside,
    );
}

fn clear_button(ui: &mut Ui, rect: Rect, enabled: bool) -> egui::Response {
    let label = localization::text(ui.ctx(), Text::FilmstripClear);
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            chrome::input_button_at(
                ui,
                rect,
                egui::Button::new(egui::RichText::new("\u{eabf}").font(crate::fonts::icon_font()))
                    .stroke(egui::Stroke::NONE)
                    .frame_when_inactive(false),
                HOVER,
            )
        })
        .inner
        .help_text(label);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), label)
    });
    response
}

#[cfg(test)]
mod tests;
