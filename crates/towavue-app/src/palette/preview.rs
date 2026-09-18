use std::path::{Path, PathBuf};

use egui::{Context, TextureHandle, Ui};
use towavue_core::MediaKind;
use towavue_runtime_windows::{MediaPreview, PreviewCache, PreviewLoader};

pub(super) struct FilePreview {
    loader: PreviewLoader,
    generation: u64,
    path: Option<PathBuf>,
    texture: Option<Result<TextureHandle, String>>,
}

impl FilePreview {
    pub fn new(cache: PreviewCache, notify: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        Ok(Self {
            loader: PreviewLoader::new(cache, notify)?,
            generation: 0,
            path: None,
            texture: None,
        })
    }

    pub fn clear(&mut self) {
        if self.path.take().is_some() {
            self.generation = self.loader.request(Vec::new());
        }
        self.texture = None;
    }

    pub fn show(&mut self, ui: &mut Ui, path: Option<&Path>) {
        if self.path.as_deref() != path {
            self.texture = None;
            self.path = path.map(Path::to_path_buf);
            let request = path
                .and_then(|path| MediaKind::from_path(path).map(|kind| (path.to_owned(), kind)))
                .into_iter()
                .collect();
            self.generation = self.loader.request(request);
            if path.is_some_and(|path| MediaKind::from_path(path).is_none()) {
                self.texture = Some(Err("Unsupported media type".into()));
            }
        }
        paint(ui, path, self.texture.as_ref());
    }

    pub fn finish(&mut self, context: &Context) -> bool {
        let mut changed = false;
        for result in self.loader.take_completed() {
            changed |= self.accept(context, &result.path, result.generation, result.result);
        }
        changed
    }

    fn accept(
        &mut self,
        context: &Context,
        path: &Path,
        generation: u64,
        result: Result<MediaPreview, String>,
    ) -> bool {
        if generation != self.generation || self.path.as_deref() != Some(path) {
            return false;
        }
        self.texture = Some(result.map(|preview| {
            context.load_texture(
                "picker-file-preview",
                crate::image_color::preview_color_image(&preview.image),
                egui::TextureOptions::LINEAR,
            )
        }));
        true
    }
}

