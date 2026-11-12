use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::{Color32, Rect, TextureHandle, TextureOptions, Vec2};
use towavue_core::{CommandId, MediaKind};

use crate::hover_help::HoverHelp;
use crate::scroll_style::ScrollAreaStyle;
use crate::{AppEvent, Application, ImagePresentation, UiAction, chrome, localization, media_time};

#[derive(Clone)]
pub(super) struct Transport {
    repeat: bool,
    remaining: Duration,
    ended: bool,
    frame_times: std::sync::Arc<[Duration]>,
    thumbnails: HashMap<usize, TextureHandle>,
}

impl Transport {
    pub(super) fn without_thumbnails(&self) -> Self {
        Self {
            thumbnails: HashMap::new(),
            ..self.clone()
        }
    }
}

impl ImagePresentation {
    fn open_transport(&mut self) {
        if self.animation_transport.is_none() {
            self.plays_left = if self.decoded.animation_plays == 0 {
                0
            } else {
                1
            };
            self.animation_transport = Some(Transport {
                frame_times: std::iter::once(Duration::ZERO)
                    .chain(
                        self.decoded
                            .frames
                            .iter()
                            .scan(Duration::ZERO, |time, frame| {
                                *time += frame.delay;
                                Some(*time)
                            }),
                    )
                    .collect(),
                repeat: self.decoded.animation_plays == 0,
                remaining: self
                    .next_frame_at
                    .map_or(self.decoded.frames[self.frame_index].delay, |deadline| {
                        deadline.saturating_duration_since(Instant::now())
                    }),
                ended: self.next_frame_at.is_none(),
                thumbnails: HashMap::new(),
            });
        }
    }

    fn close_transport(&mut self) {
        self.animation_transport = None;
        self.plays_left = self.decoded.animation_plays;
        self.next_frame_at = Some(Instant::now() + self.decoded.frames[self.frame_index].delay);
    }

    fn select_animation_frame(&mut self, index: usize) {
        self.open_transport();
        self.frame_index = index.min(self.decoded.frames.len() - 1);
        self.next_frame_at = None;
        let transport = self
            .animation_transport
            .as_mut()
            .expect("animation transport");
        transport.remaining = self.decoded.frames[self.frame_index].delay;
        transport.ended = false;
        self.texture.set(
            crate::color_image(&self.decoded.frames[self.frame_index]),
            self.sampling.get(),
        );
    }

    fn toggle_animation_pause(&mut self) {
        self.open_transport();
        let now = Instant::now();
        if let Some(deadline) = self.next_frame_at.take() {
            self.animation_transport
                .as_mut()
                .expect("animation transport")
                .remaining = deadline.saturating_duration_since(now);
        } else {
            if self
                .animation_transport
                .as_ref()
                .is_some_and(|transport| transport.ended)
            {
                self.select_animation_frame(0);
            }
            let transport = self
                .animation_transport
                .as_ref()
                .expect("animation transport");
            self.plays_left = if transport.repeat { 0 } else { 1 };
            self.next_frame_at = Some(now + transport.remaining);
        }
    }

    pub(super) fn animation_ended(&mut self) {
        if let Some(transport) = &mut self.animation_transport {
            transport.ended = true;
            transport.remaining = Duration::ZERO;
        }
    }

    pub(super) fn restored_animation_textures(
        &self,
    ) -> Vec<(egui::TextureId, egui::epaint::ImageDelta)> {
        self.animation_transport
            .iter()
            .flat_map(|transport| transport.thumbnails.iter())
            .map(|(index, texture)| {
                (
                    texture.id(),
                    egui::epaint::ImageDelta::full(
                        thumbnail(&self.decoded.frames[*index]),
                        TextureOptions::LINEAR,
                    ),
                )
            })
            .collect()
    }

    fn animation_duration(&self) -> Duration {
        self.animation_transport
            .as_ref()
            .and_then(|transport| transport.frame_times.last().copied())
            .unwrap_or_else(|| self.decoded.frames.iter().map(|frame| frame.delay).sum())
    }

