use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use egui::{TextureHandle, TextureOptions};
use towavue_core::{MediaKind, Tab, TabId, TabSet};
use towavue_runtime_windows::{LatestTask, PreviewCache, PreviewImage};

#[cfg(test)]
mod navigation_tests;

#[cfg(test)]
mod reading_tests;

pub(super) enum RetainedPreview {
    Image(TextureHandle),
    Reading {
        pages: Vec<(Option<TextureHandle>, egui::Vec2)>,
        settings: towavue_core::ReadingSettings,
    },
}

impl RetainedPreview {
    fn image(&self) -> Option<&TextureHandle> {
        match self {
            Self::Image(image) => Some(image),
            Self::Reading { .. } => None,
        }
    }

    fn show_reading(
        &self,
        ui: &mut egui::Ui,
        control: Option<&egui::Response>,
    ) -> Option<egui::Rect> {
        let Self::Reading { pages, settings } = self else {
            return None;
        };
        let sizes: Vec<_> = pages.iter().map(|page| page.1).collect();
        let rects = crate::reading_page_rects(
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(240.0, 160.0)),
            &sizes,
            settings.axis,
            settings.reversed,
        );
        let spread = rects
            .iter()
            .copied()
            .reduce(egui::Rect::union)
            .expect("reading pages");
        let natural = spread.height().max(40.0);
        let height = control.map_or(natural, |response| card_viewport_height(response, natural));
        let scale = (height / spread.height()).min(1.0);
        let (bounds, _) = ui.allocate_exact_size(egui::vec2(240.0, height), egui::Sense::hover());
        let rect = egui::Rect::from_center_size(bounds.center(), spread.size() * scale);
        for ((texture, _), page) in pages.iter().zip(rects) {
            if let Some(texture) = texture {
                let page = if scale == 1.0 {
                    page.translate(rect.min - spread.min)
                } else {
                    egui::Rect::from_min_max(
                        rect.min + (page.min - spread.min) * scale,
                        rect.min + (page.max - spread.min) * scale,
                    )
                };
                crate::media_preview::image(
                    ui,
                    texture.id(),
                    page,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    bounds,
                );
            }
        }
        Some(bounds)
    }
}

impl<N: Fn(crate::AppEvent) + Send + Sync + 'static> crate::Application<N> {
    pub(super) fn retained_tab_preview(&self, tab: TabId, path: &Path) -> Option<RetainedPreview> {
        let (image, others, reading, settings, snapshot, loading, focus) =
            if self.displayed_tab == Some(tab) && self.path.as_deref() == Some(path) {
                (
                    self.image.as_ref(),
                    &self.reading_pages,
                    self.reading_mode,
                    self.reading_settings,
                    self.folder_snapshot.as_ref(),
                    self.image_loading,
                    self.reading_focus.as_ref(),
                )
            } else {
                let saved = self.retained_images.get(&tab).filter(|saved| {
                    saved.path == path && saved.graphics_epoch == self.graphics_epoch
                })?;
                (
                    saved.image.as_ref(),
                    &saved.reading_pages,
                    saved.reading_mode,
                    saved.reading_settings,
                    saved.folder_snapshot.as_ref(),
                    saved.resume_loading,
                    saved.reading_focus.as_ref(),
                )
            };
        if !reading {
            return image.map(|image| RetainedPreview::Image(image.texture.clone()));
        }
        let pending_size = image.map_or(egui::Vec2::splat(1.0), |image| image.texture.size_vec2());
        let mut pages: Vec<_> = others
            .iter()
            .map(|page| match page {
                Ok(image) => (Some(image.texture.clone()), image.texture.size_vec2()),
                Err(_) => (None, egui::Vec2::splat(1.0)),
            })
            .collect();
        // Loader results omit the current source and follow the chosen reading sequence.
        // Insert that source before applying the reading axis/order to the joined layout.
        let ordered = snapshot
            .map(|snapshot| {
                snapshot.reading_items(
                    focus.map_or(path, |item| item.path.as_path()),
                    towavue_core::ReadingSettings {
                        reversed: false,
                        ..settings
                    },
                )
            })
            .unwrap_or_default();
        let index = ordered
            .iter()
            .position(|item| item.path == path)
            .or_else(|| ordered.is_empty().then_some(0));
        if loading {
            pages.resize(
                ordered.len().max(1) - usize::from(index.is_some()),
                (None, pending_size),
            );
        }
        if let Some(index) = index {
            pages.insert(
                index.min(pages.len()),
                (image.map(|image| image.texture.clone()), pending_size),
            );
        }
        pages
            .iter()
            .any(|page| page.0.is_some())
            .then_some(RetainedPreview::Reading { pages, settings })
    }
}

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
    sheet_uv: Option<egui::Rect>,
    sheet_layout: Option<towavue_runtime_windows::VideoSheetLayout>,
    positions: BTreeMap<TabId, LastPosition>,
    input_path: Option<PathBuf>,
}

impl TabPreview {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            worker: LatestTask::new("towavue-tab-preview")?,
            generation: 0,
            target: None,
            texture: None,
            sheet_uv: None,
            sheet_layout: None,
            positions: BTreeMap::new(),
            input_path: None,
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
            let position = source_sample_time(position, duration, timeline);
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

    pub fn target_with_playback(
        &self,
        tab: &Tab,
        saved: Option<&crate::playback_tab::RetainedPlaybackTab>,
    ) -> Target {
        let mut target = self.target(tab);
        if let Some(saved) = saved
            && target.kind == MediaKind::Video
            && saved.kind == MediaKind::Video
            && saved.path == target.path
        {
            target.position = source_sample_time(
                Duration::from_nanos(saved.position().as_nanoseconds().max(0) as u64),
                saved.duration,
                saved
                    .session
                    .as_ref()
                    .and_then(towavue_runtime_windows::PlaybackSession::timeline),
            );
        }
        target
    }

    pub fn clear(&mut self) {
        if self.target.take().is_some() {
            self.worker.clear();
            self.generation = self.generation.wrapping_add(1);
            self.texture = None;
            self.sheet_uv = None;
            self.sheet_layout = None;
        }
    }