pub(super) fn paint(
    ui: &mut Ui,
    path: Option<&Path>,
    texture: Option<&Result<TextureHandle, String>>,
) {
    let Some(path) = path else { return };
    // Keep several result rows available in small windows. Loading, errors and
    // aspect-ratio changes use the same reserved area, so the list never jumps.
    let remaining = (ui.ctx().content_rect().bottom() - ui.cursor().top() - 8.0)
        .min(ui.available_height())
        .max(0.0);
    let height = (remaining - 88.0).clamp(0.0, 160.0);
    if height < 1.0 {
        return;
    }
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(egui::accesskit::Role::Image);
        node.set_label(format!("Preview: {}", path.display()));
        node.set_bounds(egui::accesskit::Rect::new(
            rect.left().into(),
            rect.top().into(),
            rect.right().into(),
            rect.bottom().into(),
        ));
    });
    let painter = ui.painter().with_clip_rect(rect);
    match texture {
        Some(Ok(texture)) => {
            let size = texture.size_vec2();
            let scale = (rect.width().min(240.0) / size.x)
                .min(height / size.y)
                .min(1.0);
            painter.image(
                texture.id(),
                egui::Rect::from_center_size(rect.center(), size * scale),
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        Some(Err(_)) => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No preview",
                egui::FontId::proportional(12.0),
                crate::chrome::MUTED,
            );
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::tests::{key, open_frame_at, picker_text};
    use crate::palette::{CommandPalette, OpenSources};
    use std::os::windows::process::CommandExt;
    use std::time::{Duration, Instant};

    #[test]
    fn picker_preview_tracks_selection_and_leaves_results_inside_small_windows() {
        for size in [egui::vec2(640.0, 480.0), egui::vec2(240.0, 180.0)] {
            for density in [1.0, 1.25, 2.0] {
                let context = crate::fonts::test_context();
                context.enable_accesskit();
                context.global_style_mut(|style| {
                    crate::chrome::style(style);
                    style.animation_time = 0.0;
                    style.scroll_animation = egui::style::ScrollAnimation::none();
                });
                let mut palette = CommandPalette::default();
                palette.open_files(false);
                let files: Vec<_> = (0..40)
                    .map(|i| PathBuf::from(format!("{i:02}.png")))
                    .collect();
                let sources = OpenSources {
                    files: &files,
                    ..Default::default()
                };
                let frame = |palette: &mut CommandPalette, events| {
                    open_frame_at(&context, palette, sources, events, (size, 32.0, density))
                };
                for _ in 0..3 {
                    frame(&mut palette, vec![]);
                }
                for (index, file) in files.iter().enumerate() {
                    let events = if index == 0 {
                        vec![]
                    } else {
                        vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)]
                    };
                    let (output, choices) = frame(&mut palette, events);
                    assert!(choices.is_empty());
                    assert_eq!(palette.selected_path.as_ref(), Some(file));
                    let name = file.to_string_lossy();
                    let (text, clip) = picker_text(&output, &name).unwrap_or_else(|| {
                        panic!("selected row visible: {size:?}, density={density}, index={index}")
                    });
                    let row = egui::Rect::from_min_size(text.pos, text.galley.size());
                    assert!(clip.intersects(row));
                    assert!(row.bottom() <= size.y + 1.0 / density);
                    let tree = output
                        .platform_output
                        .accesskit_update
                        .as_ref()
                        .expect("tree");
                    let preview = tree.nodes.iter().find(|(_, node)| {
                        node.label() == Some(format!("Preview: {name}").as_str())
                    });
                    if size.y > 200.0 {
                        let bounds = preview
                            .expect("preview above results")
                            .1
                            .bounds()
                            .expect("bounds");
                        assert!(bounds.y1 as f32 <= row.top());
                        assert!((bounds.y1 - bounds.y0 - 160.0).abs() <= 1.0);
                    }
                }
                // Filtering to nothing and entering command/folder modes removes the preview.
                for query in ["absent", ">"] {
                    palette.query = query.into();
                    let (output, _) = frame(&mut palette, vec![]);
                    assert!(
                        !output
                            .platform_output
                            .accesskit_update
                            .as_ref()
                            .expect("tree")
                            .nodes
                            .iter()
                            .any(|(_, node)| node
                                .label()
                                .is_some_and(|l| l.starts_with("Preview: ")))
                    );
                }
                palette.open_files(true);
                let (output, _) = frame(&mut palette, vec![]);
                assert!(
                    !output
                        .platform_output
                        .accesskit_update
                        .as_ref()
                        .expect("tree")
                        .nodes
                        .iter()
                        .any(|(_, node)| node.label().is_some_and(|l| l.starts_with("Preview: ")))
                );
            }
        }
    }

    #[test]
    fn picker_preview_decodes_media_wakes_ui_and_rejects_replaced_or_closed_results() {
        let Some(root) = crate::tests::isolated_test_root(
            "palette::preview::tests::picker_preview_decodes_media_wakes_ui_and_rejects_replaced_or_closed_results",
        ) else {
            return;
        };
        let files = [
            root.join("image.png"),
            root.join("video.mp4"),
            root.join("audio.wav"),
        ];
        let ffmpeg =
            PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
        let output = std::process::Command::new(ffmpeg)
            .creation_flags(0x08000000)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=80x60:r=10",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000",
                "-map",
                "0:v",
                "-frames:v",
                "1",
            ])
            .arg(&files[0])
            .args(["-map", "0:v", "-t", "0.5", "-c:v", "mpeg4"])
            .arg(&files[1])
            .args(["-map", "1:a", "-t", "0.5", "-c:a", "pcm_s16le"])
            .arg(&files[2])
            .output()
            .expect("generate owned media");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let original: Vec<_> = files
            .iter()
            .map(|p| std::fs::read(p).expect("source"))
            .collect();
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut app = crate::Application::new_with_preview_cache(
            None,
            move |event| {
                let _ = sender.send(event);
            },
            PreviewCache::new(root.join("cache")).expect("cache"),
        )
        .expect("app");
        let context = crate::fonts::test_context();
        app.ui_context = Some(context.clone());
        app.recent_paths = files.to_vec();
        app.palette.open_files(false);
        app.palette_open = true;
        let frame = |app: &mut crate::Application<_>, events| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(640.0, 480.0),
                    )),
                    events,
                    ..Default::default()
                },
                |_| app.draw_command_palette(&context, 32.0, &mut actions),
            );
            assert!(actions.is_empty(), "preview cannot open or seek media");
            output
        };
        let tabs = app.tabs.tabs().len();
        for (index, path) in files.iter().enumerate() {
            frame(
                &mut app,
                if index == 0 {
                    vec![]
                } else {
                    vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)]
                },
            );
            assert_eq!(
                app.palette.preview.as_ref().expect("preview").path.as_ref(),
                Some(path)
            );
            let deadline = Instant::now() + Duration::from_secs(15);
            while app
                .palette
                .preview
                .as_ref()
                .expect("preview")
                .texture
                .is_none()
            {
                let event = receiver
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .expect("worker wakes idle UI");
                app.handle_app_event(event);
            }
            let preview = app.palette.preview.as_ref().expect("preview");
            let texture = preview
                .texture
                .as_ref()
                .expect("completed")
                .as_ref()
                .expect("decoded")
                .id();
            let generation = preview.generation;
            let output = frame(&mut app, vec![]);
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Mesh(mesh) if mesh.texture_id == texture)));
            assert_eq!(
                app.palette.preview.as_ref().expect("preview").generation,
                generation
            );
            assert!(app.path.is_none() && app.session.is_none());
            assert_eq!(app.tabs.tabs().len(), tabs);
            assert!(!app.palette.preview.as_mut().expect("preview").accept(
                &context,
                path,
                generation.wrapping_sub(1),
                Err("stale".into())
            ));
            assert!(!app.palette.preview.as_mut().expect("preview").accept(
                &context,
                &root.join("replaced.png"),
                generation,
                Err("wrong source".into())
            ));
        }
        let preview = app.palette.preview.as_mut().expect("preview");
        let generation = preview.generation;
        assert!(preview.accept(&context, &files[2], generation, Err("decode failed".into())));
        for _ in 0..3 {
            let output = frame(&mut app, vec![]);
            assert!(picker_text(&output, "No preview").is_some());
            assert_eq!(
                app.palette.preview.as_ref().expect("preview").generation,
                generation,
                "failed previews do not retry on every frame"
            );
        }
        let preview = app.palette.preview.as_ref().expect("preview");
        let generation = preview.generation;
        let old = preview.path.clone().expect("path");
        app.cancel_command_overlay();
        let preview = app.palette.preview.as_mut().expect("preview");
        assert!(preview.path.is_none() && preview.texture.is_none());
        assert!(!preview.accept(&context, &old, generation, Err("late".into())));
        app.palette.open_files(false);
        frame(&mut app, vec![]);
        let generation = app.palette.preview.as_ref().expect("preview").generation;
        app.palette.query = ">".into();
        frame(&mut app, vec![]);
        assert!(
            app.palette
                .preview
                .as_ref()
                .expect("preview")
                .path
                .is_none()
        );
        assert_ne!(
            app.palette.preview.as_ref().expect("preview").generation,
            generation
        );
        for (path, bytes) in files.iter().zip(original) {
            assert_eq!(std::fs::read(path).expect("unchanged source"), bytes);
        }
    }
}
