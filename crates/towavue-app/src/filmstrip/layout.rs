use super::*;

pub(super) const DIAMETER: f32 = 120.0;
const GAP: f32 = 10.0;

/// Thumbnail sizes are independent of the fixed column geometry.
/// Loading or evicting a texture never changes scroll positions.
#[derive(Default)]
pub(super) struct Layout {
    key: Option<(PathBuf, u64, std::time::SystemTime, usize)>,
    indices: HashMap<PathBuf, usize>,
    sizes: Vec<Vec2>,
}

pub(super) fn size(source: Vec2) -> Vec2 {
    source * (DIAMETER / source.length())
}

impl Layout {
    pub fn prepare(
        &mut self,
        snapshot: &FolderSnapshot,
        previews: &HashMap<PathBuf, Preview>,
        uvs: &HashMap<PathBuf, Rect>,
    ) {
        let key = (
            snapshot.folder_path.clone(),
            snapshot.generation,
            snapshot.captured_at,
            snapshot.items.len(),
        );
        let reset = self.key.as_ref() != Some(&key);
        let same_folder = self.key.as_ref().is_some_and(|old| old.0 == key.0);
        if reset {
            self.key = Some(key);
            let old_indices = std::mem::take(&mut self.indices);
            let old_sizes = std::mem::take(&mut self.sizes);
            for (index, item) in snapshot.items.iter().enumerate() {
                self.indices.insert(item.path.clone(), index);
                let measured = same_folder
                    .then(|| {
                        old_indices
                            .get(&item.path)
                            .and_then(|index| old_sizes.get(*index))
                    })
                    .flatten()
                    .copied();
                self.sizes.push(measured.unwrap_or_else(|| {
                    size(if item.kind == MediaKind::Audio {
                        egui::vec2(2.0, 1.0)
                    } else {
                        egui::vec2(3.0, 2.0)
                    })
                }));
            }
        }
        // Only inspect the bounded ready texture set on an unchanged folder.
        for (path, preview) in previews {
            if let Some(&index) = self.indices.get(path)
                && let Ok((texture, _)) = preview
            {
                let source = if snapshot.items[index].kind == MediaKind::Audio {
                    egui::vec2(2.0, 1.0)
                } else {
                    texture.size_vec2() * uvs.get(path).map_or(Vec2::splat(1.0), Rect::size)
                };
                self.sizes[index] = size(source);
            }
        }
    }

    pub fn center_offset(&self, index: usize) -> f32 {
        index as f32 * (DIAMETER + GAP)
    }

    pub fn nearest(&self, offset: f32) -> usize {
        // Prefer the left column at an exact midpoint.
        ((offset / (DIAMETER + GAP) - 0.5).ceil().max(0.0) as usize)
            .min(self.sizes.len().saturating_sub(1))
    }

    pub fn width(&self, viewport: f32) -> f32 {
        self.sizes
            .len()
            .checked_sub(1)
            .map_or(viewport, |last| viewport + self.center_offset(last))
    }

    pub fn rect(&self, index: usize, origin: egui::Pos2, viewport: Vec2) -> Rect {
        Rect::from_center_size(
            origin
                + egui::vec2(
                    viewport.x * 0.5 + self.center_offset(index),
                    viewport.y * 0.5,
                ),
            self.sizes[index],
        )
    }