    fn animation_position(&self, now: Instant) -> Duration {
        let before = self.animation_transport.as_ref().map_or_else(
            || {
                self.decoded.frames[..self.frame_index]
                    .iter()
                    .map(|frame| frame.delay)
                    .sum()
            },
            |transport| transport.frame_times[self.frame_index],
        );
        let delay = self.decoded.frames[self.frame_index].delay;
        let remaining = self.next_frame_at.map_or_else(
            || {
                self.animation_transport
                    .as_ref()
                    .map_or(delay, |transport| transport.remaining)
            },
            |deadline| deadline.saturating_duration_since(now),
        );
        before + delay.saturating_sub(remaining)
    }
}

impl<N> Application<N>
where
    N: Fn(AppEvent) + Send + Sync + 'static,
{
    pub(super) fn animation_timeline_open(&self) -> bool {
        self.timeline_open
            && !self.reading_mode
            && self.media_kind == Some(MediaKind::Image)
            && self
                .image
                .as_ref()
                .is_some_and(|image| image.decoded.is_animated())
    }

    pub(super) fn animation_timeline_visible(&self) -> bool {
        self.animation_timeline_open() && !self.fullscreen
    }

    pub(super) fn animation_command(&mut self, command: CommandId) -> bool {
        if self.media_kind != Some(MediaKind::Image) {
            return false;
        }
        if command == CommandId::ToggleTimeline
            && self
                .image
                .as_ref()
                .is_some_and(|image| image.decoded.is_animated())
        {
            if self.fullscreen {
                self.set_fullscreen(false);
                self.timeline_open = true;
            } else {
                self.timeline_open = !self.timeline_open;
            }
            let image = self.image.as_mut().expect("animated image");
            if self.timeline_open {
                image.open_transport();
            } else {
                image.close_transport();
            }
            self.request_redraw();
            return true;
        }
        if !self.animation_timeline_open() {
            return false;
        }
        let image = self.image.as_mut().expect("animated image");
        image.open_transport();
        match command {
            CommandId::TogglePause => image.toggle_animation_pause(),
            CommandId::SeekBackward | CommandId::PreviousVideoFrame => {
                image.select_animation_frame(image.frame_index.saturating_sub(1))
            }
            CommandId::SeekForward | CommandId::NextVideoFrame => {
                image.select_animation_frame(image.frame_index.saturating_add(1))
            }
            CommandId::ToggleVideoRepeat => {
                let transport = image
                    .animation_transport
                    .as_mut()
                    .expect("animation transport");
                transport.repeat = !transport.repeat;
                image.plays_left = if transport.repeat { 0 } else { 1 };
            }
            command if command.video_seek_percent().is_some() => {
                let total = image.animation_duration();
                let target = total.mul_f64(
                    f64::from(command.video_seek_percent().expect("seek percentage")) / 100.0,
                );
                let index = image
                    .animation_transport
                    .as_ref()
                    .expect("animation transport")
                    .frame_times
                    .partition_point(|time| *time <= target)
                    .saturating_sub(1);
                image.select_animation_frame(index);
            }
            _ => return false,
        }
        self.request_redraw();
        true
    }

    pub(super) fn draw_animation_transport(&self, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        let language = self.language();
        let image = self.image.as_ref().expect("animated image");
        let playing = image.next_frame_at.is_some();
        if playing {
            // Long frame delays still have a running elapsed-time display.
            ui.ctx().request_repaint_after(Duration::from_millis(33));
        }
        let repeat = image
            .animation_transport
            .as_ref()
            .map_or(image.decoded.animation_plays == 0, |transport| {
                transport.repeat
            });
        let position = image.animation_position(Instant::now());
        let total = image.animation_duration();
        let label = format!(
            "{} / {} ({}/{})",
            crate::format_time_precise(media_time(position)),
            crate::format_time_precise(media_time(total)),
            image.frame_index + 1,
            image.decoded.frames.len()
        );
        if chrome::transport_button(
            ui,
            if playing {
                chrome::Icon::Pause
            } else {
                chrome::Icon::Play
            },
            &self.command_hint(
                CommandId::TogglePause,
                if playing {
                    localization::Text::Pause
                } else {
                    localization::Text::PlayReplay
                }
                .in_language(language),
            ),
        )
        .clicked()
        {
            actions.push(UiAction::Command(CommandId::TogglePause));
        }
        if chrome::audio_button(
            ui,
            chrome::AudioIcon::Repeat,
            repeat,
            &self.command_hint(
                CommandId::ToggleVideoRepeat,
                if repeat {
                    localization::Text::VideoRepeatOff
                } else {
                    localization::Text::VideoRepeatOn
                }
                .in_language(language),
            ),
        )
        .clicked()
        {
            actions.push(UiAction::Command(CommandId::ToggleVideoRepeat));
        }
        let compact = ui.max_rect().width() < 340.0;
        let shown = if compact {
            crate::format_time_precise(media_time(position))
        } else {
            label.clone()
        };
        if crate::status_bar::item(ui, egui::RichText::new(shown).size(chrome::TEXT_LG), true)
            .help_text(label)
            .clicked()
        {
            actions.push(UiAction::Command(CommandId::ToggleTimeline));
        }
    }

    pub(super) fn draw_animation_timeline(&mut self, root: &mut egui::Ui) {
        let panel = self.timeline_panel_id();
        let enabled = !self.modal_input_blocked()
            && !self.filmstrip_open
            && !self.image_edit_pending
            && self.image_handoff.is_none();
        let mut height = self.timeline_height(true);
        let image = self.image.as_mut().expect("animated image");
        image.open_transport();
        let mut selected = None;
        crate::timeline_edit::show_panel(
            root,
            panel,
            &mut height,
            enabled,
            egui::Margin {
                left: 0,
                right: 0,
                top: 8,
                bottom: 0,
            },
            |ui| {
                let background = ui.available_rect_before_wrap();
                let side = background.height().max(1.0);
                let pitch = side + 5.0;
                let count = image.decoded.frames.len();
                ui.style_mut().always_scroll_the_only_direction = true;
                let scroll = &mut ui.spacing_mut().scroll;
                scroll.floating = true;
                scroll.floating_allocated_width = 0.0;
                scroll.bar_outer_margin = 0.0;
                scroll.fade.strength = 0.0;
                let track = Rect::from_min_max(
                    background.min + egui::vec2(8.0, 0.0),
                    background.max - egui::vec2(8.0, 0.0),
                );
                let anchor_id = panel.with("animation-viewport");
                let previous = ui.data(|data| data.get_temp::<(f32, f32, f32)>(anchor_id));
                let mut area = egui::ScrollArea::horizontal()
                    .content_margin(0)
                    .id_salt("animation-frames")
                    .auto_shrink([false, false])
                    .scroll_bar_rect(track);
                if let Some((old_pitch, old_width, offset)) = previous
                    && (old_pitch - pitch).abs() > 0.01
                {
                    // Keep the frame coordinate at the viewport center while resizing.
                    let center = (offset + old_width * 0.5 - 8.0) / old_pitch;
                    area = area.horizontal_scroll_offset(
                        (center * pitch + 8.0 - background.width() * 0.5).max(0.0),
                    );
                }
                let output = area.show_viewport_styled(ui, |ui, viewport| {
                    ui.set_min_size(egui::vec2(
                        (count as f32 * pitch - 5.0).max(0.0) + 16.0,
                        side,
                    ));
                    let first = ((viewport.left() - 8.0).max(0.0) / pitch).floor() as usize;
                    let last = (((viewport.right() - 8.0).max(0.0) / pitch).ceil() as usize + 1)
                        .min(count);
                    let transport = image
                        .animation_transport
                        .as_mut()
                        .expect("animation transport");
                    // Decoded frames already belong to the image; only visible
                    // reduced textures are retained, independent of file previews.
                    transport
                        .thumbnails
                        .retain(|index, _| (first..last).contains(index));
                    for index in first..last {
                        let rect = Rect::from_min_size(
                            ui.min_rect().min + egui::vec2(8.0 + index as f32 * pitch, 0.0),
                            Vec2::splat(side),
                        );
                        let frame = &image.decoded.frames[index];
                        let texture = transport.thumbnails.entry(index).or_insert_with(|| {
                            ui.ctx().load_texture(
                                format!("animation-frame-{index}"),
                                thumbnail(frame),
                                TextureOptions::LINEAR,
                            )
                        });
                        ui.painter().rect_filled(rect, 3.0, chrome::BORDER);
                        let size = texture.size_vec2();
                        let fitted =
                            Rect::from_center_size(rect.center(), size * (side / size.max_elem()));
                        egui::Image::from_texture((texture.id(), texture.size_vec2()))
                            .corner_radius(3.0)
                            .paint_at(ui, fitted);
                        if index == image.frame_index {
                            ui.painter().rect_stroke(
                                rect,
                                3.0,
                                egui::Stroke::new(
                                    1.0 / ui.ctx().pixels_per_point(),
                                    crate::chrome::FOREGROUND,
                                ),
                                egui::StrokeKind::Inside,
                            );
                        }
                        let response = ui.interact(rect, ui.id().with(index), egui::Sense::click());
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                ui.is_enabled(),
                                format!("{}/{}", index + 1, count),
                            )
                        });
                        crate::tab_focus::observe_pointer_control(
                            &response,
                            ("animation-frame", index),
                        );
                        if response.clicked() {
                            selected = Some(index);
                        }
                    }
                });
                ui.data_mut(|data| {
                    data.insert_temp(
                        anchor_id,
                        (pitch, background.width(), output.state.offset.x),
                    )
                });
            },
        );
        if let Some(index) = selected {
            image.select_animation_frame(index);
            root.ctx().request_repaint();
        }
        self.remember_timeline_height(height, true);
    }
}

