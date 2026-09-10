use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use egui::{TextureHandle, TextureOptions};
use towavue_runtime_windows::{LatestTask, PreviewCache, VideoPreviewSheet, VideoSheetLayout};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request {
    pub path: PathBuf,
    pub layout: VideoSheetLayout,
}

pub struct VideoSheets {
    worker: LatestTask,
    generation: u64,
    pending: Option<Request>,
    textures: VecDeque<(Request, TextureHandle)>,
    failed: VecDeque<Request>,
}

impl VideoSheets {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            worker: LatestTask::new("towavue-video-sheets")?,
            generation: 0,
            pending: None,
            textures: VecDeque::new(),
            failed: VecDeque::new(),
        })
    }

    pub fn clear(&mut self) {
        self.worker.clear();
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
        self.textures.clear();
        self.failed.clear();
    }

    pub fn request<N>(
        &mut self,
        target: Request,
        priority: bool,
        cache: &PreviewCache,
        notify: Arc<N>,
    ) where
        N: Fn(crate::AppEvent) + Send + Sync + 'static,
    {
        if let Some(index) = self
            .textures
            .iter()
            .position(|(request, _)| request == &target)
        {
            let entry = self.textures.remove(index).expect("cached sheet");
            self.textures.push_back(entry);
            return;
        }
        if self.failed.contains(&target)
            || self.pending.as_ref() == Some(&target)
            || (!priority && self.pending.is_some())
        {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.pending = Some(target.clone());
        let cache = cache.clone();
        self.worker.submit(move |cancel| {
            let result = cache
                .cancellable(cancel.clone())
                .video_sheet(&target.path, target.layout)
                .map_err(|error| error.to_string());
            if !cancel.is_cancelled() {
                notify(crate::AppEvent::VideoSheet(target, generation, result));
            }
        });
    }

    pub fn finish(
        &mut self,
        context: &egui::Context,
        target: Request,
        generation: u64,
        result: Result<VideoPreviewSheet, String>,
    ) {
        if generation != self.generation || self.pending.as_ref() != Some(&target) {
            return;
        }
        self.pending = None;
        let result = result.and_then(|sheet| {
            let limit = context.input(|input| input.max_texture_side);
            if sheet.layout != target.layout
                || sheet.image.width as usize > limit
                || sheet.image.height as usize > limit
            {
                return Err("Video sheet exceeds texture limits or has a stale layout".into());
            }
            Ok(context.load_texture(
                format!(
                    "video-sheet:{}:{}",
                    target.path.display(),
                    target.layout.index()
                ),
                egui::ColorImage::from_rgba_unmultiplied(
                    [sheet.image.width as usize, sheet.image.height as usize],
                    &sheet.image.rgba,
                ),
                TextureOptions::LINEAR,
            ))
        });
        match result {
            Ok(texture) => {
                while self.textures.len() >= 2 {
                    self.textures.pop_front();
                }
                self.textures.push_back((target, texture));
            }
            Err(error) => {
                eprintln!("towavue: video sheet unavailable: {error}");
                while self.failed.len() >= 32 {
                    self.failed.pop_front();
                }
                self.failed.push_back(target);
            }
        }
        context.request_repaint();
    }

    pub fn image(&self, target: &Request, position: Duration) -> Option<egui::Image<'static>> {
        let (texture, uv) = self.sample(target, position)?;
        Some(
            // Mesh painting preserves cell UVs; rounded-rect antialiasing expands them.
            egui::Image::new((texture.id(), egui::vec2(240.0, 160.0)))
                .rotate(0.0, egui::vec2(0.5, 0.5))
                .uv(uv),
        )
    }

    pub fn sample(
        &self,
        target: &Request,
        position: Duration,
    ) -> Option<(TextureHandle, egui::Rect)> {
        let (_, texture) = self
            .textures
            .iter()
            .find(|(request, _)| request == target)?;
        let [left, top, right, bottom] = target.layout.uv(position)?;
        Some((
            texture.clone(),
            egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(index: u64) -> Request {
        Request {
            path: "owned-fixture.mp4".into(),
            layout: VideoSheetLayout::for_position(
                Duration::from_secs(3600),
                Duration::from_secs(index * 80),
            )
            .expect("layout"),
        }
    }

    fn pixels(target: &Request) -> VideoPreviewSheet {
        VideoPreviewSheet {
            layout: target.layout,
            image: towavue_runtime_windows::PreviewImage {
                width: 960,
                height: 640,
                rgba: vec![127; 960 * 640 * 4],
            },
        }
    }

    #[test]
    fn sheet_cells_share_one_texture_and_reject_stale_or_oversized_results() {
        let context = crate::fonts::test_context();
        let mut sheets = VideoSheets::new().expect("worker");
        let target = target(0);
        sheets.pending = Some(target.clone());
        sheets.finish(&context, target.clone(), 1, Ok(pixels(&target)));
        assert!(sheets.textures.is_empty());
        sheets.finish(&context, target.clone(), 0, Ok(pixels(&target)));
        assert_eq!(sheets.textures.len(), 1);
        let id = sheets.textures[0].1.id();
        let initial = context.run_ui(egui::RawInput::default(), |_| {});
        assert!(
            initial
                .textures_delta
                .set
                .iter()
                .any(|(texture, _)| *texture == id)
        );
        for slot in 0..16 {
            let position = target.layout.position(slot).expect("sample");
            let output = context.run_ui(egui::RawInput::default(), |ui| {
                ui.add(sheets.image(&target, position).expect("resident cell"));
            });
            assert!(
                output
                    .textures_delta
                    .set
                    .iter()
                    .all(|(texture, _)| *texture != id),
                "moving between cells must not upload a new texture"
            );
            let primitives = context.tessellate(output.shapes, output.pixels_per_point);
            let meshes: Vec<_> = primitives
                .iter()
                .filter_map(|primitive| match &primitive.primitive {
                    egui::epaint::Primitive::Mesh(mesh) if mesh.texture_id == id => Some(mesh),
                    _ => None,
                })
                .collect();
            assert!(!meshes.is_empty());
            let [left, top, right, bottom] = target.layout.uv(position).expect("cell UV");
            assert!(
                meshes
                    .iter()
                    .flat_map(|mesh| &mesh.vertices)
                    .all(|vertex| vertex.uv.x >= left
                        && vertex.uv.x <= right
                        && vertex.uv.y >= top
                        && vertex.uv.y <= bottom),
                "slot {slot}: expected {:?}, actual {:?}",
                [left, top, right, bottom],
                meshes
                    .iter()
                    .flat_map(|mesh| &mesh.vertices)
                    .map(|vertex| vertex.uv)
                    .collect::<Vec<_>>()
            );
        }
        sheets.clear();
        sheets.finish(&context, target.clone(), 0, Ok(pixels(&target)));
        assert!(sheets.textures.is_empty());
        sheets.pending = Some(target.clone());
        context.input_mut(|input| input.max_texture_side = 512);
        sheets.finish(
            &context,
            target.clone(),
            sheets.generation,
            Ok(pixels(&target)),
        );
        assert!(sheets.failed.contains(&target));
        assert!(sheets.textures.is_empty());
    }

    #[test]
    fn priority_preempts_prefetch_and_two_sheet_lru_keeps_the_visible_area() {
        let context = crate::fonts::test_context();
        let root =
            std::env::temp_dir().join(format!("towavue-sheet-controller-{}", std::process::id()));
        let cache = PreviewCache::new(root.clone()).expect("owned cache");
        let mut sheets = VideoSheets::new().expect("worker");
        let notify = Arc::new(|_| {});
        sheets.pending = Some(target(0));
        sheets.request(target(1), false, &cache, notify.clone());
        assert_eq!(sheets.pending, Some(target(0)));
        sheets.request(target(1), true, &cache, notify.clone());
        assert_eq!(sheets.pending, Some(target(1)));
        sheets.finish(&context, target(0), 0, Ok(pixels(&target(0))));
        assert!(sheets.textures.is_empty());
        sheets.worker.clear();
        for index in 0..2 {
            sheets.pending = Some(target(index));
            sheets.finish(
                &context,
                target(index),
                sheets.generation,
                Ok(pixels(&target(index))),
            );
        }
        sheets.request(target(0), false, &cache, notify);
        sheets.pending = Some(target(2));
        sheets.finish(
            &context,
            target(2),
            sheets.generation,
            Ok(pixels(&target(2))),
        );
        assert_eq!(sheets.textures.len(), 2);
        assert_eq!(sheets.textures[0].0, target(0));
        assert_eq!(sheets.textures[1].0, target(2));
        drop(sheets);
        std::fs::remove_dir_all(root).expect("remove owned cache");
    }
}