    pub fn visible(&self, viewport: Rect) -> Range<usize> {
        let half = viewport.width() * 0.5;
        // Include enlarged edge pixels while keeping the working set bounded.
        let radius = DIAMETER * 0.55 + 4.0;
        let start = ((viewport.left() - half - radius) / (DIAMETER + GAP))
            .ceil()
            .max(0.0) as usize;
        let end = (((viewport.right() - half + radius) / (DIAMETER + GAP))
            .floor()
            .max(0.0) as usize
            + 1)
        .min(self.sizes.len());
        start..end.min(start + VISIBLE_PREVIEW_LIMIT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_aspects_keep_fixed_columns_and_stable_scroll_on_load() {
        let context = crate::fonts::test_context();
        let snapshot = FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![]),
            folder_path: PathBuf::from("layout-fixture"),
            items: (0..50_000)
                .map(|index| towavue_core::FolderMediaItem {
                    identity: towavue_core::ShellIdentity::new(vec![]),
                    path: PathBuf::from(format!("layout-fixture/{index}.png")),
                    kind: MediaKind::Image,
                })
                .collect(),
            sort_columns: vec![],
            source: towavue_core::FolderSnapshotSource::NaturalNameFallback,
            generation: 1,
            captured_at: std::time::SystemTime::now(),
        };
        let mut layout = Layout::default();

        let mut previews = HashMap::new();
        let uvs = HashMap::new();
        layout.prepare(&snapshot, &previews, &uvs);
        let mut offset = layout.center_offset(25_000) + 10.0;
        let before = layout.center_offset(25_000) - offset;
        for (index, dimensions) in [[160, 80], [80, 160], [100, 100]].into_iter().enumerate() {
            let texture = context.load_texture(
                format!("layout-{index}"),
                egui::ColorImage::filled(dimensions, Color32::WHITE),
                egui::TextureOptions::LINEAR,
            );
            previews.insert(snapshot.items[index].path.clone(), Ok((texture, None)));
        }
        layout.prepare(&snapshot, &previews, &uvs);
        assert!((layout.center_offset(25_000) - offset - before).abs() < 0.5);
        // Loading the nearest card itself must preserve its center, even when
        // the viewport center is still to its left during ongoing scrolling.
        offset = layout.center_offset(4) - 35.0;
        assert_eq!(layout.nearest(offset), 4);
        let before = layout.center_offset(4) - offset;
        let texture = context.load_texture(
            "late-portrait",
            egui::ColorImage::filled([10, 200], Color32::WHITE),
            egui::TextureOptions::LINEAR,
        );
        previews.insert(snapshot.items[4].path.clone(), Ok((texture, None)));
        layout.prepare(&snapshot, &previews, &uvs);
        assert!((layout.center_offset(4) - offset - before).abs() < 0.001);
        for step in 0..400 {
            let offset = step as f32 * 3.0;
            let expected = (0..20)
                .min_by(|a, b| {
                    (layout.center_offset(*a) - offset)
                        .abs()
                        .total_cmp(&(layout.center_offset(*b) - offset).abs())
                })
                .expect("nonempty centers");
            assert_eq!(layout.nearest(offset), expected);
        }
        let rects: Vec<_> = (0..3)
            .map(|index| layout.rect(index, egui::Pos2::ZERO, egui::vec2(960.0, 500.0)))
            .collect();
        for rect in &rects {
            assert!((rect.size().length() - 120.0).abs() < 0.001);
            assert!((rect.center().y - 250.0).abs() < 0.001);
        }
        for pair in rects.windows(2) {
            assert!((pair[1].center().x - pair[0].center().x - 130.0).abs() < 0.001);
        }
        let measured = layout.sizes.clone();
        previews.clear();
        layout.prepare(&snapshot, &previews, &uvs);
        assert_eq!(
            layout.sizes, measured,
            "texture eviction preserves geometry"
        );
        let refreshed = FolderSnapshot {
            generation: snapshot.generation + 1,
            ..snapshot.clone()
        };
        let before = offset;
        layout.prepare(&refreshed, &previews, &uvs);
        assert_eq!(
            layout.sizes, measured,
            "a new listing must retain known widths by path"
        );
        assert_eq!(offset, before, "unchanged ordering must not move the view");
        for width in [320.0, 960.0, 100_000.0] {
            let viewport = Rect::from_min_size(egui::pos2(offset, 0.0), egui::vec2(width, 500.0));
            assert!(layout.visible(viewport).len() <= VISIBLE_PREVIEW_LIMIT);
        }
    }
}
