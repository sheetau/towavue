use std::path::{Path, PathBuf};

use egui::RichText;
use towavue_core::{FolderSnapshot, MediaKind};

#[derive(Default)]
pub struct Playlist {
    focus: Option<(PathBuf, usize)>,
    keyboard_focus: Option<(PathBuf, egui::Id)>,
    wheel: crate::wheel_input::Scroll,
    scroll_offset: f32,
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
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: Option<&FolderSnapshot>,
        current: Option<&Path>,
        allow_wheel: bool,
    ) -> Option<PathBuf> {
        let mut chosen = None;
        crate::wheel_input::begin_frame(ui.ctx());
        let items: Vec<_> = snapshot
            .into_iter()
            .flat_map(|snapshot| snapshot.items_of_kind(MediaKind::Audio))
            .collect();
        egui::Frame::new().inner_margin(8).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
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
                    let offset = if top < offset {
                        top
                    } else if top + 32.0 > offset + height {
                        (top + 32.0 - height).max(0.0)
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
            let output = scroll.show_rows(ui, 32.0, items.len(), |ui, rows| {
                for index in rows {
                    let item = items[index];
                    let selected = current == Some(item.path.as_path());
                    let name = crate::display_name(&item.path);
                    let text = RichText::new(format!("{}. {name}", index + 1)).size(14.0);
                    let response = ui
                        .scope_builder(
                            egui::UiBuilder::new().id(ui.id().with(("audio-row", &item.path))),
                            |ui| {
                                ui.add_sized(
                                    egui::vec2(ui.available_width(), 32.0),
                                    egui::Button::selectable(selected, (text, egui::Atom::grow()))
                                        .truncate(),
                                )
                            },
                        )
                        .inner
                        .on_hover_ui(|ui| {
                            ui.set_max_width(
                                (ui.ctx().viewport_rect().width() - 32.0).clamp(1.0, 400.0),
                            );
                            ui.add(egui::Label::new(name).wrap());
                        });
                    if reveal == Some(index) {
                        response.request_focus();
                    }
                    crate::tab_focus::observe(&response, ("playlist-row", &item.path));
                    if response.has_focus() {
                        self.keyboard_focus = Some((item.path.clone(), response.id));
                    }
                    ui.ctx().accesskit_node_builder(response.id, |node| {
                        node.clear_toggled();
                        node.set_description(format!(
                            "{}{}",
                            item.path.display(),
                            if selected { " (current track)" } else { "" }
                        ));
                    });
                    if response.clicked() {
                        chosen = Some(item.path.clone());
                    }
                }
            });
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
                Shape::Text(text) => result.push(text),
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
