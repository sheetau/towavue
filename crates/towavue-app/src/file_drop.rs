use super::*;
use crate::localization::Text;
use egui::{Color32, Context, Rect, pos2, vec2};

#[derive(Default)]
pub(super) struct State {
    pub hovering: bool,
    pub batch: Option<Insertion>,
    queued: std::collections::VecDeque<(PathBuf, Insertion)>,
    pub folder: Option<FolderDrop>,
}

// Keep an identity-based gap for a whole native multi-file drop, including
// asynchronous folders. Reused tabs stay in place; new tabs retain drop order.
#[derive(Clone)]
pub(super) struct Insertion {
    before: Vec<TabId>,
}

impl Insertion {
    pub fn place_new(&self, tabs: &mut towavue_core::TabSet, previous: &[TabId]) {
        let added: Vec<_> = tabs.tab_ids().filter(|id| !previous.contains(id)).collect();
        for id in added {
            let gap = self
                .before
                .iter()
                .find_map(|anchor| tabs.tab_ids().position(|tab| tab == *anchor))
                .unwrap_or_else(|| tabs.tab_ids().count());
            tabs.reorder(id, gap);
        }
    }
}

pub(super) struct FolderDrop {
    pub generation: u64,
    pub insertion: Insertion,
}

impl State {
    pub fn take_folder(&mut self, generation: u64) -> Option<Insertion> {
        let folder = self.folder.take()?;
        if folder.generation != generation {
            self.queued.clear();
            return None;
        }
        Some(folder.insertion)
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn enqueue_dropped_path(&mut self, path: PathBuf) {
        self.file_drop.hovering = false;
        if self.modal_input_blocked() {
            self.open_dropped_path(path); // Keep the existing guarded status.
            return;
        }
        let insertion = self.file_drop.batch.clone().unwrap_or_else(|| {
            let ids: Vec<_> = self.tabs.tab_ids().collect();
            let gap = self
                .ui_context
                .as_ref()
                .filter(|_| !self.fullscreen)
                .and_then(|context| {
                    let point = self
                        .window
                        .as_ref()
                        .and_then(cursor_position_in_window)
                        .map(|(x, y)| pos2(x as f32, y as f32) / context.pixels_per_point())
                        .or_else(|| context.input(|input| input.pointer.hover_pos()))?;
                    tab_drag::incoming_gap(context, &ids, point)
                })
                .unwrap_or(ids.len());
            Insertion {
                before: ids.into_iter().skip(gap).collect(),
            }
        });
        self.file_drop.batch = Some(insertion.clone());
        self.file_drop.queued.push_back((path, insertion));
        self.process_dropped_paths();
        self.request_redraw();
    }

    pub(super) fn process_dropped_paths(&mut self) {
        if let Some(folder) = &self.file_drop.folder {
            if self
                .pending_folder
                .as_ref()
                .is_some_and(|(generation, _)| *generation == folder.generation)
            {
                return;
            }
            // A newer explicit request superseded this folder open.
            self.file_drop.folder = None;
            self.file_drop.queued.clear();
        }
        while let Some((path, insertion)) = self.file_drop.queued.pop_front() {
            if self.exit_requested {
                self.file_drop.queued.clear();
                return;
            }
            if self.modal_input_blocked() {
                self.file_drop.queued.clear();
                self.open_dropped_path(path);
                return;
            }
            let previous: Vec<_> = self.tabs.tab_ids().collect();
            let folder = path.is_dir();
            self.open_dropped_path(path);
            if folder && let Some((generation, FolderIntent::Open)) = self.pending_folder {
                self.file_drop.folder = Some(FolderDrop {
                    generation,
                    insertion,
                });
                return;
            }
            insertion.place_new(&mut self.tabs, &previous);
        }
    }
}

pub(super) fn insertion_pointer(context: &Context, blocked: bool) -> Option<egui::Pos2> {
    context.input(|input| {
        (!blocked
            && input.raw.hovered_files.iter().any(|file| {
                file.path
                    .as_deref()
                    .is_none_or(|path| !subtitles::supported(path))
            }))
        .then(|| input.pointer.hover_pos())
        .flatten()
    })
}

pub(super) fn draw(context: &Context, blocked: bool, video: bool) {
    if !context.input(|input| !input.raw.hovered_files.is_empty()) {
        return;
    }
    // OLE may not send ordinary cursor-move events while it owns capture.
    // Refresh native position on these hover-only frames; no idle polling.
    context.request_repaint_after(Duration::from_millis(16));
    let viewport = context.content_rect();
    let pointer = context
        .input(|input| input.pointer.hover_pos())
        .unwrap_or(viewport.center());
    let painter = context
        .layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("external-file-drop"),
        ))
        .with_clip_rect(viewport);
    let subtitle = context.input(|input| {
        input
            .raw
            .hovered_files
            .iter()
            .any(|file| file.path.as_deref().is_some_and(subtitles::supported))
    });
    let label = if blocked {
        Text::DropGuideBlocked
    } else if subtitle {
        if video {
            Text::DropGuideSubtitle
        } else {
            Text::SubtitleVideoRequired
        }
    } else {
        Text::DropGuideOpen
    }
    .in_language(localization::language(context));
    let kind = if blocked || (subtitle && !video) {
        tab_drag::badge::Kind::Forbidden
    } else {
        tab_drag::badge::Kind::Move
    };
    let size = (23.0 * context.pixels_per_point())
        .round()
        .clamp(1.0, 256.0) as u32;
    let cache_id = egui::Id::new("external-file-drop-badge");
    let cached = context
        .data(|data| data.get_temp::<(tab_drag::badge::Kind, u32, egui::TextureHandle)>(cache_id));
    let texture = match cached {
        Some((old, pixels, texture)) if old == kind && pixels == size => Some(texture),
        _ => tab_drag::badge::render(kind, size).map(|pixels| {
            let texture = context.load_texture(
                "file-drop-badge",
                egui::ColorImage::from_rgba_premultiplied([size as usize; 2], pixels.data()),
                egui::TextureOptions::LINEAR,
            );
            context.data_mut(|data| data.insert_temp(cache_id, (kind, size, texture.clone())));
            texture
        }),
    };
    let origin = (pointer + vec2(12.0, 12.0))
        .min(viewport.max - vec2(23.0, 23.0))
        .max(viewport.min);
    let badge = Rect::from_min_size(origin, vec2(23.0, 23.0));
    if let Some(texture) = texture {
        painter.image(
            texture.id(),
            badge,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    if blocked || subtitle {
        let text = painter.layout(
            label.into(),
            egui::FontId::proportional(13.0),
            Color32::WHITE,
            (viewport.width() - 16.0).max(1.0),
        );
        let origin = (badge.right_top() + vec2(6.0, 0.0))
            .min(viewport.max - text.size() - vec2(4.0, 4.0))
            .max(viewport.min);
        painter.rect_filled(
            Rect::from_min_size(origin, text.size()).expand(4.0),
            4.0,
            Color32::from_black_alpha(220),
        );
        painter.galley(origin, text, Color32::WHITE);
    }
    let language = localization::language(context);
    context.accesskit_node_builder(egui::Id::new("external-file-drop-notice"), |node| {
        node.set_role(egui::accesskit::Role::Label);
        node.set_label(label);
        node.set_bounds(egui::accesskit::Rect::new(
            badge.left().into(),
            badge.top().into(),
            badge.right().into(),
            badge.bottom().into(),
        ));
        node.set_description(
            if blocked {
                Text::DropGuideBlockedHelp
            } else if subtitle {
                if video {
                    Text::DropGuideSubtitle
                } else {
                    Text::SubtitleVideoRequired
                }
            } else {
                Text::DropGuideHelp
            }
            .in_language(language),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_file_batch_and_async_folder_keep_the_indicated_gap_and_drop_order() {
        let Some(root) = crate::tests::isolated_test_root(
            "file_drop::tests::native_file_batch_and_async_folder_keep_the_indicated_gap_and_drop_order",
        ) else {
            return;
        };
        for density in [1.0, 1.25, 2.0] {
            let mut app = Application::new(None, |_| {}).expect("app");
            let context = fonts::test_context();
            context.set_pixels_per_point(density);
            app.ui_context = Some(context.clone());
            let left = app.tabs.open_new(root.join("left.bmp"), MediaKind::Image);
            let right = app.tabs.open_new(root.join("right.bmp"), MediaKind::Image);
            let gallery = app.tabs.gallery().expect("Gallery");
            let frame = |app: &mut Application<_>, pointer: Option<egui::Pos2>| {
                let _ = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(960.0, 576.0),
                        )),
                        hovered_files: vec![egui::HoveredFile {
                            path: Some(root.join("first.bmp")),
                            ..Default::default()
                        }],
                        events: pointer
                            .map(|point| vec![egui::Event::PointerMoved(point)])
                            .unwrap_or_default(),
                        ..Default::default()
                    },
                    |ui| app.draw_top_bar(ui, &mut vec![]),
                );
            };
            for _ in 0..3 {
                frame(&mut app, None);
            }
            let mut point = tab_drag::tests::drop_point(&context, 2);
            point.y = 250.0; // Whole-client insertion, not just a tabbar hit.
            frame(&mut app, Some(point));
            assert_eq!(insertion_pointer(&context, false), Some(point));
            assert_eq!(
                tab_drag::incoming_gap(&context, &[gallery, left, right], point),
                Some(2)
            );
            let folder = root.join(format!("folder-{density}"));
            std::fs::create_dir(&folder).expect("folder");
            let folder_image = folder.join("only.bmp");
            tab_transfer::tests::bitmap(&folder_image);
            let first = root.join(format!("first-{density}.bmp"));
            let last = root.join(format!("last-{density}.bmp"));
            for path in [&first, &last] {
                tab_transfer::tests::bitmap(path);
            }
            app.enqueue_dropped_path(first.clone());
            app.enqueue_dropped_path(folder);
            app.enqueue_dropped_path(last.clone());
            let independent = app
                .tabs
                .open_new(root.join("independent.bmp"), MediaKind::Image);
            let deadline = Instant::now() + Duration::from_secs(20);
            while (app.file_drop.folder.is_some() || !app.file_drop.queued.is_empty())
                && Instant::now() < deadline
            {
                app.finish_folder_load();
                app.process_dropped_paths();
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(app.file_drop.folder.is_none() && app.file_drop.queued.is_empty());
            let paths: Vec<_> = app
                .tabs
                .tabs()
                .iter()
                .map(|tab| tab.target.current_path().expect("media").to_owned())
                .collect();
            assert_eq!(
                paths,
                vec![
                    root.join("left.bmp"),
                    first,
                    folder_image,
                    last,
                    root.join("right.bmp"),
                    root.join("independent.bmp")
                ]
            );
            assert_eq!(
                app.tabs.tab_ids().last(),
                Some(independent),
                "unrelated tabs opened while waiting must stay in place"
            );
            let before = app.tabs.clone();
            app.pending_dialog = Some(DialogIntent::OpenFile);
            app.enqueue_dropped_path(root.join("blocked.bmp"));
            assert_eq!(app.tabs, before);
            assert!(app.file_drop.queued.is_empty());
            app.pending_dialog = None;
            app.enqueue_dropped_path(root.join("captions.srt"));
            assert_eq!(app.tabs, before, "subtitles never create or reorder tabs");
            assert!(app.file_drop.queued.is_empty());
            app.file_drop.batch = None;
            app.fullscreen = true;
            let fullscreen = root.join(format!("fullscreen-{density}.bmp"));
            tab_transfer::tests::bitmap(&fullscreen);
            app.enqueue_dropped_path(fullscreen.clone());
            assert_eq!(
                app.tabs
                    .tabs()
                    .last()
                    .expect("appended")
                    .target
                    .current_path(),
                Some(fullscreen.as_path())
            );
            app.fullscreen = false;
            app.file_drop.batch = None;
            let canceled = root.join(format!("canceled-{density}"));
            std::fs::create_dir(&canceled).expect("canceled folder");
            app.enqueue_dropped_path(canceled);
            app.enqueue_dropped_path(root.join("canceled-tail.bmp"));
            assert_eq!(app.file_drop.queued.len(), 1);
            app.open_folder_path(root.clone());
            app.process_dropped_paths();
            assert!(
                app.file_drop.queued.is_empty() && app.file_drop.folder.is_none(),
                "an explicit request supersedes the pending drop batch"
            );
        }
    }

    #[test]
    fn subtitle_drop_guidance_distinguishes_video_targets_and_modal_guards() {
        for language in [
            localization::Language::English,
            localization::Language::Japanese,
        ] {
            let context = crate::fonts::test_context();
            localization::set_language(&context, language);
            context.enable_accesskit();
            for (video, blocked, expected) in [
                (true, false, Text::DropGuideSubtitle),
                (false, false, Text::SubtitleVideoRequired),
                (true, true, Text::DropGuideBlocked),
            ] {
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(800.0, 500.0),
                        )),
                        hovered_files: vec![egui::HoveredFile {
                            path: Some("captions.SRT".into()),
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                    |_| draw(&context, blocked, video),
                );
                let tree = output.platform_output.accesskit_update.expect("drop guide");
                assert!(
                    tree.nodes
                        .iter()
                        .any(|(_, node)| node.label() == Some(expected.in_language(language)))
                );
            }
        }
    }

    #[test]
    fn file_hover_uses_compact_shared_badge_without_covering_media() {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            let viewport = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 500.0));
            let frame = |hovered| {
                context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(viewport),
                        hovered_files: if hovered {
                            vec![egui::HoveredFile {
                                path: Some("test.png".into()),
                                ..Default::default()
                            }]
                        } else {
                            vec![]
                        },
                        events: vec![egui::Event::PointerMoved(pos2(400.0, 250.0))],
                        ..Default::default()
                    },
                    |_| draw(&context, false, false),
                )
            };
            frame(true);
            let output = frame(true);
            let badge = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) => Some(mesh.calc_bounds()),
                    _ => None,
                })
                .expect("shared cursor badge");
            assert_eq!(badge.size(), vec2(23.0, 23.0));
            assert_eq!(badge.min, pos2(412.0, 262.0));
            assert!(!output.shapes.iter().any(
                |shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.rect == viewport)
            ));
            assert!(context.dragged_id().is_none());
            assert!(frame(false).shapes.is_empty());
        }
    }

    #[test]
    fn centered_guide_preserves_whole_window_folder_drop_and_modal_guard() {
        let Some(root) = crate::tests::isolated_test_root(
            "file_drop::tests::centered_guide_preserves_whole_window_folder_drop_and_modal_guard",
        ) else {
            return;
        };
        let folder = root.join("dropped-folder");
        std::fs::create_dir(&folder).expect("owned empty folder");
        let mut app = crate::Application::new(None, |_| {}).expect("app");
        let context = crate::fonts::test_context();
        app.ui_context = Some(context.clone());
        app.pending_dialog = Some(crate::DialogIntent::OpenFolder);
        for pointer in [pos2(2.0, 2.0), pos2(400.0, 250.0), pos2(798.0, 498.0)] {
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 500.0))),
                    hovered_files: vec![egui::HoveredFile {
                        path: Some(folder.clone()),
                        ..Default::default()
                    }],
                    events: vec![egui::Event::PointerMoved(pointer)],
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut vec![]),
            );
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "Close the dialog before dropping files")));
            app.open_dropped_path(folder.clone());
            assert!(app.pending_folder.is_none());
            app.pending_dialog = None;
            app.open_dropped_path(folder.clone());
            assert!(
                app.pending_folder.is_some(),
                "the central guide is not a drop hit rectangle"
            );
            app.pending_folder = None;
            app.pending_dialog = Some(crate::DialogIntent::OpenFolder);
        }
        assert!(app.edits.is_empty());
    }
}