fn thumbnail(frame: &towavue_runtime_windows::DecodedImageFrame) -> egui::ColorImage {
    let scale = 128.0 / frame.width.max(frame.height).max(1) as f32;
    let width = (frame.width as f32 * scale.min(1.0)).round().max(1.0) as usize;
    let height = (frame.height as f32 * scale.min(1.0)).round().max(1.0) as usize;
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let source = ((y * frame.height as usize / height) * frame.width as usize
                + x * frame.width as usize / width)
                * 4;
            let rgba = &frame.rgba[source..source + 4];
            pixels.push(Color32::from_rgba_unmultiplied(
                rgba[0], rgba[1], rgba[2], rgba[3],
            ));
        }
    }
    egui::ColorImage::new([width, height], pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use towavue_runtime_windows::{DecodedImage, DecodedImageFrame};

    fn decoded(count: usize, plays: u32) -> Arc<DecodedImage> {
        Arc::new(DecodedImage {
            animation_plays: plays,
            format: "test",
            frames: (0..count)
                .map(|index| DecodedImageFrame {
                    width: 2,
                    height: 2,
                    rgba: [index as u8, 80, 180, 255].repeat(4),
                    delay: Duration::from_millis(40 * (index as u64 + 1)),
                })
                .collect(),
        })
    }

    #[test]
    #[ignore = "requires hidden hardware D3D11; animated timeline frame and border readbacks"]
    fn animation_timeline_reaches_gpu_at_supported_densities() {
        use winit::platform::windows::EventLoopBuilderExtWindows;
        let Some(_root) = crate::tests::isolated_test_root(
            "image_animation::tests::animation_timeline_reaches_gpu_at_supported_densities",
        ) else {
            return;
        };
        struct Trial(bool);
        impl winit::application::ApplicationHandler for Trial {
            fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
                let window = event_loop
                    .create_window(winit::window::Window::default_attributes().with_visible(false))
                    .expect("hidden window");
                let mut renderer = crate::FrameRenderer::new(&window).expect("hardware D3D11");
                for density in [1.0, 1.25, 2.0] {
                    let context = crate::fonts::test_context();
                    context.global_style_mut(chrome::style);
                    let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
                    let mut source = (*decoded(16, 0)).clone();
                    source.frames[1].rgba = [200, 40, 60, 255].repeat(4);
                    app.image = Some(
                        ImagePresentation::from_named_frame(
                            &context,
                            "animation",
                            Arc::new(source),
                            0,
                            TextureOptions::LINEAR,
                        )
                        .expect("image"),
                    );
                    app.media_kind = Some(MediaKind::Image);
                    app.timeline_open = true;
                    let width = (600.0 * density) as u32;
                    let height = (400.0 * density) as u32;
                    renderer.resize_surface(width, height).expect("surface");
                    for (stage, selected) in [0, 1, 1].into_iter().enumerate() {
                        if stage == 2 {
                            let device = renderer.graphics_device();
                            renderer.release_surface();
                            renderer = crate::FrameRenderer::with_graphics_device(&window, device)
                                .expect("recreated renderer");
                            renderer
                                .resize_surface(width, height)
                                .expect("recreated surface");
                        }
                        app.image
                            .as_mut()
                            .expect("image")
                            .select_animation_frame(selected);
                        for pass in 0..3 {
                            let mut input = egui::RawInput {
                                screen_rect: Some(Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(600.0, 400.0),
                                )),
                                ..Default::default()
                            };
                            input
                                .viewports
                                .get_mut(&egui::ViewportId::ROOT)
                                .expect("viewport")
                                .native_pixels_per_point = Some(density);
                            let mut output =
                                context.run_ui(input, |ui| app.draw_animation_timeline(ui));
                            if stage == 2 && pass == 0 {
                                output
                                    .textures_delta
                                    .set
                                    .extend(app.restored_ui_textures(&context));
                            }
                            renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
                            renderer
                                .render_ui(&context, output)
                                .expect("timeline render");
                            renderer.present_surface().expect("Present");
                        }
                        let pixels = renderer.verification_surface_rgba().expect("GPU pixels");
                        if let Some(directory) = std::env::var_os("TOWAVUE_REFERENCE_RENDER_DIR") {
                            std::fs::write(
                                std::path::PathBuf::from(directory)
                                    .join(format!("animation-{stage}-{width}x{height}.rgba")),
                                &pixels,
                            )
                            .expect("reference pixels");
                        }
                        let pixel = |x: f32, y: f32| {
                            let offset = (((y * density) as u32 * width + (x * density) as u32) * 4)
                                as usize;
                            &pixels[offset..offset + 4]
                        };
                        assert_eq!(pixel(50.0, 350.0), &[0, 80, 180, 255]);
                        assert_eq!(pixel(145.0, 350.0), &[200, 40, 60, 255]);
                        let border = pixel(8.0 + selected as f32 * 93.0, 350.0);
                        assert!(
                            border[..3].iter().all(|channel| *channel >= 220),
                            "current frame border at density {density}: {border:?}"
                        );
                    }
                }
                self.0 = true;
                event_loop.exit();
            }
            fn window_event(
                &mut self,
                _: &winit::event_loop::ActiveEventLoop,
                _: winit::window::WindowId,
                _: winit::event::WindowEvent,
            ) {
            }
        }
        let mut trial = Trial(false);
        let mut builder = winit::event_loop::EventLoop::builder();
        builder.with_any_thread(true);
        builder
            .build()
            .expect("event loop")
            .run_app(&mut trial)
            .expect("hidden trial");
        assert!(trial.0);
    }

    #[test]
    fn variable_frame_delays_pause_step_repeat_and_hidden_autoplay() {
        let context = crate::fonts::test_context();
        let mut image = ImagePresentation::from_named_frame(
            &context,
            "animation",
            decoded(3, 1),
            0,
            TextureOptions::LINEAR,
        )
        .expect("image");
        image.open_transport();
        image.select_animation_frame(1);
        assert_eq!(
            image.animation_position(Instant::now()),
            Duration::from_millis(40)
        );
        image.toggle_animation_pause();
        let deadline = image.next_frame_at.expect("playing");
        assert!(!image.advance_animation(deadline - Duration::from_nanos(1)));
        assert!(image.advance_animation(deadline));
        assert_eq!(image.frame_index, 2);
        image.advance_animation(deadline + Duration::from_millis(120));
        assert!(image.next_frame_at.is_none());
        assert_eq!(
            image.animation_position(Instant::now()),
            Duration::from_millis(240)
        );
        image.toggle_animation_pause();
        assert_eq!(image.frame_index, 0, "replay restarts an ended animation");
        image.select_animation_frame(2);
        image
            .animation_transport
            .as_mut()
            .expect("transport")
            .repeat = true;
        image.toggle_animation_pause();
        image.advance_animation(image.next_frame_at.expect("playing"));
        assert_eq!(image.frame_index, 0);
        image.select_animation_frame(1);
        image.close_transport();
        assert!(image.next_frame_at.is_some());
        assert_eq!(
            image.plays_left, 1,
            "hidden transport restores the source loop policy"
        );
        assert!(image.animation_transport.is_none());
    }

    #[test]
    fn animation_commands_keep_image_editing_and_exclude_reading_and_time_edits() {
        let Some(root) = crate::tests::isolated_test_root(
            "image_animation::tests::animation_commands_keep_image_editing_and_exclude_reading_and_time_edits",
        ) else {
            return;
        };
        let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
        let context = crate::fonts::test_context();
        let path = root.join("animated.gif");
        app.tabs.open_new(path.clone(), MediaKind::Image);
        app.path = Some(path);
        app.media_kind = Some(MediaKind::Image);
        app.ui_context = Some(context.clone());
        app.image = Some(
            ImagePresentation::from_named_frame(
                &context,
                "animation",
                decoded(3, 0),
                0,
                TextureOptions::LINEAR,
            )
            .expect("image"),
        );
        app.palette_open = true;
        app.dispatch(CommandId::ToggleTimeline);
        assert!(
            !app.palette_open,
            "palette activation completes before animation dispatch"
        );
        assert!(app.animation_timeline_visible());
        assert!(!app.timeline_is_visible(), "time editing stays unavailable");
        let status = context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                app.draw_status_bar(ui, &mut vec![], &mut vec![]);
            },
        );
        let clock = status
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().contains("(1/3)") => Some(text),
                _ => None,
            })
            .expect("animation clock");
        assert!(
            clock
                .galley
                .job
                .sections
                .iter()
                .all(|section| section.format.font_id
                    == egui::FontId::proportional(chrome::TEXT_LG)
                    && (section.format.color == chrome::MUTED
                        || section.format.color == egui::Color32::PLACEHOLDER))
        );
        let enabled = |id, context| {
            crate::command_definitions()
                .iter()
                .find(|definition| definition.id == id)
                .expect("command")
                .is_enabled(context)
        };
        for id in [
            CommandId::SelectAll,
            CommandId::ApplyCrop,
            CommandId::FlipHorizontal,
            CommandId::TogglePause,
            CommandId::NextVideoFrame,
        ] {
            assert!(enabled(id, app.command_context()), "{id:?}");
        }
        for id in [
            CommandId::ToggleReadingMode,
            CommandId::DeleteTimeSelection,
            CommandId::KeepTimeSelection,
            CommandId::EditSpeed,
        ] {
            assert!(!enabled(id, app.command_context()), "{id:?}");
        }
        app.process_shortcut(crate::KeyStroke {
            key: crate::Key::ArrowRight,
            modifiers: Default::default(),
        });
        assert_eq!(app.image.as_ref().expect("image").frame_index, 1);
        assert!(app.image.as_ref().expect("image").next_frame_at.is_none());
        app.install_edited_image(decoded(3, 0))
            .expect("edited image");
        assert_eq!(app.image.as_ref().expect("image").frame_index, 1);
        assert!(app.image.as_ref().expect("image").next_frame_at.is_none());
        app.dispatch(CommandId::TogglePause);
        assert!(app.image.as_ref().expect("image").next_frame_at.is_some());
        app.repeat_media_shortcut(crate::KeyStroke {
            key: crate::Key::Space,
            modifiers: Default::default(),
        });
        assert!(
            app.image.as_ref().expect("image").next_frame_at.is_some(),
            "held Space does not toggle repeatedly"
        );
        app.dispatch(CommandId::ToggleTimeline);
        assert!(!app.animation_timeline_visible());
        app.reading_mode = true;
        assert!(!enabled(CommandId::ToggleTimeline, app.command_context()));
    }

    #[test]
    fn timeline_virtualizes_frames_and_accepts_an_ordinary_wheel() {
        let Some(_root) = crate::tests::isolated_test_root(
            "image_animation::tests::timeline_virtualizes_frames_and_accepts_an_ordinary_wheel",
        ) else {
            return;
        };
        let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
        let context = crate::fonts::test_context();
        app.image = Some(
            ImagePresentation::from_named_frame(
                &context,
                "animation",
                decoded(100, 0),
                0,
                TextureOptions::LINEAR,
            )
            .expect("image"),
        );
        app.media_kind = Some(MediaKind::Image);
        app.timeline_open = true;
        let frame = |app: &mut Application<fn(AppEvent)>, events| {
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600.0, 400.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_animation_timeline(ui),
            )
        };
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(egui::pos2(300.0, 350.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -600.0),
                    modifiers: egui::Modifiers::NONE,
                    phase: egui::TouchPhase::Move,
                },
            ],
        );
        for _ in 0..30 {
            frame(&mut app, vec![]);
        }
        let panel = app.timeline_panel_id();
        let anchor = panel.with("animation-viewport");
        let before = context
            .data(|data| data.get_temp::<(f32, f32, f32)>(anchor))
            .expect("viewport");
        let center = (before.2 + before.1 * 0.5 - 8.0) / before.0;
        // Simulate a height update at the same nonzero scroll position.
        context.data_mut(|data| {
            data.insert_persisted(
                panel,
                egui::containers::panel::PanelState {
                    outer_rect: Rect::from_min_max(
                        egui::pos2(0.0, 280.0),
                        egui::pos2(600.0, 400.0),
                    ),
                },
            )
        });
        let output = frame(&mut app, vec![]);
        let after = context
            .data(|data| data.get_temp::<(f32, f32, f32)>(anchor))
            .expect("resized viewport");
        assert!(((after.2 + after.1 * 0.5 - 8.0) / after.0 - center).abs() < 0.02);
        assert!(
            output.shapes.iter().any(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.brush.is_some() => {
                    let bounds = rect.rect;
                    bounds.left() < 8.0 && bounds.right() > 0.0 && shape.clip_rect.left() <= 0.0
                }
                _ => false,
            }),
            "scrolled frames paint through the side padding to the screen edge"
        );
        app.ui_context = Some(context.clone());
        let hidden_seek = context.run_ui(egui::RawInput::default(), |_| {
            app.draw_seek_bar(
                &context,
                Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 24.0)),
                None,
                &mut vec![],
            );
        });
        assert!(
            hidden_seek.shapes.is_empty(),
            "animation transport hides folder seeking"
        );
        let thumbnails = &app
            .image
            .as_ref()
            .expect("image")
            .animation_transport
            .as_ref()
            .expect("transport")
            .thumbnails;
        assert!(thumbnails.len() < 12, "only viewport frames have textures");
        assert!(
            !thumbnails.contains_key(&0),
            "ordinary vertical wheel scrolls horizontal frames"
        );
        assert!(thumbnails.keys().any(|index| *index > 8));
    }
}
