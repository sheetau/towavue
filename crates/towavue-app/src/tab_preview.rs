use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use egui::{TextureHandle, TextureOptions};
use towavue_core::{MediaKind, Tab, TabId, TabSet};
use towavue_runtime_windows::{LatestTask, PreviewCache, PreviewImage};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub tab: TabId,
    pub path: PathBuf,
    pub kind: MediaKind,
    pub position: Duration,
}

struct LastPosition {
    path: PathBuf,
    position: Duration,
}

pub struct TabPreview {
    worker: LatestTask,
    generation: u64,
    target: Option<Target>,
    texture: Option<Result<TextureHandle, String>>,
    positions: BTreeMap<TabId, LastPosition>,
}

impl TabPreview {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            worker: LatestTask::new("towavue-tab-preview")?,
            generation: 0,
            target: None,
            texture: None,
            positions: BTreeMap::new(),
        })
    }

    pub fn record(
        &mut self,
        tabs: &TabSet,
        path: Option<&Path>,
        position: Duration,
        duration: Option<Duration>,
        timeline: Option<&towavue_core::EditTimeline>,
    ) {
        self.positions
            .retain(|id, _| tabs.tabs().iter().any(|tab| tab.id == *id));
        if let Some(tab) = tabs.active()
            && Some(tab.target.current_path()) == path
            && tab.target.media_kind() == MediaKind::Video
        {
            let position = if let Some(plan) = timeline {
                let duration = Duration::from_nanos(plan.duration().as_nanoseconds() as u64);
                plan.source_time(crate::media_time(sample_time(position, Some(duration))))
                    .map_or(Duration::ZERO, |source| {
                        Duration::from_nanos(source.as_nanoseconds() as u64)
                    })
            } else {
                sample_time(position, duration)
            };
            self.positions.insert(
                tab.id,
                LastPosition {
                    path: tab.target.current_path().to_owned(),
                    position,
                },
            );
        }
    }

    pub fn target(&self, tab: &Tab) -> Target {
        let path = tab.target.current_path().to_owned();
        let position = self
            .positions
            .get(&tab.id)
            .filter(|last| last.path == path)
            .map_or(Duration::ZERO, |last| last.position);
        Target {
            tab: tab.id,
            path,
            kind: tab.target.media_kind(),
            position,
        }
    }

    pub fn clear(&mut self) {
        if self.target.take().is_some() {
            self.worker.clear();
            self.generation = self.generation.wrapping_add(1);
            self.texture = None;
        }
    }

    pub fn request<N>(&mut self, target: Option<Target>, cache: &PreviewCache, notify: Arc<N>)
    where
        N: Fn(crate::AppEvent) + Send + Sync + 'static,
    {
        if self.target == target {
            return;
        }
        self.clear();
        let Some(target) = target else {
            return;
        };
        self.target = Some(target.clone());
        let generation = self.generation;
        let cache = cache.clone();
        self.worker.submit(move |cancellation| {
            let cache = cache.cancellable(cancellation.clone());
            let result = if target.kind == MediaKind::Video {
                cache.thumbnail(&target.path, target.position, 240)
            } else {
                cache
                    .filmstrip(&target.path, target.kind)
                    .map(|media| media.image)
            }
            .map_err(|error| error.to_string());
            if !cancellation.is_cancelled() {
                notify(crate::AppEvent::TabPreview(target, generation, result));
            }
        });
    }

    pub fn finish(
        &mut self,
        context: &egui::Context,
        target: Target,
        generation: u64,
        result: Result<PreviewImage, String>,
    ) {
        if generation != self.generation || self.target.as_ref() != Some(&target) {
            return;
        }
        self.texture = Some(result.and_then(|image| {
            let limit = context.input(|input| input.max_texture_side);
            if image.width as usize > limit || image.height as usize > limit {
                return Err("Preview exceeds texture limit".into());
            }
            Ok(context.load_texture(
                format!("tab-preview:{}", target.path.display()),
                egui::ColorImage::from_rgba_unmultiplied(
                    [image.width as usize, image.height as usize],
                    &image.rgba,
                ),
                TextureOptions::LINEAR,
            ))
        }));
        context.request_repaint();
    }

    pub fn show(&self, response: &egui::Response, target: &Target) {
        let mut tooltip = egui::Tooltip::for_enabled(response).width(240.0);
        tooltip.popup = tooltip
            .popup
            .at_position(response.rect.left_bottom())
            .align(egui::RectAlign::BOTTOM_START);
        tooltip.show(|ui| {
            ui.set_max_width(240.0);
            let texture = (self.target.as_ref() == Some(target))
                .then_some(self.texture.as_ref())
                .flatten();
            match texture {
                Some(Ok(texture)) => {
                    ui.add(
                        egui::Image::new((texture.id(), texture.size_vec2()))
                            .max_size(egui::vec2(240.0, 160.0)),
                    );
                }
                result => {
                    ui.label(if result.is_some() {
                        "No preview"
                    } else {
                        "Loading preview…"
                    });
                }
            }
            if target.kind == MediaKind::Video {
                ui.monospace(format!(
                    "Preview near {}",
                    crate::format_time(crate::media_time(target.position))
                ));
            }
            ui.add(egui::Label::new(target.path.display().to_string()).wrap());
        });
    }
}

