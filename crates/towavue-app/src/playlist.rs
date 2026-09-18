use crate::scroll_style::ScrollAreaStyle;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use egui::RichText;
use towavue_core::{FolderSnapshot, MediaKind};

use crate::hover_help::HoverHelp;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DurationRequest {
    snapshot: (PathBuf, u64, SystemTime),
    pub path: PathBuf,
}

#[derive(Default)]
pub struct Playlist {
    focus: Option<(PathBuf, usize)>,
    keyboard_focus: Option<(PathBuf, egui::Id)>,
    wheel: crate::wheel_input::Scroll,
    scroll_offset: f32,
    pub scroll_rect: Option<egui::Rect>,
    duration_snapshot: Option<(PathBuf, u64, SystemTime)>,
    durations: BTreeMap<PathBuf, Option<Duration>>,
    visible: Vec<PathBuf>,
}

impl Playlist {
    pub(super) fn detach_context(&mut self) {
        self.suspend();
        self.keyboard_focus = None;
    }

    pub fn suspend(&mut self) {
        self.wheel.clear();
    }

    pub fn clear(&mut self) {
        self.focus = None;
        self.keyboard_focus = None;
        self.wheel.clear();
        self.scroll_offset = 0.0;
        self.duration_snapshot = None;
        self.durations.clear();
        self.visible.clear();
        self.scroll_rect = None;
    }

    pub fn duration_request(&self) -> Option<DurationRequest> {
        Some(DurationRequest {
            snapshot: self.duration_snapshot.clone()?,
            path: self
                .visible
                .iter()
                .find(|path| !self.durations.contains_key(*path))?
                .clone(),
        })
    }

