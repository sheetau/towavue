use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use egui::{Color32, Context, Rect, Ui, Vec2};
use towavue_core::{FolderSnapshot, MediaKind};
use towavue_runtime_windows::{FolderBrowser, FolderListing};

use crate::chrome;
use crate::hover_help::HoverHelp;
use crate::localization::{self, Text};
use crate::scroll_style::ScrollAreaStyle;
mod prefetch;

#[derive(Default)]
pub(super) struct View {
    folder: PathBuf,
    query: String,
    filter: Option<MediaKind>,
    search_focused: bool,
}

pub(super) const HEADER_HEIGHT: f32 = chrome::STATUS_HEIGHT;
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

#[derive(Debug, PartialEq)]
pub(super) enum Action {
    Folder(PathBuf, bool),
    History(bool),
    Menu(crate::thumbnail_menu::Intent),
}

#[derive(Default)]
pub(super) struct Browser {
    pub owner: Option<crate::thumbnail_menu::Owner>,
    pub navigation: [bool; 2],
    pub current_path: Option<PathBuf>,
    tab: Option<towavue_core::TabId>,
    views: std::collections::BTreeMap<towavue_core::TabId, View>,
    search_focused: bool,
    restore_search_focus: bool,
    folder: PathBuf,
    pub query: String,
    pub filter: Option<MediaKind>,
    worker: Option<FolderBrowser>,
    cache: Vec<Arc<FolderListing>>,
    prefetch: prefetch::Prefetch,
    prepare_after: Option<f64>,
    prepare_pointer: Option<egui::Pos2>,
    worker_failed: bool,
    menu_open: Option<egui::Id>,
    menu_floor: f32,
    source: Option<SourceKey>,
    projected: Option<Arc<FolderSnapshot>>,
    projected_query: String,
    projected_filter: Option<MediaKind>,
    counts: crate::browser_controls::Counts,
}

impl Browser {
    pub fn switch_tab(&mut self, tab: Option<towavue_core::TabId>, context: Option<&Context>) {
        if self.tab == tab {
            return;
        }
        self.hide(context);
        if let Some(old) = self.tab {
            self.views.insert(
                old,
                View {
                    folder: std::mem::take(&mut self.folder),
                    query: std::mem::take(&mut self.query),
                    filter: self.filter.take(),
                    search_focused: self.search_focused,
                },
            );
        }
        self.tab = tab;
        let view = tab
            .and_then(|id| self.views.remove(&id))
            .unwrap_or_default();
        self.folder = view.folder;
        self.query = view.query;
        self.filter = view.filter;
        self.search_focused = view.search_focused;
        self.restore_search_focus = view.search_focused;
        self.source = None;
        self.projected = None;
        self.projected_query.clone_from(&self.query);
        self.projected_filter = self.filter;
    }

    pub fn remove_tab(
        &mut self,
        tab: towavue_core::TabId,
        context: Option<&Context>,
    ) -> Option<View> {
        if self.tab == Some(tab) {
            self.switch_tab(None, context);
        }
        self.views.remove(&tab)
    }