fn sample_time(position: Duration, duration: Option<Duration>) -> Duration {
    match duration.filter(|duration| !duration.is_zero()) {
        Some(duration) => {
            let bucket =
                ((position.as_secs_f64() / duration.as_secs_f64() * 20.0).floor() as u64).min(19);
            duration.mul_f64((bucket as f64 + 0.5) / 20.0)
        }
        None => Duration::from_secs(position.as_secs()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_targets_use_seek_buckets_and_last_observed_tab_identity() {
        let duration = Duration::from_secs(100);
        for (position, expected) in [
            (0.0, 2.5),
            (4.99, 2.5),
            (5.0, 7.5),
            (37.0, 37.5),
            (100.0, 97.5),
            (200.0, 97.5),
        ] {
            assert_eq!(
                sample_time(Duration::from_secs_f64(position), Some(duration)),
                Duration::from_secs_f64(expected)
            );
        }
        assert_eq!(
            sample_time(Duration::from_millis(3700), None),
            Duration::from_secs(3)
        );
        let mut preview = TabPreview::new().expect("worker");
        let mut tabs = TabSet::default();
        let video = tabs.open_new("video.mp4".into(), MediaKind::Video);
        preview.record(
            &tabs,
            Some(Path::new("video.mp4")),
            Duration::from_secs(37),
            Some(duration),
            None,
        );
        tabs.open_new("image.png".into(), MediaKind::Image);
        preview.record(
            &tabs,
            Some(Path::new("image.png")),
            Duration::ZERO,
            None,
            None,
        );
        assert_eq!(
            preview.target(&tabs.tabs()[0]).position,
            Duration::from_millis(37500)
        );
        assert_eq!(preview.target(&tabs.tabs()[1]).position, Duration::ZERO);
        tabs.activate(video);
        tabs.active_mut()
            .expect("video tab")
            .target
            .set_current_path("changed.mp4".into(), MediaKind::Video);
        assert_eq!(
            preview.target(tabs.active().expect("active")).position,
            Duration::ZERO
        );
        let active = tabs.clone();
        preview.record(
            &tabs,
            Some(Path::new("stale.mp4")),
            Duration::from_secs(50),
            Some(duration),
            None,
        );
        assert_eq!(
            preview.target(tabs.active().expect("active")).position,
            Duration::ZERO
        );
        assert_eq!(
            tabs, active,
            "recording a preview does not activate or modify tabs"
        );
    }

    #[test]
    fn edited_preview_samples_in_edited_time_then_maps_to_the_source() {
        use towavue_core::{EditTimeline, MediaTime, PlaybackRange, TimeRange, TimelineEdit};
        let time = |seconds: i64| MediaTime::from_nanoseconds(seconds * 1_000_000_000);
        let mut plan = EditTimeline::new(time(100), PlaybackRange::default()).expect("plan");
        assert!(plan.apply(TimelineEdit::Delete(
            TimeRange::new(time(20), time(60)).expect("range")
        )));
        let mut preview = TabPreview::new().expect("worker");
        let mut tabs = TabSet::default();
        tabs.open_new("edited.mp4".into(), MediaKind::Video);
        preview.record(
            &tabs,
            Some(Path::new("edited.mp4")),
            Duration::from_secs(30),
            Some(Duration::from_secs(100)),
            Some(&plan),
        );
        let target = preview.target(tabs.active().expect("tab"));
        assert_eq!(target.position, Duration::from_millis(71500));
        tabs.open_new("other.png".into(), MediaKind::Image);
        assert_eq!(preview.target(&tabs.tabs()[0]), target);
    }

    #[test]
    fn preview_results_follow_latest_target_and_do_not_retry_each_frame() {
        let root = std::env::temp_dir().join(format!("towavue-tab-preview-{}", std::process::id()));
        let cache = PreviewCache::new(root.clone()).expect("cache");
        let mut tabs = TabSet::default();
        tabs.open_new(root.join("first.png"), MediaKind::Image);
        tabs.open_new(root.join("second.png"), MediaKind::Image);
        let mut preview = TabPreview::new().expect("worker");
        let first = preview.target(&tabs.tabs()[0]);
        let second = preview.target(&tabs.tabs()[1]);
        let (sent, events) = std::sync::mpsc::channel();
        let notify = Arc::new(move |event| {
            let _ = sent.send(event);
        });
        preview.request(Some(first.clone()), &cache, notify.clone());
        let stale = preview.generation;
        preview.request(Some(second.clone()), &cache, notify.clone());
        let current = preview.generation;
        let context = crate::fonts::test_context();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while preview.texture.is_none() {
            let event = events
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("preview event");
            if let crate::AppEvent::TabPreview(target, generation, result) = event {
                preview.finish(&context, target, generation, result);
            }
        }
        assert!(matches!(preview.texture, Some(Err(_))));
        preview.finish(
            &context,
            first,
            stale,
            Ok(PreviewImage {
                width: 1,
                height: 1,
                rgba: vec![255; 4],
            }),
        );
        assert!(
            matches!(preview.texture, Some(Err(_))),
            "late target cannot replace current error"
        );
        preview.request(Some(second.clone()), &cache, notify.clone());
        assert_eq!(preview.generation, current);
        preview.request(None, &cache, notify);
        preview.finish(
            &context,
            second,
            current,
            Ok(PreviewImage {
                width: 1,
                height: 1,
                rgba: vec![255; 4],
            }),
        );
        assert!(preview.target.is_none() && preview.texture.is_none());
        std::fs::remove_dir(root).expect("remove empty owned cache");
    }

    #[test]
    fn hovering_a_dirty_tab_draws_preview_below_it_without_media_actions() {
        let mut app = crate::Application::new(None, |_| {}).expect("headless app");
        let context = crate::fonts::test_context();
        context.global_style_mut(|style| {
            crate::chrome::style(style);
            style.interaction.tooltip_delay = 0.0;
            style.interaction.show_tooltips_only_when_still = false;
        });
        app.ui_context = Some(context.clone());
        let path = PathBuf::from("preview-image.png");
        app.tabs.open_new(path.clone(), MediaKind::Image);
        app.path = Some(path);
        app.media_kind = Some(MediaKind::Image);
        app.push_edit(towavue_core::EditOperation::RotateClockwise);
        let tabs = app.tabs.clone();
        let history = app.edits[&tabs.active().expect("active").id]
            .operations()
            .to_vec();
        let target = app.tab_preview.target(tabs.active().expect("tab"));
        let texture = context.load_texture(
            "fixture-tab-preview",
            egui::ColorImage::filled([64, 32], egui::Color32::RED),
            TextureOptions::LINEAR,
        );
        let texture_id = texture.id();
        let frame = |app: &mut crate::Application<_>, pointer, time| {
            context.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events: vec![egui::Event::PointerMoved(pointer)],
                    ..Default::default()
                },
                |ui| {
                    let mut actions = Vec::new();
                    app.draw_top_bar(ui, &mut actions);
                    assert!(actions.is_empty(), "hover must not dispatch actions");
                },
            )
        };
        for index in 0..3 {
            frame(&mut app, egui::pos2(90.0, 16.0), index as f64 * 0.1);
        }
        app.tab_preview.target = Some(target);
        app.tab_preview.texture = Some(Ok(texture));
        let mut output = egui::FullOutput::default();
        for index in 3..8 {
            output = frame(&mut app, egui::pos2(90.0, 16.0), index as f64 * 0.1);
        }
        let rect = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.fill_texture_id() == texture_id => Some(rect.rect),
                _ => None,
            })
            .expect("tab thumbnail shown");
        assert!(rect.top() >= 32.0 && rect.right() <= 960.0);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().contains("preview-image.png"))));
        assert_eq!(app.tabs, tabs);
        assert_eq!(
            app.edits[&tabs.active().expect("active").id].operations(),
            history
        );
        assert!(app.pending_guard.is_none() && app.session.is_none());
        frame(&mut app, egui::pos2(400.0, 300.0), 1.0);
        assert!(app.tab_preview.target.is_none() && app.tab_preview.texture.is_none());
        for overlay in 0..3 {
            frame(&mut app, egui::pos2(90.0, 16.0), 2.0 + overlay as f64);
            assert!(app.tab_preview.target.is_some());
            app.palette_open = overlay == 0;
            app.grid_open = overlay == 1;
            app.filmstrip_open = overlay == 2;
            frame(&mut app, egui::pos2(90.0, 16.0), 2.5 + overlay as f64);
            assert!(app.tab_preview.target.is_none() && app.tab_preview.texture.is_none());
            app.palette_open = false;
            app.grid_open = false;
            app.filmstrip_open = false;
        }
    }
}