    pub fn finish_duration(&mut self, request: DurationRequest, duration: Option<Duration>) {
        if self.duration_snapshot.as_ref() != Some(&request.snapshot) {
            return;
        }
        if self.durations.len() >= 256.max(self.visible.len()) {
            let Some(old) = self
                .durations
                .keys()
                .find(|path| !self.visible.contains(path))
                .cloned()
            else {
                return;
            };
            self.durations.remove(&old);
        }
        self.durations.insert(request.path, duration);
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: Option<&FolderSnapshot>,
        current: Option<&Path>,
        allow_wheel: bool,
    ) -> Option<PathBuf> {
        let mut chosen = None;
        self.scroll_rect = None;
        self.visible.clear();
        let key = snapshot.map(|snapshot| {
            (
                snapshot.folder_path.clone(),
                snapshot.generation,
                snapshot.captured_at,
            )
        });
        if self.duration_snapshot != key {
            self.durations.clear();
            self.duration_snapshot = key;
        }
        crate::wheel_input::begin_frame(ui.ctx());
        let items: Vec<_> = snapshot
            .into_iter()
            .flat_map(|snapshot| snapshot.items_of_kind(MediaKind::Audio))
            .collect();
        let margin = egui::Margin {
            left: 8,
            right: 8,
            top: 8,
            bottom: 0,
        };
        egui::Frame::new().inner_margin(margin).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            // The scrollbar is part of the list's wheel ownership too.
            self.scroll_rect = Some(ui.available_rect_before_wrap().intersect(ui.clip_rect()));
            let mut reveal = None;
            if ui.is_enabled()
                && !egui::Popup::is_any_open(ui.ctx())
                && let Some((path, id)) = &self.keyboard_focus
                && ui.memory(|memory| memory.has_focus(*id))
                && let Some(mut index) = items.iter().position(|item| &item.path == path)
            {
                let page = (ui.available_height() / 32.0).floor().max(1.0) as usize;
                ui.input_mut(|input| {
                    input.events.retain(|event| {
                        let egui::Event::Key {
                            key,
                            pressed: true,
                            modifiers,
                            ..
                        } = event
                        else {
                            return true;
                        };
                        if *modifiers != egui::Modifiers::NONE {
                            return true;
                        }
                        index = match key {
                            egui::Key::ArrowUp => index.saturating_sub(1),
                            egui::Key::ArrowDown => (index + 1).min(items.len() - 1),
                            egui::Key::Home => 0,
                            egui::Key::End => items.len() - 1,
                            egui::Key::PageUp => index.saturating_sub(page),
                            egui::Key::PageDown => (index + page).min(items.len() - 1),
                            egui::Key::Enter | egui::Key::Space => {
                                chosen = Some(items[index].path.clone());
                                return false;
                            }
                            _ => return true,
                        };
                        reveal = Some(index);
                        false
                    });
                });
                if reveal.is_some() {
                    ui.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
                }
            }
            let focus = current.and_then(|path| {
                items
                    .iter()
                    .position(|item| item.path == path)
                    .map(|index| (path, index))
            });
            let changed = self
                .focus
                .as_ref()
                .map(|(path, index)| (path.as_path(), *index))
                != focus;
            let mut scroll = egui::ScrollArea::vertical()
                .id_salt("audio_playlist")
                .vertical_scroll_offset(self.scroll_offset)
                .scroll_source(egui::scroll_area::ScrollSource {
                    mouse_wheel: false,
                    ..Default::default()
                })
                .auto_shrink([false, false]);
            if changed || reveal.is_some() {
                self.wheel.clear();
                if let Some(index) = reveal.or_else(|| focus.map(|(_, index)| index)) {
                    let offset = self.scroll_offset;
                    let top = index as f32 * 32.0;
                    let height = ui.available_height();
                    let bottom = top + 32.0 + if index + 1 == items.len() { 8.0 } else { 0.0 };
                    let offset = if top < offset {
                        top
                    } else if bottom > offset + height {
                        (bottom - height).max(0.0)
                    } else {
                        offset
                    };
                    scroll = scroll.vertical_scroll_offset(offset);
                    self.focus = focus.map(|(path, index)| (path.to_path_buf(), index));
                } else {
                    self.clear();
                }
            } else {
                let delta = self.wheel.delta(
                    ui,
                    ui.available_rect_before_wrap().intersect(ui.clip_rect()),
                    allow_wheel,
                );
                if delta.y != 0.0 {
                    let offset = self.scroll_offset;
                    scroll = scroll.vertical_scroll_offset((offset - delta.y).max(0.0));
                }
            }
            let output = scroll.show_rows_styled(ui, 32.0, items.len(), |ui, rows| {
                // The virtual child begins at rows.start. Extend its minimum to the
                // list end plus padding, keeping the scroll viewport at the media edge.
                if !items.is_empty() {
                    ui.set_min_height((items.len() - rows.start) as f32 * 32.0 + 8.0);
                }
                for index in rows {
                    let item = items[index];
                    self.visible.push(item.path.clone());
                    let selected = current == Some(item.path.as_path());
                    let name = crate::display_name(&item.path);
                    let text = RichText::new(format!("{}. {name}", index + 1)).size(14.0);
                    let duration = self
                        .durations
                        .get(&item.path)
                        .copied()
                        .flatten()
                        .map(|duration| crate::format_time(crate::media_time(duration)))
                        .unwrap_or_else(|| "—".into());
                    let duration = RichText::new(duration).size(14.0);
                    let response = ui
                        .scope_builder(
                            egui::UiBuilder::new().id(ui.id().with(("audio-row", &item.path))),
                            |ui| {
                                crate::chrome::flat_buttons(ui);
                                if selected {
                                    ui.visuals_mut().widgets.inactive.fg_stroke.color =
                                        egui::Color32::WHITE;
                                }
                                let background = ui.painter().add(egui::Shape::Noop);
                                let response = ui.add_sized(
                                    egui::vec2(ui.available_width(), 32.0),
                                    egui::Button::selectable(
                                        false,
                                        (text, egui::Atom::grow(), duration),
                                    )
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE)
                                    .truncate(),
                                );
                                if response.hovered() {
                                    ui.painter().set(
                                        background,
                                        egui::Shape::rect_filled(
                                            response.rect,
                                            3,
                                            crate::chrome::HOVER,
                                        ),
                                    );
                                }
                                response
                            },
                        )
                        .inner
                        .help_ui(|ui| {
                            ui.set_max_width(
                                (ui.ctx().viewport_rect().width() - 32.0).clamp(1.0, 400.0),
                            );
                            ui.add(egui::Label::new(&name).wrap());
                        });
                    if reveal == Some(index) {
                        response.request_focus();
                    }
                    crate::tab_focus::observe_pointer_control(
                        &response,
                        ("playlist-row", &item.path),
                    );
                    if response.has_focus() {
                        self.keyboard_focus = Some((item.path.clone(), response.id));
                    }
                    ui.ctx().accesskit_node_builder(response.id, |node| {
                        node.clear_toggled();
                        node.set_label(format!("{}. {name}", index + 1));
                        node.set_description(format!(
                            "{}{}{}",
                            item.path.display(),
                            if selected { " (current track)" } else { "" },
                            self.durations
                                .get(&item.path)
                                .copied()
                                .flatten()
                                .map(|duration| format!(
                                    " · Duration {}",
                                    crate::format_time(crate::media_time(duration))
                                ))
                                .unwrap_or_default()
                        ));
                    });
                    if response.clicked() {
                        chosen = Some(item.path.clone());
                    }
                }
            });
            // The vertical bar is a separate egui 0.35 widget from the playlist rows.
            if let Some(response) = ui.ctx().read_response(output.id.with(1_usize))
                && response.enabled()
            {
                crate::tab_focus::observe_pointer_control(&response, "playlist-scrollbar");
            }
            self.scroll_offset = output.state.offset.y;
        });
        chosen
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use egui::{Event, Pos2, Rect, Shape, Vec2};
    use towavue_core::{FolderMediaItem, FolderSnapshotSource, ShellIdentity};

    use super::*;

    #[test]
    fn pointer_rows_and_scrollbar_return_shortcuts_to_media_without_removing_explicit_focus() {
        for (density, scrollbar) in [1.0, 1.25, 2.0]
            .into_iter()
            .flat_map(|density| [false, true].map(|scrollbar| (density, scrollbar)))
        {
            for (batched, focused) in [(false, false), (false, true), (true, false), (true, true)] {
                let context = crate::fonts::test_context();
                context.enable_accesskit();
                let mut tabs = towavue_core::TabSet::default();
                let active = tabs.open_new("active.wav".into(), MediaKind::Audio);
                let other = tabs.open_new("other.wav".into(), MediaKind::Audio);
                let snapshot = snapshot(if scrollbar { 100 } else { 3 });
                let current = &snapshot.items[0].path;
                let mut playlist = Playlist::default();
                let mut frame = |tab, events| {
                    let mut chosen = None;
                    let mut raw = egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(480.0, 240.0))),
                        events,
                        ..Default::default()
                    };
                    raw.viewports
                        .get_mut(&egui::ViewportId::ROOT)
                        .expect("viewport")
                        .native_pixels_per_point = Some(density);
                    let output = context.run_ui(raw, |ui| {
                        crate::tab_focus::begin(&context, Some(tab), true);
                        chosen = playlist.show(ui, Some(&snapshot), Some(current), true);
                        crate::tab_focus::finish(&context, false, true);
                    });
                    (
                        output.platform_output.accesskit_update.expect("tree"),
                        chosen,
                    )
                };
                frame(active, vec![]);
                let tree = frame(active, vec![]).0;
                let (id, row) = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some("1. track-0.wav"))
                    .expect("row");
                let bounds = if scrollbar {
                    tree.nodes
                        .iter()
                        .find(|(_, node)| node.role() == egui::accesskit::Role::ScrollBar)
                        .expect("scrollbar")
                        .1
                        .bounds()
                        .expect("scrollbar bounds")
                } else {
                    row.bounds().expect("row bounds")
                };
                let point = egui::pos2(
                    ((bounds.x0 + bounds.x1) * 0.5) as f32,
                    if scrollbar {
                        bounds.y0 as f32 + 3.0
                    } else {
                        ((bounds.y0 + bounds.y1) * 0.5) as f32
                    },
                );
                let focus = || {
                    Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                        action: egui::accesskit::Action::Focus,
                        target_tree: egui::accesskit::TreeId::ROOT,
                        target_node: *id,
                        data: None,
                    })
                };
                if focused {
                    frame(active, vec![focus()]);
                    assert_eq!(frame(active, vec![]).0.focus, *id);
                    assert_eq!(
                        frame(active, vec![Event::PointerMoved(point)]).0.focus,
                        *id,
                        "hover must preserve explicit row focus"
                    );
                }
                let pointer = |pressed| Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                let mut press = vec![Event::PointerMoved(point), pointer(true)];
                if batched {
                    press.push(pointer(false));
                } else {
                    assert!(frame(active, press).1.is_none());
                    assert_eq!(
                        context.memory(|memory| memory.focused()),
                        None,
                        "pointer press releases focus: scrollbar={scrollbar}, {density}x"
                    );
                    press = vec![pointer(false)];
                }
                assert_eq!(
                    frame(active, press).1.as_ref(),
                    (!scrollbar).then_some(current),
                    "only a row click chooses the track"
                );
                let key = |key| Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                };
                assert!(
                    frame(active, vec![key(egui::Key::Space)]).1.is_none(),
                    "Space after a row click must not reopen the track"
                );
                assert!(
                    context.input(|input| input.key_pressed(egui::Key::Space)),
                    "leave Space for media playback"
                );
                assert!(!context.egui_wants_keyboard_input());
                assert_eq!(context.memory(|memory| memory.focused()), None);
                frame(other, vec![]);
                frame(active, vec![]);
                frame(active, vec![]);
                assert_eq!(
                    context.memory(|memory| memory.focused()),
                    None,
                    "pointer focus must not return with the tab"
                );
                frame(active, vec![focus()]);
                frame(active, vec![]);
                assert_eq!(
                    frame(
                        active,
                        vec![key(egui::Key::ArrowDown), key(egui::Key::Enter)]
                    )
                    .1,
                    Some(snapshot.items[1].path.clone()),
                    "explicit row navigation remains available"
                );
            }
        }
    }

    #[test]
    fn durations_are_visible_only_refresh_safe_and_right_aligned_without_current_fill() {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.global_style_mut(crate::chrome::style);
            context.set_pixels_per_point(density);
            let mut playlist = Playlist::default();
            let mut snapshot = snapshot(10_000);
            snapshot.items[0].path = PathBuf::from(format!("{}.wav", "long-".repeat(30)));
            let mut time = 0.0;
            let mut frame = |playlist: &mut Playlist,
                             snapshot: &FolderSnapshot,
                             pointer: Option<Pos2>| {
                time += 0.1;
                context.run_ui(
                    egui::RawInput {
                        time: Some(time),
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 240.0))),
                        events: pointer.map(Event::PointerMoved).into_iter().collect(),
                        ..Default::default()
                    },
                    |ui| {
                        playlist.show(ui, Some(snapshot), Some(&snapshot.items[0].path), true);
                    },
                )
            };
            frame(&mut playlist, &snapshot, None);
            frame(&mut playlist, &snapshot, None);
            let first = playlist.duration_request().expect("first visible duration");
            assert_eq!(first.path, snapshot.items[0].path);
            playlist.finish_duration(first.clone(), Some(Duration::from_secs(161)));
            let second = playlist.duration_request().expect("next visible duration");
            assert_eq!(second.path, snapshot.items[1].path);
            playlist.finish_duration(second, None);
            assert_eq!(
                playlist.duration_request().expect("skip failed row").path,
                snapshot.items[2].path
            );
            assert!(
                playlist.visible.len() < 12,
                "do not probe the entire folder"
            );
            let output = frame(&mut playlist, &snapshot, None);
            let duration = texts(&output)
                .into_iter()
                .find(|text| text.galley.text() == "02:41")
                .expect("duration caption");
            assert!(duration.pos.x > 260.0 && duration.pos.x + duration.galley.size().x <= 312.0);
            assert_eq!(duration.fallback_color, egui::Color32::WHITE);
            let row =
                Rect::from_min_size(Pos2::new(8.0, duration.pos.y - 5.0), Vec2::new(300.0, 32.0));
            let backgrounds = |output: &egui::FullOutput| {
                output.shapes.iter().filter(|shape| matches!(&shape.shape, Shape::Rect(rect) if rect.fill == crate::chrome::HOVER && rect.rect.intersects(row))).count()
            };
            assert_eq!(backgrounds(&output), 0, "current row is text-only");
            frame(&mut playlist, &snapshot, Some(row.center()));
            assert!(
                backgrounds(&frame(&mut playlist, &snapshot, Some(row.center()))) > 0,
                "current row still has hover feedback"
            );
            snapshot.generation += 1;
            frame(&mut playlist, &snapshot, None);
            playlist.finish_duration(first, Some(Duration::from_secs(99)));
            assert!(
                playlist.durations.is_empty(),
                "reject stale snapshot results"
            );
            assert_eq!(
                playlist.duration_request().expect("refresh retry").path,
                snapshot.items[0].path
            );
            let request = playlist.duration_request().expect("visible request");
            playlist.finish_duration(request.clone(), Some(Duration::from_secs(161)));
            for index in 0..300 {
                let mut offscreen = request.clone();
                offscreen.path = PathBuf::from(format!("offscreen-{index}.wav"));
                playlist.finish_duration(offscreen, None);
            }
            assert_eq!(playlist.durations.len(), 256);
            assert_eq!(
                playlist.durations[&request.path],
                Some(Duration::from_secs(161))
            );
        }
    }

    #[test]
    fn keyboard_focus_reaches_virtualized_rows_without_selecting_them() {
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut playlist = Playlist::default();
        let snapshot = snapshot(10_000);
        let current = &snapshot.items[0].path;
        let mut frame = |events, disabled| {
            let mut chosen = None;
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(480.0, 240.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    if disabled {
                        ui.disable();
                    }
                    chosen = playlist.show(ui, Some(&snapshot), Some(current), true);
                },
            );
            (
                output.platform_output.accesskit_update.expect("tree"),
                chosen,
            )
        };
        let key = |key| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![], false);
        let (tree, _) = frame(vec![], false);
        let first = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("1. track-0.wav"))
            .expect("first row")
            .0;
        frame(
            vec![Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Focus,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: first,
                    data: None,
                },
            )],
            false,
        );
        for (key_code, expected) in [
            (egui::Key::End, "10000. track-9999.wav"),
            (egui::Key::ArrowUp, "9999. track-9998.wav"),
            (egui::Key::Home, "1. track-0.wav"),
            (egui::Key::PageDown, "8. track-7.wav"),
            (egui::Key::PageUp, "1. track-0.wav"),
        ] {
            let (tree, chosen) = frame(vec![key(key_code)], false);
            assert!(chosen.is_none(), "focus navigation must not select a track");
            let focused = tree
                .nodes
                .iter()
                .find(|(id, _)| *id == tree.focus)
                .expect("visible focus");
            assert_eq!(focused.1.label(), Some(expected));
            assert!(
                tree.nodes
                    .iter()
                    .filter(|(_, node)| node.role() == egui::accesskit::Role::Button)
                    .count()
                    < 12,
                "keep offscreen rows virtualized"
            );
        }
        for index in 1..20 {
            let (tree, chosen) = frame(vec![key(egui::Key::ArrowDown)], false);
            assert!(chosen.is_none());
            assert_eq!(
                tree.nodes
                    .iter()
                    .find(|(id, _)| *id == tree.focus)
                    .expect("row focus")
                    .1
                    .label(),
                Some(format!("{}. track-{index}.wav", index + 1).as_str())
            );
        }
        assert_eq!(
            frame(vec![key(egui::Key::Enter)], false).1,
            Some(snapshot.items[19].path.clone())
        );
        assert_eq!(
            frame(vec![key(egui::Key::End), key(egui::Key::Enter)], false).1,
            Some(snapshot.items[9999].path.clone()),
            "a batched Enter must activate the new focus"
        );
        assert_eq!(
            frame(vec![key(egui::Key::Space), key(egui::Key::Home)], false).1,
            Some(snapshot.items[9999].path.clone()),
            "activation before navigation retains its original target"
        );
        frame(vec![], true);
        assert!(
            frame(vec![key(egui::Key::End), key(egui::Key::Enter)], true)
                .1
                .is_none()
        );
    }

    #[test]
    fn accessible_rows_follow_paths_after_shell_updates() {
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut playlist = Playlist::default();
        let mut snapshot = snapshot(3);
        let current = snapshot.items[2].path.clone();
        let target = snapshot.items[1].path.clone();
        let mut frame = |snapshot: &FolderSnapshot, events, disabled| {
            let mut chosen = None;
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(480.0, 240.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    if disabled {
                        ui.disable();
                    }
                    chosen = playlist.show(ui, Some(snapshot), Some(&current), true);
                },
            );
            (
                output.platform_output.accesskit_update.expect("tree"),
                chosen,
            )
        };
        frame(&snapshot, vec![], false);
        let (tree, _) = frame(&snapshot, vec![], false);
        let id = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("2. track-1.wav"))
            .expect("named row")
            .0;
        let click = || {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Click,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: id,
                data: None,
            })
        };
        snapshot.items.swap(0, 1);
        frame(&snapshot, vec![], false);
        assert_eq!(
            frame(&snapshot, vec![click()], false).1,
            Some(target.clone())
        );
        let (tree, _) = frame(&snapshot, vec![], false);
        let node = &tree
            .nodes
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .expect("same row")
            .1;
        assert_eq!(node.label(), Some("1. track-1.wav"));
        assert!(
            node.toggled().is_none(),
            "opening a track is not an on/off toggle"
        );
        assert_eq!(node.description(), Some(target.to_string_lossy().as_ref()));
        frame(&snapshot, vec![], true);
        assert!(frame(&snapshot, vec![click()], true).1.is_none());
        snapshot.items.remove(0);
        frame(&snapshot, vec![], false);
        assert!(frame(&snapshot, vec![click()], false).1.is_none());
        snapshot.items[0].path = PathBuf::from("another-folder/track-1.wav");
        frame(&snapshot, vec![], false);
        assert!(frame(&snapshot, vec![click()], false).1.is_none());
    }

    #[test]
    fn playlist_reaches_the_media_bottom_at_each_density_and_scroll_position() {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.global_style_mut(crate::chrome::style);
            context.enable_accesskit();
            context.set_pixels_per_point(density);
            let snapshot = snapshot(100);
            for index in [0, 99] {
                let mut playlist = Playlist::default();
                let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(480.0, 300.0));
                let media = Rect::from_min_max(Pos2::new(0.0, 32.0), Pos2::new(480.0, 276.0));
                for pass in 0..3 {
                    let output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            ..Default::default()
                        },
                        |ui| {
                            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(media));
                            child.set_clip_rect(media);
                            assert!(
                                playlist
                                    .show(
                                        &mut child,
                                        Some(&snapshot),
                                        Some(&snapshot.items[index].path),
                                        true
                                    )
                                    .is_none()
                            );
                        },
                    );
                    let scroll = playlist.scroll_rect.expect("list viewport");
                    assert!(
                        (scroll.bottom() - media.bottom()).abs() <= 1.0 / density,
                        "list reaches the status boundary"
                    );
                    if pass == 2 {
                        let tree = output
                            .platform_output
                            .accesskit_update
                            .as_ref()
                            .expect("tree");
                        let label = format!("{}. track-{index}.wav", index + 1);
                        let bounds = tree
                            .nodes
                            .iter()
                            .find(|(_, node)| node.label() == Some(label.as_str()))
                            .expect("edge row")
                            .1
                            .bounds()
                            .expect("row bounds");
                        let gap = if index == 0 {
                            bounds.y0 as f32 - media.top()
                        } else {
                            media.bottom() - bounds.y1 as f32
                        };
                        assert!(
                            (gap - 8.0).abs() <= 1.0 / density,
                            "matching edge padding: {index}, {density}, {gap}"
                        );
                        assert!(
                            playlist.visible.len() < 12,
                            "padding keeps rows virtualized"
                        );
                    }
                    assert_eq!(scroll.left(), media.left() + 8.0);
                    assert_eq!(scroll.right(), media.right() - 8.0);
                    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(_) if (shape.clip_rect.bottom() - media.bottom()).abs() <= 1.0 / density)), "rows paint to the media bottom");
                }
            }
        }
    }

    #[test]
    fn playlist_row_hover_is_borderless_and_both_captions_stay_centered() {
        for density in [1.0, 1.25, 2.0] {
            for width in [240.0, 960.0] {
                let context = crate::fonts::test_context();
                context.enable_accesskit();
                context.set_pixels_per_point(density);
                context.global_style_mut(|style| {
                    crate::chrome::style(style);
                    style.animation_time = 0.0;
                    style.interaction.tooltip_delay = 60.0;
                });
                let snapshot = snapshot(2);
                let mut playlist = Playlist::default();
                let frame = |playlist: &mut Playlist, events| {
                    context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                Pos2::ZERO,
                                Vec2::new(width, 240.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ui| {
                            assert!(
                                playlist
                                    .show(ui, Some(&snapshot), Some(&snapshot.items[0].path), true)
                                    .is_none()
                            );
                        },
                    )
                };
                frame(&mut playlist, vec![]);
                for _ in 0..2 {
                    let request = playlist.duration_request().expect("row duration");
                    playlist.finish_duration(request, Some(Duration::from_secs(123)));
                }
                for index in 0..2 {
                    let output = frame(&mut playlist, vec![Event::PointerGone]);
                    let label = format!("{}. track-{index}.wav", index + 1);
                    let bounds = output
                        .platform_output
                        .accesskit_update
                        .as_ref()
                        .expect("tree")
                        .nodes
                        .iter()
                        .find(|(_, node)| node.label() == Some(label.as_str()))
                        .expect("row")
                        .1
                        .bounds()
                        .expect("bounds");
                    let row = Rect::from_min_max(
                        Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
                        Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
                    );
                    let original_positions: Vec<_> = texts(&output)
                        .into_iter()
                        .filter(|text| row.contains(text.pos))
                        .map(|text| (text.galley.text().to_owned(), text.pos))
                        .collect();
                    for held in [false, true] {
                        let mut events = vec![Event::PointerMoved(row.center())];
                        if held {
                            events.push(Event::PointerButton {
                                pos: row.center(),
                                button: egui::PointerButton::Primary,
                                pressed: true,
                                modifiers: egui::Modifiers::NONE,
                            });
                        }
                        frame(&mut playlist, events);
                        let output = frame(&mut playlist, vec![]);
                        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                            Shape::Rect(rect) if rect.rect == row && rect.fill == crate::chrome::HOVER)),
                            "hover bounds: density={density}, width={width}, index={index}, held={held}, row={row:?}, fills={:?}",
                            output.shapes.iter().filter_map(|shape| match &shape.shape {
                                Shape::Rect(rect) if rect.fill == crate::chrome::HOVER => Some(rect.rect), _ => None
                            }).collect::<Vec<_>>());
                        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
                            Shape::Rect(rect) if rect.rect.intersects(row.shrink(2.0)) && rect.stroke.width > 0.0)),
                            "hover and held backgrounds have no border");
                        let captions: Vec<_> = texts(&output)
                            .into_iter()
                            .filter(|text| {
                                row.contains(text.pos)
                                    && (text.galley.text() == label
                                        || text.galley.text() == "02:03")
                            })
                            .collect();
                        assert_eq!(captions.len(), 2);
                        for text in captions {
                            let original = original_positions
                                .iter()
                                .find(|(label, _)| label == text.galley.text())
                                .expect("original caption")
                                .1;
                            assert_eq!(text.pos, original, "hover/press must not shift text");
                            assert!(
                                (text.pos.y + text.galley.size().y * 0.5 - row.center().y).abs()
                                    <= 1.0 / density,
                                "centered filename and duration at {density}"
                            );
                        }
                    }
                    // Release outside every row so the next trial starts with no held button.
                    let outside = Pos2::new(width + 10.0, 250.0);
                    frame(
                        &mut playlist,
                        vec![
                            Event::PointerMoved(outside),
                            Event::PointerButton {
                                pos: outside,
                                button: egui::PointerButton::Primary,
                                pressed: false,
                                modifiers: egui::Modifiers::NONE,
                            },
                            Event::PointerGone,
                        ],
                    );
                }
            }
        }
    }

    #[test]
    fn playlist_rows_preserve_order_and_offer_full_width_targets() {
        for width in [240.0, 960.0] {
            let context = egui::Context::default();
            context.enable_accesskit();
            context.global_style_mut(|style| {
                crate::chrome::style(style);
                style.animation_time = 0.0;
                style.interaction.tooltip_delay = 0.0;
            });
            let mut snapshot = snapshot(10_000);
            snapshot.items[0].path = PathBuf::from("z-first.wav");
            snapshot.items[1].path = PathBuf::from("a-current.wav");
            snapshot.items[2].path = PathBuf::from(format!("{}.wav", "long-name-".repeat(40)));
            snapshot.items.insert(
                1,
                FolderMediaItem {
                    identity: ShellIdentity::new(vec![255]),
                    path: PathBuf::from("skip.png"),
                    kind: MediaKind::Image,
                },
            );
            let current = snapshot.items[2].path.as_path();
            let time = std::cell::Cell::new(0.0);
            let mut playlist = Playlist::default();
            let mut frame = |events| {
                time.set(time.get() + 0.2);
                let mut chosen = None;
                let output = context.run_ui(
                    egui::RawInput {
                        time: Some(time.get()),
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 240.0))),
                        events,
                        ..Default::default()
                    },
                    |ui| chosen = playlist.show(ui, Some(&snapshot), Some(current), true),
                );
                (output, chosen)
            };
            for _ in 0..4 {
                frame(vec![]);
            }
            let (output, chosen) = frame(vec![]);
            assert!(chosen.is_none());
            let texts = texts(&output);
            assert!(texts.len() < 12, "only visible rows should be laid out");
            assert_eq!(texts[0].galley.job.text, "1. z-first.wav");
            assert_eq!(texts[1].galley.job.text, "2. a-current.wav");
            assert_eq!(texts[1].pos.y - texts[0].pos.y, 32.0);
            assert_eq!(texts[0].fallback_color, crate::chrome::MUTED);
            assert_eq!(texts[1].fallback_color, crate::chrome::FOREGROUND);
            assert_eq!(texts[2].galley.rows.len(), 1);
            assert!(texts[2].galley.elided);
            for text in &texts {
                assert!(text.pos.x < 30.0, "left aligned");
                assert!(text.pos.x + text.galley.size().x <= width - 8.0);
            }
            let first_row = output
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("playlist accessibility tree")
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some("1. z-first.wav"))
                .expect("first row accessibility node")
                .0;
            let hover = egui::pos2(width - 32.0, texts[0].pos.y + 7.0);
            frame(vec![Event::PointerMoved(hover)]);
            let (hovered, chosen) = frame(vec![]);
            assert!(chosen.is_none());
            assert_eq!(
                self::texts(&hovered)[0].fallback_color,
                crate::chrome::FOREGROUND
            );
            frame(vec![
                Event::PointerGone,
                Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Focus,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: first_row,
                    data: None,
                }),
            ]);
            let (focused, chosen) = frame(vec![]);
            assert!(chosen.is_none(), "focus does not select another track");
            assert_eq!(
                focused
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("focused playlist accessibility tree")
                    .focus,
                first_row
            );
            assert_eq!(
                self::texts(&focused)[0].fallback_color,
                crate::chrome::FOREGROUND
            );
            assert!(
                !focused.shapes.iter().any(|shape| matches!(
                    &shape.shape, Shape::Rect(rect) if rect.fill == crate::chrome::HOVER
                )),
                "keyboard focus keeps white text without a hover background"
            );
            let pos = egui::pos2(width - 32.0, texts[1].pos.y + 7.0);
            frame(vec![Event::PointerMoved(pos)]);
            let mut actions = Vec::new();
            for pressed in [true, false] {
                actions.extend(
                    frame(vec![Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    }])
                    .1,
                );
            }
            assert_eq!(actions, [current.to_path_buf()]);
            let pos = egui::pos2(width - 32.0, texts[2].pos.y + 7.0);
            frame(vec![Event::PointerMoved(pos)]);
            let mut output = frame(vec![]).0;
            for _ in 0..5 {
                output = frame(vec![]).0;
            }
            let tooltip = self::texts(&output)
                .into_iter()
                .find(|text| text.galley.job.text == crate::display_name(&snapshot.items[3].path))
                .expect("tooltip contains the entire filename");
            assert!(tooltip.pos.x >= 0.0);
            assert!(tooltip.pos.x + tooltip.galley.size().x <= width);
            frame(vec![Event::PointerGone]);
            frame(vec![Event::PointerMoved(egui::pos2(width / 2.0, 30.0))]);
            frame(vec![Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -1600.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            }]);
            for _ in 0..40 {
                frame(vec![]);
            }
            let output = frame(vec![Event::PointerGone]).0;
            let texts = self::texts(&output);
            assert!(texts.len() < 12);
            let row = texts
                .iter()
                .find(|text| text.pos.y > 20.0 && text.pos.y < 180.0)
                .expect("scrolled row");
            let number: usize = row
                .galley
                .job
                .text
                .split('.')
                .next()
                .expect("row number prefix")
                .parse()
                .expect("numeric row prefix");
            assert!(number > 40, "scroll reaches later audio rows");
            let pos = egui::pos2(width - 32.0, row.pos.y + 7.0);
            frame(vec![Event::PointerMoved(pos)]);
            for pressed in [true, false] {
                if let Some(path) = frame(vec![Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }])
                .1
                {
                    assert_eq!(
                        path,
                        snapshot
                            .items_of_kind(MediaKind::Audio)
                            .nth(number - 1)
                            .expect("clicked audio item")
                            .path
                    );
                    actions.push(path);
                }
            }
            assert_eq!(actions.len(), 2);
        }
    }

    #[test]
    fn playlist_reveals_changed_current_row_without_overriding_manual_scroll() {
        let context = egui::Context::default();
        let mut playlist = Playlist::default();
        let mut snapshot = snapshot(10_000);
        let last = snapshot.items[9999].path.clone();
        let frame =
            |playlist: &mut Playlist, snapshot: Option<&FolderSnapshot>, current: &Path, events| {
                context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(480.0, 240.0))),
                        events,
                        ..Default::default()
                    },
                    |ui| assert!(playlist.show(ui, snapshot, Some(current), true).is_none()),
                )
            };
        let visible = |output: &egui::FullOutput, name: &str| {
            texts(output).iter().any(|text| {
                text.galley.job.text.ends_with(name)
                    && text.pos.y >= 8.0
                    && text.pos.y + text.galley.size().y <= 232.0
            })
        };
        frame(&mut playlist, None, &last, vec![]);
        assert!(
            playlist.focus.is_none(),
            "wait for the snapshot before consuming reveal"
        );
        for _ in 0..4 {
            frame(&mut playlist, Some(&snapshot), &last, vec![]);
        }
        let output = frame(&mut playlist, Some(&snapshot), &last, vec![]);
        assert!(
            visible(&output, "track-9999.wav"),
            "initial late-file open must reveal the current row"
        );
        assert!(texts(&output).len() < 12);

        let scroll_away = |playlist: &mut Playlist, snapshot: &FolderSnapshot, path: &Path| {
            frame(
                playlist,
                Some(snapshot),
                path,
                vec![Event::PointerMoved(egui::pos2(400.0, 50.0))],
            );
            frame(
                playlist,
                Some(snapshot),
                path,
                vec![Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 1600.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            for _ in 0..50 {
                frame(playlist, Some(snapshot), path, vec![]);
            }
            frame(playlist, Some(snapshot), path, vec![Event::PointerGone])
        };
        let output = scroll_away(&mut playlist, &snapshot, &last);
        assert!(
            !visible(&output, "track-9999.wav"),
            "manual scroll must not snap back on redraw"
        );
        let first = snapshot.items[0].path.clone();
        for _ in 0..4 {
            frame(&mut playlist, Some(&snapshot), &first, vec![]);
        }
        let output = frame(&mut playlist, Some(&snapshot), &first, vec![]);
        assert!(
            visible(&output, "track-0.wav"),
            "navigation reveals the first row"
        );
        let before = texts(&output)[0].pos;
        let second = snapshot.items[1].path.clone();
        let output = frame(&mut playlist, Some(&snapshot), &second, vec![]);
        assert_eq!(
            texts(&output)[0].pos,
            before,
            "already-visible navigation does not move the list"
        );

        snapshot.items.swap(1, 9999);
        for _ in 0..4 {
            frame(&mut playlist, Some(&snapshot), &second, vec![]);
        }
        assert!(
            visible(
                &frame(&mut playlist, Some(&snapshot), &second, vec![]),
                "track-1.wav"
            ),
            "Shell reorder reveals the new index"
        );
        assert!(!visible(
            &scroll_away(&mut playlist, &snapshot, &second),
            "track-1.wav"
        ));
        playlist.clear();
        for _ in 0..4 {
            frame(&mut playlist, Some(&snapshot), &second, vec![]);
        }
        assert!(
            visible(
                &frame(&mut playlist, Some(&snapshot), &second, vec![]),
                "track-1.wav"
            ),
            "tab reactivation reveals the current row again"
        );

        let removed = snapshot.items.pop().expect("current row");
        frame(&mut playlist, Some(&snapshot), &second, vec![]);
        assert!(playlist.focus.is_none());
        snapshot.items.insert(0, removed);
        for _ in 0..4 {
            frame(&mut playlist, Some(&snapshot), &second, vec![]);
        }
        assert!(
            visible(
                &frame(&mut playlist, Some(&snapshot), &second, vec![]),
                "track-1.wav"
            ),
            "a reappearing item can be revealed"
        );
    }

    #[test]
    fn playlist_scroll_stays_with_event_target_and_keeps_its_smoothing() {
        let context = egui::Context::default();
        let mut playlist = Playlist::default();
        let snapshot = snapshot(100);
        let mut time = 0.0;
        let mut frame = |events| {
            time += 1.0 / 60.0;
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(480.0, 300.0))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.set_max_height(200.0);
                    assert!(
                        playlist
                            .show(ui, Some(&snapshot), Some(&snapshot.items[0].path), true)
                            .is_none()
                    );
                },
            )
        };
        for _ in 0..4 {
            frame(vec![]);
        }
        let baseline = texts(&frame(vec![]))[0].pos.y;
        let offset = |output: &egui::FullOutput| {
            let row = texts(output)
                .into_iter()
                .find(|text| text.pos.y >= 8.0 && text.pos.y < 180.0)
                .expect("visible row");
            let index: usize = row
                .galley
                .job
                .text
                .split('.')
                .next()
                .expect("row number")
                .parse()
                .expect("number");
            (index - 1) as f32 * 32.0 + baseline - row.pos.y
        };
        let inside = egui::pos2(300.0, 80.0);
        let outside = egui::pos2(300.0, 260.0);
        let wheel = Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, -3.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![
            Event::PointerMoved(outside),
            wheel.clone(),
            Event::PointerMoved(inside),
        ]);
        for _ in 0..60 {
            frame(vec![]);
        }
        assert!(
            offset(&frame(vec![])).abs() < 0.01,
            "an outside wheel and its tail must never scroll the list"
        );
        frame(vec![
            Event::PointerMoved(inside),
            wheel,
            Event::PointerMoved(outside),
        ]);
        let intermediate = offset(&frame(vec![]));
        for _ in 0..60 {
            frame(vec![]);
        }
        let final_offset = offset(&frame(vec![]));
        assert!(
            intermediate > 0.0 && intermediate < final_offset,
            "retain smooth motion: intermediate {intermediate}, final {final_offset}"
        );
        let expected = context.options(|options| options.input_options.line_scroll_speed) * 3.0;
        assert!(
            (final_offset - expected).abs() < 0.01,
            "the entire owned wheel distance stays with this list"
        );
    }

    #[test]
    fn inactive_playlist_accepts_positioned_wheel_without_selecting_or_focusing() {
        let context = egui::Context::default();
        let mut playlist = Playlist::default();
        let snapshot = snapshot(100);
        let mut time = 0.0;
        for index in 0..65 {
            time += 1.0 / 60.0;
            let events = if index == 4 {
                vec![
                    Event::PointerMoved(egui::pos2(100.0, 80.0)),
                    Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta: egui::vec2(0.0, -3.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            } else {
                vec![]
            };
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(480.0, 300.0))),
                    time: Some(time),
                    focused: false,
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.set_max_height(200.0);
                    assert!(
                        playlist
                            .show(ui, Some(&snapshot), Some(&snapshot.items[0].path), true)
                            .is_none()
                    );
                },
            );
            assert!(context.memory(egui::Memory::focused).is_none());
        }
        let expected = context.options(|options| options.input_options.line_scroll_speed) * 3.0;
        assert!((playlist.scroll_offset - expected).abs() < 0.01);
    }

    #[test]
    fn empty_playlist_has_no_rows_or_actions() {
        for snapshot in [None, Some(snapshot(0))] {
            let context = egui::Context::default();
            let mut playlist = Playlist::default();
            let output = context.run_ui(egui::RawInput::default(), |ui| {
                assert!(playlist.show(ui, snapshot.as_ref(), None, true).is_none());
            });
            assert!(texts(&output).is_empty());
        }
    }

    fn snapshot(count: usize) -> FolderSnapshot {
        FolderSnapshot {
            folder_identity: ShellIdentity::new(vec![1]),
            folder_path: PathBuf::from("playlist"),
            items: (0..count)
                .map(|index| FolderMediaItem {
                    identity: ShellIdentity::new(index.to_le_bytes().to_vec()),
                    path: PathBuf::from(format!("track-{index}.wav")),
                    kind: MediaKind::Audio,
                })
                .collect(),
            sort_columns: Vec::new(),
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: SystemTime::now(),
        }
    }

    #[test]
    fn retained_lists_restore_their_own_scroll_in_the_same_ui() {
        let context = egui::Context::default();
        let snapshot = snapshot(200);
        let current = snapshot.items[0].path.clone();
        let frame = |playlist: &mut Playlist| {
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(480.0, 240.0))),
                    ..Default::default()
                },
                |ui| {
                    playlist.show(ui, Some(&snapshot), Some(&current), true);
                },
            )
        };
        let mut first = Playlist::default();
        let mut second = Playlist::default();
        for _ in 0..3 {
            frame(&mut first);
        }
        first.scroll_offset = 1280.0;
        let before = frame(&mut first);
        first.suspend();
        assert!(
            texts(&before)
                .iter()
                .any(|text| text.galley.job.text == "41. track-40.wav")
        );
        for _ in 0..3 {
            frame(&mut second);
        }
        assert_eq!(second.scroll_offset, 0.0);
        let after = frame(&mut first);
        assert_eq!(first.scroll_offset, 1280.0);
        assert!(
            texts(&after)
                .iter()
                .any(|text| text.galley.job.text == "41. track-40.wav")
        );
        second.scroll_offset = 2560.0;
        frame(&mut second);
        frame(&mut first);
        frame(&mut second);
        assert_eq!(second.scroll_offset, 2560.0);
        first.clear();
        frame(&mut first);
        assert_eq!(first.scroll_offset, 0.0);
    }

    fn texts(output: &egui::FullOutput) -> Vec<&egui::epaint::TextShape> {
        fn visit<'a>(shape: &'a Shape, result: &mut Vec<&'a egui::epaint::TextShape>) {
            match shape {
                // These existing navigation assertions inspect names, not duration placeholders.
                Shape::Text(text) if text.galley.text() != "—" => result.push(text),
                Shape::Vec(shapes) => shapes.iter().for_each(|shape| visit(shape, result)),
                _ => {}
            }
        }
        let mut result = Vec::new();
        for shape in &output.shapes {
            visit(&shape.shape, &mut result);
        }
        result
    }
}