    #[cfg(test)]
    pub fn request<N>(&mut self, target: Option<Target>, cache: &PreviewCache, notify: Arc<N>)
    where
        N: Fn(crate::AppEvent) + Send + Sync + 'static,
    {
        let input = target
            .as_ref()
            .map(|target| towavue_runtime_windows::MediaInput::new(target.path.clone()));
        self.request_input(target, input, cache, notify);
    }

    pub fn request_input<N>(
        &mut self,
        target: Option<Target>,
        input: Option<towavue_runtime_windows::MediaInput>,
        cache: &PreviewCache,
        notify: Arc<N>,
    ) where
        N: Fn(crate::AppEvent) + Send + Sync + 'static,
    {
        if self.input_path.as_deref()
            != input
                .as_ref()
                .map(towavue_runtime_windows::MediaInput::path)
        {
            self.clear();
            self.input_path = input.as_ref().map(|input| input.path().to_owned());
        }
        if self.target == target {
            return;
        }
        if let (Some(previous), Some(next)) = (&self.target, &target)
            && previous.tab == next.tab
            && previous.path == next.path
            && previous.kind == MediaKind::Video
            && next.kind == MediaKind::Video
            && matches!(self.texture, Some(Ok(_)))
            && let Some([left, top, right, bottom]) = self
                .sheet_layout
                .and_then(|layout| layout.uv(next.position))
        {
            self.sheet_uv = Some(egui::Rect::from_min_max(
                egui::pos2(left, top),
                egui::pos2(right, bottom),
            ));
            self.target = target;
            return;
        }
        self.clear();
        let Some(target) = target else {
            return;
        };
        let Some(input) = input else {
            return;
        };
        self.target = Some(target.clone());
        let generation = self.generation;
        let cache = cache.clone();
        self.worker.submit(move |cancellation| {
            let cache = cache.cancellable(cancellation.clone());
            if target.kind == MediaKind::Video {
                let mut first_preview_sent = false;
                let result = cache
                    .duration(input.path())
                    .and_then(|duration| {
                        let layout = towavue_runtime_windows::VideoSheetLayout::for_position(
                            duration,
                            target.position,
                        )
                        .ok_or(towavue_runtime_windows::PreviewError::InvalidDuration)?;
                        if let Some(sheet) = cache.cached_video_sheet(input.path(), layout)? {
                            return Ok(sheet);
                        }
                        let first = cache
                            .thumbnail(input.path(), target.position, 240)
                            .map_err(|error| error.to_string());
                        if !cancellation.is_cancelled() {
                            notify(crate::AppEvent::TabPreview(
                                target.clone(),
                                generation,
                                first,
                            ));
                            first_preview_sent = true;
                        }
                        cache.video_sheet(input.path(), layout)
                    })
                    .map_err(|error| error.to_string());
                if !cancellation.is_cancelled() {
                    match result {
                        Ok(sheet) => notify(crate::AppEvent::TabVideoSheet(
                            target,
                            generation,
                            Ok(sheet),
                        )),
                        Err(_) if !first_preview_sent => {
                            let result = cache
                                .thumbnail(input.path(), target.position, 240)
                                .map_err(|error| error.to_string());
                            if !cancellation.is_cancelled() {
                                notify(crate::AppEvent::TabPreview(target, generation, result));
                            }
                        }
                        Err(_) => {}
                    }
                }
                return;
            }
            let result = cache
                .filmstrip(input.path(), target.kind)
                .map(|media| media.image)
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
    ) -> bool {
        if generation != self.generation || self.target.as_ref() != Some(&target) {
            return false;
        }
        self.sheet_uv = None;
        self.sheet_layout = None;
        self.texture = Some(result.and_then(|image| {
            let limit = context.input(|input| input.max_texture_side);
            if image.width as usize > limit || image.height as usize > limit {
                return Err("Preview exceeds texture limit".into());
            }
            Ok(context.load_texture(
                format!("tab-preview:{}", target.path.display()),
                crate::image_color::preview_color_image(&image),
                TextureOptions::LINEAR,
            ))
        }));
        context.request_repaint();
        true
    }

    pub fn finish_sheet(
        &mut self,
        context: &egui::Context,
        target: Target,
        generation: u64,
        result: Result<towavue_runtime_windows::VideoPreviewSheet, String>,
    ) {
        if generation != self.generation || self.target.as_ref() != Some(&target) {
            return;
        }
        let mut uv = None;
        let mut layout = None;
        let pixels = result.and_then(|sheet| {
            let [left, top, right, bottom] = sheet
                .layout
                .uv(target.position)
                .ok_or_else(|| "Video sheet does not contain the requested position".to_owned())?;
            uv = Some(egui::Rect::from_min_max(
                egui::pos2(left, top),
                egui::pos2(right, bottom),
            ));
            layout = Some(sheet.layout);
            Ok(sheet.image)
        });
        self.finish(context, target, generation, pixels);
        if matches!(self.texture, Some(Ok(_))) {
            self.sheet_uv = uv;
            self.sheet_layout = layout;
        }
    }

    #[cfg(test)]
    pub fn show(
        &self,
        response: &egui::Response,
        target: &Target,
        retained: Option<&RetainedPreview>,
    ) {
        self.show_with_transport(response, target, retained, None, None);
    }

    pub fn show_with_transport(
        &self,
        response: &egui::Response,
        target: &Target,
        retained: Option<&RetainedPreview>,
        transport: Option<&crate::preview_transport::Transport>,
        folder: Option<&crate::image_tab_preview::FolderPosition>,
    ) -> Option<crate::preview_transport::Action> {
        crate::media_preview::Preview::tab(response)
            .show(|ui| {
                ui.set_max_width(240.0);
                if let Some(thumbnail) =
                    retained.and_then(|preview| preview.show_reading(ui, folder.map(|_| response)))
                {
                    let action = folder.and_then(|folder| folder.show(ui, thumbnail));
                    crate::media_preview::caption(ui, |ui| {
                        ui.add(egui::Label::new(target.path.display().to_string()).wrap());
                    });
                    return action;
                }
                let cached = (self.target.as_ref() == Some(target))
                    .then_some(self.texture.as_ref())
                    .flatten();
                let texture = retained
                    .and_then(RetainedPreview::image)
                    .map(Ok)
                    .or_else(|| cached.map(Result::as_ref));
                let sheet_uv = if retained.is_some() {
                    None
                } else {
                    self.sheet_uv
                };
                let mut thumbnail = None;
                if let Some(Ok(texture)) = texture {
                    let size =
                        sheet_uv.map_or_else(|| texture.size_vec2(), |_| egui::vec2(240.0, 160.0));
                    let scale = (240.0 / size.x).min(160.0 / size.y).min(1.0);
                    let size = size * scale;
                    // Video sheets contain padded 240x160 cells. Keep that same
                    // viewport for the earlier unpadded thumbnail, otherwise a
                    // retained shorter height shrinks the whole sheet cell.
                    let height = if target.kind == MediaKind::Video {
                        160.0
                    } else if transport.is_some() || folder.is_some() {
                        size.y.max(40.0)
                    } else {
                        size.y
                    };
                    let height = if transport.is_some() || folder.is_some() {
                        card_viewport_height(response, height)
                    } else {
                        height
                    };
                    let size = size * (height / size.y).min(1.0);
                    let (bounds, _) =
                        ui.allocate_exact_size(egui::vec2(240.0, height), egui::Sense::hover());
                    thumbnail = Some(bounds);
                    crate::media_preview::image(
                        ui,
                        texture.id(),
                        egui::Rect::from_center_size(bounds.center(), size),
                        sheet_uv.unwrap_or(egui::Rect::from_min_max(
                            egui::Pos2::ZERO,
                            egui::pos2(1.0, 1.0),
                        )),
                        bounds,
                    );
                }
                if thumbnail.is_none() && (transport.is_some() || folder.is_some()) {
                    thumbnail = Some(
                        ui.allocate_exact_size(
                            egui::vec2(
                                240.0,
                                card_viewport_height(
                                    response,
                                    if target.kind == MediaKind::Audio {
                                        40.0
                                    } else {
                                        160.0
                                    },
                                ),
                            ),
                            egui::Sense::hover(),
                        )
                        .0,
                    );
                }
                let action = transport
                    .zip(thumbnail)
                    .and_then(|(transport, thumbnail)| transport.show(ui, thumbnail))
                    .or_else(|| {
                        folder
                            .zip(thumbnail)
                            .and_then(|(folder, thumbnail)| folder.show(ui, thumbnail))
                    });
                crate::media_preview::caption(ui, |ui| {
                    if texture.is_some_and(|texture| texture.is_err()) {
                        ui.label("No preview");
                    }
                    if target.kind == MediaKind::Video {
                        ui.label(format!(
                            "Preview near {}",
                            crate::format_time(crate::media_time(target.position))
                        ));
                    }
                    ui.add(egui::Label::new(target.path.display().to_string()).wrap());
                });
                action
            })
            .and_then(|output| output.inner)
    }
}

#[derive(Clone, Copy)]
struct CardViewportHeight {
    frame: u64,
    height: f32,
}

fn card_viewport_height(response: &egui::Response, natural: f32) -> f32 {
    let context = &response.ctx;
    let id = response.id.with("card-viewport-height");
    let frame = context.cumulative_frame_nr();
    let inside_card = context
        .pointer_hover_pos()
        .is_some_and(|point| !response.interact_rect.contains(point));
    // Preview::tab has already validated this card's hover/captured-seek owner.
    // Keep its controls stationary across a media/preview replacement while the
    // pointer is off the source tab; otherwise a shorter preview can remove
    // the card from beneath the pointer. Reopening or returning to the tab allows
    // the natural preview size again. Only geometry is retained, never old pixels.
    context.data_mut(|data| {
        let previous = data.get_temp::<CardViewportHeight>(id);
        let height = previous
            .filter(|previous| inside_card && previous.frame.saturating_add(1) >= frame)
            .map_or(natural, |previous| previous.height);
        data.insert_temp(id, CardViewportHeight { frame, height });
        height
    })
}

fn source_sample_time(
    position: Duration,
    duration: Option<Duration>,
    timeline: Option<&towavue_core::EditTimeline>,
) -> Duration {
    let source = timeline.map_or(position, |plan| {
        plan.source_time(crate::media_time(position))
            .map_or(Duration::ZERO, |source| {
                Duration::from_nanos(source.as_nanoseconds() as u64)
            })
    });
    sample_time(source, duration)
}

fn sample_time(position: Duration, duration: Option<Duration>) -> Duration {
    match duration.filter(|duration| !duration.is_zero()) {
        Some(duration) => {
            towavue_runtime_windows::VideoSheetLayout::for_position(duration, position)
                .and_then(|layout| layout.sample_position(position))
                .unwrap_or(position)
        }
        None => Duration::from_secs(position.as_secs()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "preview acceptance cost; run in Release without concurrent builds"]
    fn preview_acceptance_reports_packed_color_cost() -> Result<(), &'static str> {
        if cfg!(debug_assertions) {
            return Err("use Release");
        }
        let mut tabs = TabSet::default();
        tabs.open_new("generated-preview.mp4".into(), MediaKind::Video);
        let mut preview = TabPreview::new().expect("worker");
        let target = preview.target(tabs.active().expect("tab"));
        let layout = towavue_runtime_windows::VideoSheetLayout::for_position(
            Duration::from_secs(100),
            Duration::ZERO,
        )
        .expect("layout");
        for (width, height) in [(240, 160), (960, 640)] {
            for opaque in [true, false] {
                let source = PreviewImage {
                    width,
                    height,
                    rgba: (0..width * height)
                        .flat_map(|n| {
                            [
                                n as u8,
                                (n / 3) as u8,
                                (n / 7) as u8,
                                if opaque { 255 } else { n as u8 },
                            ]
                        })
                        .collect::<Vec<_>>()
                        .into(),
                };
                let expected =
                    egui::ImageData::Color(Arc::new(egui::ColorImage::from_rgba_unmultiplied(
                        [width as usize, height as usize],
                        &source.rgba,
                    )));
                for packed in [false, true, true, false] {
                    crate::image_color::PACKED_PREVIEW_COLORS.set(packed);
                    let mut times = Vec::new();
                    for _ in 0..15 {
                        preview.clear();
                        let context = egui::Context::default();
                        let _ = context.tex_manager().write().take_delta();
                        preview.target = Some(target.clone());
                        let input = source.clone();
                        let started = std::time::Instant::now();
                        if width == 960 {
                            preview.finish_sheet(
                                &context,
                                target.clone(),
                                preview.generation,
                                Ok(towavue_runtime_windows::VideoPreviewSheet {
                                    layout,
                                    image: input,
                                }),
                            );
                        } else {
                            preview.finish(&context, target.clone(), preview.generation, Ok(input));
                        }
                        times.push(started.elapsed());
                        let delta = context.tex_manager().write().take_delta();
                        assert_eq!(delta.set.len(), 1, "one queued texture");
                        assert!(delta.set[0].1.image == expected, "all queued pixels match");
                        assert_eq!(preview.sheet_layout.is_some(), width == 960);
                        assert!(matches!(preview.texture, Some(Ok(_))));
                    }
                    times.sort();
                    eprintln!(
                        "PREVIEW_ACCEPT width={width} height={height} opaque={opaque} packed={packed} median_ms={:.4}",
                        times[7].as_secs_f64() * 1000.0
                    );
                }
            }
        }
        crate::image_color::PACKED_PREVIEW_COLORS.set(true);
        Ok(())
    }

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
    fn edited_preview_maps_current_time_before_selecting_a_source_sheet_cell() {
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
        assert_eq!(target.position, Duration::from_millis(72500));
        tabs.open_new("other.png".into(), MediaKind::Image);
        assert_eq!(preview.target(&tabs.tabs()[0]), target);
    }

    #[test]
    fn advancing_within_a_video_sheet_reuses_the_uploaded_texture() {
        let root =
            std::env::temp_dir().join(format!("towavue-tab-sheet-reuse-{}", std::process::id()));
        let cache = PreviewCache::new(root.clone()).expect("cache");
        let context = crate::fonts::test_context();
        let mut tabs = TabSet::default();
        tabs.open_new(root.join("missing.mp4"), MediaKind::Video);
        let mut preview = TabPreview::new().expect("worker");
        let mut target = preview.target(tabs.active().expect("tab"));
        let layout = towavue_runtime_windows::VideoSheetLayout::for_position(
            Duration::from_secs(100),
            Duration::ZERO,
        )
        .expect("layout");
        let sheet = || towavue_runtime_windows::VideoPreviewSheet {
            layout,
            image: PreviewImage {
                width: 960,
                height: 640,
                rgba: vec![255; 960 * 640 * 4].into(),
            },
        };
        preview.input_path = Some(target.path.clone());
        preview.target = Some(target.clone());
        preview.finish_sheet(&context, target.clone(), preview.generation, Ok(sheet()));
        let stale = target.clone();
        let generation = preview.generation;
        let texture_id = preview
            .texture
            .as_ref()
            .expect("result")
            .as_ref()
            .expect("texture")
            .id();
        let _ = context.tex_manager().write().take_delta();
        let notify = Arc::new(|_| {});
        for seconds in [7, 37, 79, 0] {
            target.position = Duration::from_secs(seconds);
            preview.request(Some(target.clone()), &cache, notify.clone());
            assert_eq!(preview.generation, generation, "no replacement job");
            assert_eq!(preview.target.as_ref(), Some(&target));
            assert_eq!(
                preview
                    .texture
                    .as_ref()
                    .expect("result")
                    .as_ref()
                    .expect("texture")
                    .id(),
                texture_id
            );
            let [left, top, right, bottom] = layout.uv(target.position).expect("cell");
            assert_eq!(
                preview.sheet_uv,
                Some(egui::Rect::from_min_max(
                    egui::pos2(left, top),
                    egui::pos2(right, bottom)
                ))
            );
        }
        target.position = Duration::from_secs(40);
        preview.request(Some(target.clone()), &cache, notify.clone());
        preview.finish_sheet(&context, stale, generation, Ok(sheet()));
        let delta = context.tex_manager().write().take_delta();
        assert!(
            delta.set.is_empty() && delta.free.is_empty(),
            "UV-only changes"
        );
        preview.request(None, &cache, notify.clone());
        assert!(preview.target.is_none() && preview.texture.is_none());
        assert_eq!(
            context.tex_manager().write().take_delta().free,
            vec![texture_id]
        );
        preview.finish_sheet(&context, target.clone(), generation, Ok(sheet()));
        assert!(
            preview.texture.is_none(),
            "late sheet cannot revive a closed hover"
        );

        for changed in 0..5 {
            preview.input_path = Some(target.path.clone());
            preview.target = Some(target.clone());
            preview.finish_sheet(&context, target.clone(), preview.generation, Ok(sheet()));
            let generation = preview.generation;
            let mut next = target.clone();
            match changed {
                0 => next.position = Duration::from_secs(80),
                1 => next.path = root.join("other.mp4"),
                2 => next.tab = tabs.open_new(next.path.clone(), MediaKind::Video),
                3 => next.kind = MediaKind::Image,
                _ => preview.input_path = Some(root.join("retained-original.mp4")),
            }
            preview.request(Some(next.clone()), &cache, notify.clone());
            assert_ne!(preview.generation, generation, "different sheet or owner");
            assert_eq!(preview.target, Some(next));
            assert!(preview.texture.is_none() && preview.sheet_uv.is_none());
            preview.clear();
        }
        drop(preview);
        std::fs::remove_dir(root).expect("remove empty owned cache");
    }

    #[test]
    fn failed_sheets_and_single_frames_cannot_enable_sheet_reuse() {
        let context = crate::fonts::test_context();
        let mut tabs = TabSet::default();
        tabs.open_new("video.mp4".into(), MediaKind::Video);
        let mut preview = TabPreview::new().expect("worker");
        let target = preview.target(tabs.active().expect("tab"));
        let layout = towavue_runtime_windows::VideoSheetLayout::for_position(
            Duration::from_secs(100),
            Duration::ZERO,
        )
        .expect("layout");
        let pixels = || PreviewImage {
            width: 1,
            height: 1,
            rgba: vec![255; 4].into(),
        };
        preview.target = Some(target.clone());
        for case in 0..4 {
            preview.finish_sheet(
                &context,
                target.clone(),
                preview.generation,
                Ok(towavue_runtime_windows::VideoPreviewSheet {
                    layout,
                    image: pixels(),
                }),
            );
            match case {
                0 => {
                    assert!(preview.finish(
                        &context,
                        target.clone(),
                        preview.generation,
                        Ok(pixels())
                    ));
                }
                _ => {
                    let result = match case {
                        1 => Err("decode failed".to_owned()),
                        2 => Ok(towavue_runtime_windows::VideoPreviewSheet {
                            layout,
                            image: PreviewImage {
                                width: context.input(|input| input.max_texture_side) as u32 + 1,
                                height: 1,
                                rgba: Vec::new().into(),
                            },
                        }),
                        _ => Ok(towavue_runtime_windows::VideoPreviewSheet {
                            layout: towavue_runtime_windows::VideoSheetLayout::for_position(
                                Duration::from_secs(100),
                                Duration::from_secs(80),
                            )
                            .expect("other sheet"),
                            image: pixels(),
                        }),
                    };
                    preview.finish_sheet(&context, target.clone(), preview.generation, result);
                }
            }
            assert!(preview.sheet_layout.is_none() && preview.sheet_uv.is_none());
            assert_eq!(matches!(preview.texture, Some(Ok(_))), case == 0);
        }
    }

    #[test]
    fn first_preview_completion_wakes_an_idle_native_window() {
        use crate::*;
        use winit::event::StartCause;
        use winit::platform::windows::EventLoopBuilderExtWindows;
        let Some(root) = crate::tests::isolated_test_root(
            "tab_preview::tests::first_preview_completion_wakes_an_idle_native_window",
        ) else {
            return;
        };
        struct Trial {
            app: Application<fn(AppEvent)>,
            target: Target,
            deadline: Instant,
            waiting: bool,
            redraws: Vec<bool>,
        }
        impl Trial {
            fn finish_case(&mut self, event_loop: &ActiveEventLoop, redrawn: bool) {
                self.redraws.push(redrawn);
                self.waiting = false;
                self.deadline = Instant::now() + Duration::from_millis(100);
                if self.redraws.len() == 5 {
                    event_loop.set_control_flow(ControlFlow::Poll);
                    event_loop.exit();
                }
            }
        }
        impl ApplicationHandler for Trial {
            fn resumed(&mut self, event_loop: &ActiveEventLoop) {
                self.app.window = Some(Arc::new(
                    event_loop
                        .create_window(
                            Window::default_attributes()
                                .with_title("towavue preview redraw verification")
                                .with_inner_size(winit::dpi::LogicalSize::new(480.0, 240.0))
                                .with_active(false),
                        )
                        .expect("visible owned window"),
                ));
                self.deadline = Instant::now() + Duration::from_millis(100);
            }
            fn new_events(&mut self, event_loop: &ActiveEventLoop, _: StartCause) {
                if self.app.window.is_none() || Instant::now() < self.deadline {
                    return;
                }
                if self.waiting {
                    self.finish_case(event_loop, false);
                    return;
                }
                if self.redraws.len() == 5 {
                    return;
                }
                self.app.tab_preview.clear();
                self.app.tab_preview.target = Some(self.target.clone());
                let case = self.redraws.len();
                let result = if case != 3 {
                    Ok(PreviewImage {
                        width: 2,
                        height: 1,
                        rgba: vec![255; 8].into(),
                    })
                } else {
                    Err("controlled preview failure".into())
                };
                self.waiting = true;
                self.deadline = Instant::now() + Duration::from_millis(250);
                match case {
                    0 => self.app.request_redraw(),
                    1 => {
                        // Historical completion handler: egui-only repaint.
                        assert!(self.app.tab_preview.finish(
                            self.app.ui_context.as_ref().expect("context"),
                            self.target.clone(),
                            self.app.tab_preview.generation,
                            result,
                        ));
                    }
                    _ => self.app.handle_app_event(AppEvent::TabPreview(
                        self.target.clone(),
                        self.app
                            .tab_preview
                            .generation
                            .wrapping_add(u64::from(case == 4)),
                        result,
                    )),
                }
                assert_eq!(
                    self.app.tab_preview.texture.is_some(),
                    (1..=3).contains(&case),
                    "only current completions are accepted"
                );
                assert_eq!(self.app.ui_repaint_at, None, "no previous repaint timer");
            }
            fn window_event(
                &mut self,
                event_loop: &ActiveEventLoop,
                _: WindowId,
                event: WindowEvent,
            ) {
                if self.waiting && matches!(event, WindowEvent::RedrawRequested) {
                    self.finish_case(event_loop, true);
                }
            }
            fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
                if self.redraws.len() < 5 {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(self.deadline));
                }
            }
        }
        let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
        app.ui_context = Some(crate::fonts::test_context());
        app.tabs
            .open_new(root.join("pending.png"), MediaKind::Image);
        let target = app.tab_preview.target(app.tabs.active().expect("tab"));
        let mut trial = Trial {
            app,
            target,
            deadline: Instant::now(),
            waiting: false,
            redraws: Vec::new(),
        };
        let mut builder = EventLoop::builder();
        builder.with_any_thread(true);
        builder
            .build()
            .expect("event loop")
            .run_app(&mut trial)
            .expect("native wakeup trial");
        assert_eq!(
            trial.redraws,
            [true, false, true, true, false],
            "native control/current completions wake; egui-only/stale remain idle"
        );
    }