    pub fn adopt_tab(&mut self, tab: towavue_core::TabId, view: View) {
        self.views.insert(tab, view);
    }

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
        self.prefetch = prefetch::Prefetch::default();
        self.prepare_after = None;
        self.prepare_pointer = None;
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
            let expression = towavue_core::FileQuery::new(&self.query, None);
            self.counts.values = [0; 4];
            let mut items: Vec<_> = source
                .items
                .iter()
                .filter(|item| {
                    if !expression.matches_path(&item.path, false) {
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
    ) -> Option<Action> {
        let mut target = None;
        let header = Rect::from_min_size(
            egui::pos2(
                screen.left(),
                (screen.bottom() - HEADER_HEIGHT).max(screen.top()),
            ),
            egui::vec2(screen.width(), HEADER_HEIGHT.min(screen.height())),
        );
        egui::Area::new("filmstrip-browser".into())
            .order(egui::Order::Middle)
            .fade_in(false)
            .movable(false)
            .fixed_pos(header.min)
            .constrain(false)
            .show(context, |ui| {
                context.set_sublayer(
                    egui::LayerId::new(egui::Order::Middle, "filmstrip".into()),
                    ui.layer_id(),
                );
                ui.set_min_size(header.size());
                ui.set_max_size(header.size());
                ui.set_clip_rect(header);

                if !enabled {
                    let opacity = ui.opacity();
                    ui.disable();
                    ui.set_opacity(opacity);
                }
                // Apply arriving arrow metadata before measuring either column.
                // A late change inside address_bar would elide ancestors for one frame.
                if ui.is_enabled() && self.menu_open.is_none() {
                    let prepare = self.background_ready(ui);
                    self.listing(ui, &self.folder.clone(), prepare);
                }
                ui.painter().rect_filled(header, 0.0, chrome::BACKGROUND);
                ui.style_mut().override_font_id = Some(egui::FontId::proportional(chrome::TEXT_LG));
                let content = header.shrink(3.0);
                let filter_width = crate::browser_controls::filter_size(ui, &self.counts.values)
                    .x
                    .min((content.width() - 209.0).max(0.0));
                let filters = Rect::from_min_max(
                    egui::pos2(content.right() - filter_width, content.top()),
                    content.max,
                );
                crate::browser_controls::filters(
                    ui,
                    filters,
                    &self.counts.values,
                    &mut self.filter,
                );
                for (index, forward) in [false, true].into_iter().enumerate() {
                    let rect = Rect::from_min_size(
                        content.min + egui::vec2(index as f32 * 26.0, 0.0),
                        Vec2::splat(24.0),
                    );
                    let label = localization::text(
                        context,
                        if forward {
                            Text::CommandHistoryForward
                        } else {
                            Text::CommandHistoryBack
                        },
                    );
                    let response = ui
                        .add_enabled_ui(self.navigation[index], |ui| {
                            crate::status_bar::icon_at(
                                ui,
                                rect,
                                if forward {
                                    crate::fluent::Kind::Move
                                } else {
                                    crate::fluent::Kind::Back
                                },
                                false,
                                label,
                            )
                        })
                        .inner;
                    if response.clicked() {
                        target = Some(Action::History(forward));
                    }
                }
                let search = Rect::from_min_max(
                    content.min + egui::vec2(52.0, 0.0),
                    egui::pos2(
                        (filters.left() - 2.0).max(content.left() + 52.0),
                        content.bottom(),
                    ),
                );
                let search_id = ui.id().with(("filmstrip-search", self.tab));
                if self.restore_search_focus && ui.is_enabled() {
                    ui.data_mut(|data| data.insert_temp(search_id.with("editing"), true));
                    ui.memory_mut(|memory| memory.request_focus(search_id));
                    self.restore_search_focus = false;
                }
                crate::browser_controls::search(
                    ui,
                    search,
                    search_id,
                    localization::text(context, Text::FilmstripSearch),
                    &mut self.query,
                );
                if !crate::browser_controls::search_active(ui, search_id, &self.query) {
                    let left = search.left() + 26.0 + chrome::TAB_PADDING;
                    let path = self.current_path.clone();
                    let name = path.as_deref().map(crate::display_name);
                    let name_width = name
                        .as_ref()
                        .map_or(0.0, |name| crate::status_bar::width(ui, name))
                        .min(((search.right() - left) * 0.4).max(0.0));
                    let address = Rect::from_min_max(
                        egui::pos2(left, search.top()),
                        egui::pos2(
                            (search.right() - name_width - 20.0).max(left),
                            search.bottom(),
                        ),
                    );
                    let end = self.address_bar(ui, address, &mut target);
                    if let (Some(path), Some(name)) = (path, name) {
                        let slash = Rect::from_min_size(
                            egui::pos2(end + 2.0, search.top()),
                            egui::vec2(16.0, 24.0),
                        );
                        ui.painter().text(
                            slash.center(),
                            egui::Align2::CENTER_CENTER,
                            "/",
                            egui::FontId::proportional(chrome::TEXT_LG),
                            chrome::MUTED,
                        );
                        let slot = Rect::from_min_size(
                            slash.right_top() + egui::vec2(2.0, 0.0),
                            egui::vec2(name_width, 24.0),
                        );
                        let response = crate::status_bar::item_at(
                            ui,
                            slot,
                            egui::RichText::new(name).size(chrome::TEXT_LG),
                            true,
                        );
                        let mut menu_actions = Vec::new();
                        crate::status_bar::path_menu(
                            ui,
                            &response,
                            &path,
                            self.owner,
                            &mut menu_actions,
                        );
                        if let Some(crate::UiAction::ThumbnailMenu(intent)) = menu_actions.pop() {
                            target = Some(Action::Menu(intent));
                        }
                    }
                }
                // Tab chrome releases text focus before dispatching the tab switch.
                // Keep the departing editor's role through that pointer gesture.
                let title_bottom = context.content_rect().top() + chrome::TITLE_HEIGHT;
                let chrome_gesture = ui.input(|input| {
                    (input.pointer.any_down() || input.pointer.any_released())
                        && input
                            .pointer
                            .interact_pos()
                            .is_some_and(|pos| pos.y < title_bottom)
                });
                if ui.is_enabled() && !chrome_gesture {
                    self.search_focused = ui.memory(|memory| memory.has_focus(search_id));
                }
            });
        if self
            .menu_open
            .is_some_and(|id| !egui::Popup::is_id_open(context, id))
        {
            self.menu_open = None;
        }
        target
    }

    fn address_bar(&mut self, ui: &mut Ui, outer: Rect, target: &mut Option<Action>) -> f32 {
        let language = localization::language(ui.ctx());
        self.menu_floor = ui.ctx().content_rect().top() + chrome::TITLE_HEIGHT;
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
                        chrome::ICON_SIZE + 2.0
                    } else {
                        0.0
                    }
            })
            .collect();
        let mut first = 0;
        while first + 1 < paths.len()
            && widths[first..].iter().sum::<f32>()
                + if first > 0 {
                    chrome::ICON_SIZE + 2.0
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
                egui::vec2(chrome::ICON_SIZE + 2.0, outer.height()),
            );
            let response = crate::browser_controls::text_button_color(
                ui,
                rect,
                ui.id().with("ancestors"),
                "…",
                false,
                crate::chrome::MUTED,
            );
            if response.clicked() {
                self.menu_open = Some(egui::Popup::default_response_id(&response));
            }
            egui::Popup::menu(&response)
                .style(chrome::menu_style)
                .align(egui::RectAlign::TOP_START)
                .align_alternatives(&[])
                .show(|ui| {
                    let entries: Vec<_> = paths[..first]
                        .iter()
                        .rev()
                        .map(|path| (path.as_path(), Some(true)))
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
                    chrome::ICON_SIZE + 2.0
                } else {
                    0.0
                })
            .max(rect.left());
            let name = Rect::from_min_max(rect.min, egui::pos2(split, rect.bottom()));
            let arrow = Rect::from_center_size(
                egui::pos2(rect.right() - 8.0, rect.center().y),
                Vec2::splat(16.0),
            );
            let label = crate::folder_navigation::folder_name(path);
            let name_response = ui
                .push_id(("crumb", path), |ui| {
                    crate::browser_controls::text_button_color(
                        ui,
                        name,
                        ui.id().with("name"),
                        &label,
                        false,
                        crate::chrome::MUTED,
                    )
                })
                .inner
                .help_text(path.display().to_string());
            let mut menu_actions = Vec::new();
            crate::thumbnail_menu::show(
                ui,
                &name_response,
                path,
                crate::thumbnail_menu::Scope::Folder,
                self.owner,
                &mut menu_actions,
            );
            if let Some(crate::UiAction::ThumbnailMenu(intent)) = menu_actions.pop() {
                *target = Some(Action::Menu(intent));
            }
            let arrow_response = has_children.then(|| {
                ui.push_id(("children", path), |ui| {
                    ui.interact(arrow, ui.id().with("arrow"), egui::Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
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
                if arrow_response.hovered() {
                    ui.painter()
                        .rect_filled(arrow, chrome::RADIUS_XS, chrome::ACTIVE_TAB);
                }
                crate::fluent::paint(
                    ui,
                    arrow,
                    crate::fluent::Kind::ChevronUp,
                    if arrow_response.hovered() {
                        chrome::FOREGROUND
                    } else {
                        chrome::MUTED
                    },
                );
            }
            if name_response.clicked() || name_response.middle_clicked() {
                *target = Some(Action::Folder(path.clone(), name_response.middle_clicked()));
            }
            if let Some(arrow_response) = &arrow_response {
                egui::Popup::menu(arrow_response)
                    .style(chrome::menu_style)
                    .align(egui::RectAlign::TOP_START)
                    .align_alternatives(&[])
                    .show(|ui| {
                        self.directory_menu(ui, path, 0, target);
                    });
            }
            x = rect.right() + 2.0;
        }
        x
    }

    fn has_children(&self, path: &Path) -> bool {
        path != self.folder
            || self
                .cache
                .iter()
                .find(|listing| listing.path == path)
                .map_or_else(
                    || self.prefetch.children(path) == Some(true),
                    |listing| !listing.folders.is_empty(),
                )
    }

    fn background_ready(&mut self, ui: &Ui) -> bool {
        let (time, pointer, moving, pressed) = ui.input(|input| {
            (
                input.time,
                input.pointer.hover_pos(),
                input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::PointerMoved(_))),
                input.pointer.any_down() || input.pointer.any_pressed(),
            )
        });
        if pressed || (moving && pointer != self.prepare_pointer) {
            self.prepare_after = Some(time + 0.15);
        }
        self.prepare_pointer = pointer;
        if let Some(deadline) = self.prepare_after
            && time < deadline
        {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(deadline - time));
            return false;
        }
        self.prepare_after = None;
        true
    }

    fn listing(&mut self, ui: &Ui, path: &Path, start_next: bool) -> Option<Arc<FolderListing>> {
        if let Some(result) = self.worker.as_ref().and_then(FolderBrowser::result)
            && self.prefetch.complete(&result)
        {
            self.cache.retain(|saved| saved.path != result.path);
            if self.cache.len() == 64 {
                self.cache.remove(0);
            }
            self.cache.push(result);
        }
        let result = self
            .cache
            .iter()
            .position(|result| result.path == path)
            .map(|index| {
                let result = self.cache.remove(index);
                self.cache.push(result.clone());
                result
            });
        // Retain completed work during pointer interaction, but defer the next
        // hidden Shell view. An explicitly opened menu bypasses this idle delay.
        if !start_next {
            return result;
        }
        if result.is_none() {
            self.prefetch.prioritize(path);
        }
        let Some(request) = self.prefetch.request().map(Path::to_owned) else {
            return result;
        };
        if self.worker.is_none() && !self.worker_failed {
            let context = ui.ctx().clone();
            match FolderBrowser::new(move || context.request_repaint()) {
                Ok(worker) => self.worker = Some(worker),
                Err(_) => self.worker_failed = true,
            }
        }
        if let Some(worker) = &mut self.worker {
            worker.request(Some(request));
        }
        result
    }

    fn directory_menu(
        &mut self,
        ui: &mut Ui,
        path: &Path,
        depth: usize,
        target: &mut Option<Action>,
    ) {
        let language = localization::language(ui.ctx());
        let Some(listing) = self.listing(ui, path, true) else {
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
            ui.close();
            return;
        }
        // Publish each menu atomically: arrows never appear after its rows.
        if listing
            .folders
            .iter()
            .any(|entry| entry.has_children.is_none())
        {
            menu_message(ui, Text::FilmstripLoadingFolders.in_language(language));
            return;
        }
        let entries: Vec<_> = listing
            .folders
            .iter()
            .map(|entry| {
                let children = if self.prefetch.children(&entry.path) == Some(false) {
                    Some(false)
                } else {
                    entry.has_children
                };
                (entry.path.as_path(), children)
            })
            .collect();
        self.folder_rows(ui, &entries, depth, target);
        if listing.truncated {
            ui.label(Text::FilmstripFolderLimit.in_language(language));
        }
    }

    fn folder_rows(
        &mut self,
        ui: &mut Ui,
        entries: &[(&Path, Option<bool>)],
        depth: usize,
        target: &mut Option<Action>,
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
                    + if *children == Some(true) && depth < 63 {
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
            .show_rows_popup_styled(
                ui,
                f32::from(ui.spacing().menu_margin.right),
                chrome::SEARCH_BUTTON_SIZE,
                entries.len(),
                |ui, range| {
                    let rows = chrome::MenuRows::new(ui);
                    let mut ids = Vec::new();
                    for index in range {
                        let (path, presence) = entries[index];
                        let children = presence == Some(true);
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
                                    arrow.hovered()
                                        || arrow.has_focus()
                                        || (open && !response.hovered() && !response.has_focus()),
                                    ui.visuals().widgets.hovered.weak_bg_fill,
                                ),
                            );
                            response
                        } else {
                            ui.add(egui::Button::new(label).right_text(()).gap(0.0).truncate())
                        };
                        if !children || depth >= 63 {
                            ids.push(response.id);
                        }
                        if response.hovered() && presence.is_none() {
                            self.listing(ui, path, true);
                        }
                        if response.clicked() || response.middle_clicked() {
                            *target =
                                Some(Action::Folder(path.to_owned(), response.middle_clicked()));
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
        nw: if left && !right { chrome::RADIUS_XS } else { 0 },
        sw: if left && !right { chrome::RADIUS_XS } else { 0 },
        ne: if right || !has_arrow {
            chrome::RADIUS_XS
        } else {
            0
        },
        se: if right || !has_arrow {
            chrome::RADIUS_XS
        } else {
            0
        },
    };
    egui::Shape::Vec(vec![
        egui::Shape::rect_filled(rect, chrome::RADIUS_XS, chrome::SURFACE),
        egui::Shape::rect_filled(half, radius, color),
    ])
}