    #[test]
    fn pending_tab_preview_is_quiet_and_retains_identity_time_and_failures() {
        for kind in [MediaKind::Image, MediaKind::Video, MediaKind::Audio] {
            let context = crate::fonts::test_context();
            context.global_style_mut(|style| {
                crate::chrome::style(style);
                style.interaction.tooltip_delay = 0.0;
                style.interaction.show_tooltips_only_when_still = false;
            });
            let mut tabs = TabSet::default();
            tabs.open_new("fixture-media".into(), kind);
            let mut preview = TabPreview::new().expect("worker");
            let target = preview.target(tabs.active().expect("tab"));
            preview.target = Some(target.clone());
            let texture = context.load_texture(
                "ready",
                egui::ColorImage::filled([2, 1], egui::Color32::RED),
                TextureOptions::LINEAR,
            );
            for state in 0..3 {
                preview.texture = match state {
                    1 => Some(Err("fixture failure".into())),
                    2 => Some(Ok(texture.clone())),
                    _ => None,
                };
                for pass in 0..4 {
                    let rect =
                        egui::Rect::from_min_size(egui::pos2(40.0, 20.0), egui::vec2(140.0, 24.0));
                    let output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(480.0, 300.0),
                            )),
                            events: vec![egui::Event::PointerMoved(rect.center())],
                            ..Default::default()
                        },
                        |ui| {
                            let response = ui.allocate_rect(rect, egui::Sense::hover());
                            preview.show(&response, &target, None);
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
                            .any(|text| matches!(*text, "…" | "Loading preview…"))
                    );
                    assert!(text.contains(&"fixture-media"));
                    assert_eq!(text.contains(&"No preview"), state == 1);
                    assert_eq!(
                        text.iter().any(|text| text.starts_with("Preview near ")),
                        kind == MediaKind::Video
                    );
                    assert_eq!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == texture.id())), state == 2);
                }
            }
        }
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
                rgba: vec![255; 4].into(),
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
                rgba: vec![255; 4].into(),
            }),
        );
        assert!(preview.target.is_none() && preview.texture.is_none());
        std::fs::remove_dir(root).expect("remove empty owned cache");
    }

    #[test]
    fn background_video_hover_uses_retained_clock_without_activating_the_tab() {
        let mut app = crate::Application::new(None, |_| {}).expect("headless app");
        let context = crate::fonts::test_context();
        context.global_style_mut(crate::chrome::style);
        app.ui_context = Some(context.clone());
        let path = PathBuf::from("background-video.mp4");
        let video = app.tabs.open_new(path.clone(), MediaKind::Video);
        app.tabs
            .close_gallery(app.tabs.gallery().expect("media-only fixture"));
        app.path = Some(path.clone());
        app.media_kind = Some(MediaKind::Video);
        app.media_duration = Some(Duration::from_secs(100));
        app.state = towavue_core::PlaybackState::Paused;
        app.clock = Some(crate::PlaybackClock::paused(
            crate::media_time(Duration::from_secs(37)),
            1.0,
        ));
        app.tab_preview.record(
            &app.tabs,
            Some(&path),
            Duration::from_secs(2),
            app.media_duration,
            None,
        );
        let saved = app.take_playback_tab_state();
        app.retained_playback.insert(video, saved);
        let image = app.tabs.open_new("other.png".into(), MediaKind::Image);
        app.path = Some("other.png".into());
        app.media_kind = Some(MediaKind::Image);
        let mut time = 0.0;
        let mut paint = |app: &mut crate::Application<_>, pointer: Option<egui::Pos2>| {
            time += 0.1;
            context.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events: pointer.into_iter().map(egui::Event::PointerMoved).collect(),
                    ..Default::default()
                },
                |ui| {
                    let mut actions = Vec::new();
                    app.draw_top_bar(ui, &mut actions);
                    assert!(actions.is_empty());
                },
            )
        };
        for _ in 0..4 {
            let _ = paint(&mut app, Some(egui::pos2(90.0, 16.0)));
        }
        assert_eq!(
            app.tab_preview
                .target
                .as_ref()
                .expect("hover target")
                .position,
            Duration::from_millis(37500)
        );
        assert_eq!(app.tabs.active().expect("active").id, image);
        assert_eq!(
            app.retained_playback[&video].position(),
            crate::media_time(Duration::from_secs(37))
        );
        let target = app.tab_preview.target.clone().expect("target");
        let layout = towavue_runtime_windows::VideoSheetLayout::for_position(
            Duration::from_secs(100),
            target.position,
        )
        .expect("sheet");
        app.tab_preview.finish_sheet(
            &context,
            target,
            app.tab_preview.generation,
            Ok(towavue_runtime_windows::VideoPreviewSheet {
                layout,
                image: PreviewImage {
                    width: 960,
                    height: 640,
                    rgba: vec![255; 960 * 640 * 4].into(),
                },
            }),
        );
        let texture = app
            .tab_preview
            .texture
            .as_ref()
            .expect("result")
            .as_ref()
            .expect("texture")
            .id();
        let generation = app.tab_preview.generation;
        for _ in 0..3 {
            let _ = paint(&mut app, None);
        }
        let saved = app.retained_playback.get_mut(&video).expect("background");
        saved.clock = Some(crate::PlaybackClock::paused(
            crate::media_time(Duration::from_secs(47)),
            1.0,
        ));
        saved.state = towavue_core::PlaybackState::Playing;
        let output = paint(&mut app, None);
        assert_eq!(
            app.tab_preview
                .target
                .as_ref()
                .expect("new position")
                .position,
            Duration::from_millis(47500)
        );
        assert_eq!(app.tab_preview.generation, generation, "no new sheet job");
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == texture)), "cached cell is painted in the position-change frame");
        assert!(
            output
                .textures_delta
                .set
                .iter()
                .all(|(id, _)| *id != texture),
            "no sheet reupload"
        );
        for _ in 0..3 {
            let _ = paint(&mut app, None);
        }
        assert!(
            paint(&mut app, None).viewport_output[&egui::ViewportId::ROOT].repaint_delay
                <= Duration::from_secs(1)
        );
        app.retained_playback
            .get_mut(&video)
            .expect("background")
            .state = towavue_core::PlaybackState::Paused;
        for _ in 0..3 {
            let _ = paint(&mut app, None);
        }
        assert!(
            paint(&mut app, None).viewport_output[&egui::ViewportId::ROOT].repaint_delay
                > Duration::from_secs(1),
            "paused hover settles"
        );
        let saved = app.retained_playback.get_mut(&video).expect("background");
        saved.path = "stale.mp4".into();
        assert_eq!(
            app.tab_preview
                .target_with_playback(&app.tabs.tabs()[0], Some(saved))
                .position,
            Duration::from_millis(2500),
            "stale retained path cannot supply a position"
        );
        for _ in 0..4 {
            let _ = paint(&mut app, Some(egui::pos2(500.0, 100.0)));
        }
        assert!(app.tab_preview.target.is_none());
        assert_eq!(app.tabs.active().expect("still active").id, image);
    }

    #[test]
    fn active_image_has_only_path_help_and_background_hover_reuses_retained_pixels() {
        let mut app = crate::Application::new(None, |_| {}).expect("headless app");
        let context = crate::fonts::test_context();
        context.global_style_mut(crate::chrome::style);
        app.ui_context = Some(context.clone());
        let path = PathBuf::from("current-animation.gif");
        let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
        app.tabs
            .close_gallery(app.tabs.gallery().expect("media-only fixture"));
        app.path = Some(path.clone());
        app.displayed_tab = Some(tab);
        app.media_kind = Some(MediaKind::Image);
        let decoded = Arc::new(towavue_runtime_windows::DecodedImage {
            animation_plays: 1,
            format: "test",
            frames: [10, 30]
                .into_iter()
                .map(|color| towavue_runtime_windows::DecodedImageFrame {
                    width: 2,
                    height: 1,
                    rgba: [color, 0, 0, 255].repeat(2),
                    delay: Duration::from_millis(10),
                })
                .collect(),
        });
        app.image =
            Some(crate::ImagePresentation::from_decoded(&context, &path, decoded).expect("image"));
        let texture = app.image.as_ref().expect("image").texture.id();
        let mut time = 0.0;
        let mut frame = |app: &mut crate::Application<_>, pointer| {
            time += 0.1;
            context.run_ui(
                egui::RawInput {
                    time: Some(time),
                    focused: false,
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
                    assert!(actions.is_empty());
                },
            )
        };
        let pointer = egui::pos2(90.0, 16.0);
        for _ in 0..3 {
            frame(&mut app, pointer);
        }
        let painted = |output: &egui::FullOutput, texture| {
            output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == texture))
        };
        let _ = context.tex_manager().write().take_delta();
        for _ in 0..12 {
            frame(&mut app, pointer);
        }
        assert!(frame(&mut app, pointer).shapes.iter().any(|shape|
            matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == path.display().to_string())),
            "active tab retains ordinary delayed path help");
        assert!(
            !painted(&frame(&mut app, pointer), texture),
            "active tabs have no preview card"
        );
        assert!(
            app.tab_preview.target.is_none(),
            "no background thumbnail job"
        );
        assert!(
            context.tex_manager().write().take_delta().set.is_empty(),
            "hover does not upload pixels"
        );
        let image = app.image.as_mut().expect("image");
        let end = image.next_frame_at.expect("animation") + Duration::from_secs(1);
        assert!(image.advance_animation(end));
        assert!(image.next_frame_at.is_none());
        let _ = context.tex_manager().write().take_delta();
        assert!(!painted(&frame(&mut app, pointer), texture));
        assert!(context.tex_manager().write().take_delta().set.is_empty());
        let original = app.image.as_ref().expect("image").decoded.clone();
        let edited = towavue_runtime_windows::render_image_edits(
            &original,
            &[towavue_core::EditOperation::RotateClockwise],
            &Default::default(),
        )
        .expect("edited pixels");
        app.install_edited_image(Arc::new(edited))
            .expect("edited presentation");
        let edited_texture = app.image.as_ref().expect("edited").texture.id();
        let _ = context.tex_manager().write().take_delta();
        assert!(!painted(&frame(&mut app, pointer), edited_texture));
        assert!(!painted(&frame(&mut app, pointer), texture));
        assert!(context.tex_manager().write().take_delta().set.is_empty());
        app.tabs.open_new("other.png".into(), MediaKind::Image);
        app.retain_image_tab();
        app.path = Some("other.png".into());
        app.displayed_tab = app.tabs.active().map(|tab| tab.id);
        assert!(
            painted(&frame(&mut app, pointer), edited_texture),
            "background tab keeps its retained frame"
        );
        assert!(app.tab_preview.target.is_none());
        assert!(
            app.retained_images[&tab]
                .image
                .as_ref()
                .expect("retained")
                .next_frame_at
                .is_none()
        );
        app.retained_images
            .get_mut(&tab)
            .expect("retained")
            .graphics_epoch = app.graphics_epoch.wrapping_add(1);
        assert!(
            !painted(&frame(&mut app, pointer), edited_texture),
            "stale graphics epoch must use the fallback"
        );
        assert!(app.tab_preview.target.is_some());
        let saved = app.retained_images.get_mut(&tab).expect("retained");
        saved.graphics_epoch = app.graphics_epoch;
        saved.path = "stale-path.gif".into();
        assert!(
            !painted(&frame(&mut app, pointer), edited_texture),
            "a retained image for another path is not a match"
        );
        app.retained_images.get_mut(&tab).expect("retained").path = path;
        assert!(painted(&frame(&mut app, pointer), edited_texture));
        assert!(
            app.tab_preview.target.is_none(),
            "loaded pixels cancel fallback work"
        );
        let generation = app.tab_preview.generation;
        assert!(painted(&frame(&mut app, pointer), edited_texture));
        assert_eq!(
            app.tab_preview.generation, generation,
            "steady hover does not churn jobs"
        );
        assert!(
            painted(&frame(&mut app, egui::pos2(900.0, 400.0)), edited_texture),
            "departure starts with the last borrowed image"
        );
        frame(&mut app, egui::pos2(900.0, 400.0));
        assert!(
            !painted(&frame(&mut app, egui::pos2(900.0, 400.0)), edited_texture),
            "departure releases its borrowed image after the fade"
        );
        // A retained tab can lose its presentation after a load/recovery failure.
        let held = app
            .retained_images
            .get_mut(&tab)
            .expect("retained")
            .image
            .take()
            .expect("image");
        assert!(!painted(&frame(&mut app, pointer), edited_texture));
        let fallback = app.tab_preview.target.clone().expect("fallback target");
        let generation = app.tab_preview.generation;
        app.tab_preview.finish(
            &context,
            fallback.clone(),
            generation,
            Ok(PreviewImage {
                width: 2,
                height: 1,
                rgba: [0, 0, 255, 255].repeat(2).into(),
            }),
        );
        let fallback_texture = app
            .tab_preview
            .texture
            .as_ref()
            .expect("fallback")
            .as_ref()
            .expect("pixels")
            .id();
        assert!(painted(&frame(&mut app, pointer), fallback_texture));
        app.retained_images.get_mut(&tab).expect("retained").image = Some(held);
        assert!(painted(&frame(&mut app, pointer), edited_texture));
        assert!(
            context
                .tex_manager()
                .read()
                .meta(fallback_texture)
                .is_none(),
            "loaded pixels release the replaced thumbnail"
        );
        app.tab_preview.finish(
            &context,
            fallback.clone(),
            generation,
            Ok(PreviewImage {
                width: 1,
                height: 1,
                rgba: vec![255; 4].into(),
            }),
        );
        assert!(
            app.tab_preview.texture.is_none(),
            "late fallback cannot replace the borrowed image"
        );
        let _ = context.tex_manager().write().take_delta();
        app.close_tab_unchecked(tab);
        assert!(!app.retained_images.contains_key(&tab));
        assert!(
            context.tex_manager().read().meta(edited_texture).is_some(),
            "the departing paint owns the closed tab's image briefly"
        );
        frame(&mut app, egui::pos2(900.0, 400.0));
        frame(&mut app, egui::pos2(900.0, 400.0));
        let closed = frame(&mut app, egui::pos2(900.0, 400.0));
        assert!(
            context.tex_manager().read().meta(edited_texture).is_none(),
            "the closed tab's image is released after the fade"
        );
        assert!(closed.textures_delta.free.contains(&edited_texture));
        assert!(!painted(&frame(&mut app, pointer), edited_texture));
        app.tab_preview.finish(
            &context,
            fallback,
            generation,
            Ok(PreviewImage {
                width: 1,
                height: 1,
                rgba: vec![255; 4].into(),
            }),
        );
        assert!(
            app.tab_preview.texture.is_none(),
            "closed-tab results remain stale even while another tab is hovered"
        );
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
        app.tabs
            .close_gallery(app.tabs.gallery().expect("media-only fixture"));
        app.path = Some(path);
        app.media_kind = Some(MediaKind::Image);
        app.push_edit(towavue_core::EditOperation::RotateClockwise);
        let dirty_tab = app.tabs.active_id().expect("dirty tab");
        app.tabs.open_new("foreground.png".into(), MediaKind::Image);
        let tabs = app.tabs.clone();
        let history = app.edits[&dirty_tab].operations().to_vec();
        let target = app.tab_preview.target(
            tabs.tabs()
                .iter()
                .find(|tab| tab.id == dirty_tab)
                .expect("dirty tab"),
        );
        let texture = context.load_texture(
            "fixture-tab-preview",
            egui::ColorImage::filled([64, 32], egui::Color32::RED),
            TextureOptions::LINEAR,
        );
        let texture_id = texture.id();
        let frame = |app: &mut crate::Application<_>, pointer, time, external_drag| {
            context.run_ui(
                egui::RawInput {
                    time: Some(time),
                    focused: false,
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events: vec![egui::Event::PointerMoved(pointer)],
                    hovered_files: if external_drag {
                        vec![egui::HoveredFile::default()]
                    } else {
                        Vec::new()
                    },
                    ..Default::default()
                },
                |ui| {
                    let mut actions = Vec::new();
                    if app.filmstrip_open {
                        app.draw_ui(ui, &mut actions);
                    } else {
                        app.draw_top_bar(ui, &mut actions);
                    }
                    assert!(actions.is_empty(), "hover must not dispatch actions");
                },
            )
        };
        for index in 0..3 {
            frame(&mut app, egui::pos2(90.0, 16.0), index as f64 * 0.1, false);
        }
        assert!(
            app.tab_preview.target.is_some(),
            "inactive hover requests the preview without activating the window"
        );
        app.tab_preview.target = Some(target);
        app.tab_preview.texture = Some(Ok(texture));
        let mut output = egui::FullOutput::default();
        for index in 3..8 {
            output = frame(&mut app, egui::pos2(90.0, 16.0), index as f64 * 0.1, false);
        }
        let rect = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id == texture_id => {
                    Some(mesh.calc_bounds())
                }
                _ => None,
            })
            .expect("tab thumbnail shown");
        assert!(rect.top() >= 32.0 && rect.right() <= 960.0);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().contains("preview-image.png"))));
        assert_eq!(app.tabs, tabs);
        assert_eq!(app.edits[&dirty_tab].operations(), history);
        assert!(app.pending_guard.is_none() && app.session.is_none());
        let target = app.tab_preview.target.clone().expect("hovered tab");
        let generation = app.tab_preview.generation;
        let output = frame(&mut app, egui::pos2(90.0, 16.0), 0.8, true);
        assert!(app.tab_preview.target.is_none() && app.tab_preview.texture.is_none());
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == texture_id)), "drop overlay must release the displayed preview");
        app.tab_preview
            .finish(&context, target, generation, Err("stale result".into()));
        assert!(
            app.tab_preview.texture.is_none(),
            "late results must not restore hidden previews"
        );
        let generation = app.tab_preview.generation;
        frame(&mut app, egui::pos2(90.0, 16.0), 0.9, true);
        assert_eq!(
            app.tab_preview.generation, generation,
            "no request/cancel churn while dragging"
        );
        frame(&mut app, egui::pos2(90.0, 16.0), 1.0, false);
        assert!(
            app.tab_preview.target.is_some(),
            "hover resumes after drag leaves"
        );
        assert_eq!(app.tabs, tabs);
        assert_eq!(app.edits[&dirty_tab].operations(), history);
        frame(&mut app, egui::pos2(400.0, 300.0), 1.1, false);
        assert!(app.tab_preview.target.is_none() && app.tab_preview.texture.is_none());
        for overlay in 0..3 {
            frame(
                &mut app,
                egui::pos2(90.0, 16.0),
                2.0 + overlay as f64,
                false,
            );
            assert!(app.tab_preview.target.is_some());
            app.palette_open = overlay == 0;
            app.grid_open = overlay == 1;
            app.filmstrip_open = overlay == 2;
            frame(
                &mut app,
                egui::pos2(90.0, 16.0),
                2.5 + overlay as f64,
                false,
            );
            if overlay == 2 {
                assert!(
                    app.tab_preview.target.is_some(),
                    "filmstrip permits hover cards"
                );
                let output = frame(&mut app, egui::pos2(90.0, 16.0), 4.7, false);
                assert!(
                    output.shapes.iter().any(|shape| matches!(
                        &shape.shape, egui::Shape::Text(text)
                        if text.pos.y >= 32.0 && text.galley.text().contains("preview-image.png")
                    )),
                    "hover caption remains rendered above the filmstrip"
                );
                assert!(app.filmstrip_open);
            } else {
                assert!(app.tab_preview.target.is_none() && app.tab_preview.texture.is_none());
            }
            app.palette_open = false;
            app.grid_open = false;
            app.filmstrip_open = false;
        }
    }
}
