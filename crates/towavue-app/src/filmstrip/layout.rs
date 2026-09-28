use super::*;

pub(super) const DIAMETER: f32 = 120.0;
const GAP: f32 = 20.0;

/// Folder-sized geometry, independent of the bounded texture working set.
/// Evicting a texture must not change the widths of previously measured cards.
#[derive(Default)]
pub(super) struct Layout {
    key: Option<(PathBuf, u64, std::time::SystemTime, usize)>,
    indices: HashMap<PathBuf, usize>,
    sizes: Vec<Vec2>,
    starts: Vec<f32>,
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
        offset: &mut f32,
    ) {
        let key = (
            snapshot.folder_path.clone(),
            snapshot.generation,
            snapshot.captured_at,
            snapshot.items.len(),
        );
        let reset = self.key.as_ref() != Some(&key);
        let anchor = (!reset && !self.sizes.is_empty()).then(|| {
            let index = self.nearest(*offset);
            (index, *offset - self.center_offset(index))
        });
        if reset {
            self.key = Some(key);
            self.indices.clear();
            self.sizes.clear();
            for (index, item) in snapshot.items.iter().enumerate() {
                self.indices.insert(item.path.clone(), index);
                self.sizes.push(size(if item.kind == MediaKind::Audio {
                    egui::vec2(2.0, 1.0)
                } else {
                    egui::vec2(3.0, 2.0)
                }));
            }
        }
        let mut changed = reset;
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
                let measured = size(source);
                changed |= self.sizes[index] != measured;
                self.sizes[index] = measured;
            }
        }
        if changed {
            self.starts.clear();
            let mut start = 0.0;
            for size in &self.sizes {
                self.starts.push(start);
                start += size.x + GAP;
            }
            if let Some((index, within)) = anchor {
                *offset = (self.center_offset(index) + within).max(0.0);
            }
        }
    }

    pub fn center_offset(&self, index: usize) -> f32 {
        self.starts.get(index).copied().unwrap_or(0.0)
            + (self.sizes.get(index).map_or(0.0, |size| size.x)
                - self.sizes.first().map_or(0.0, |size| size.x))
                * 0.5
    }

    pub fn nearest(&self, offset: f32) -> usize {
        let (mut left, mut right) = (0, self.sizes.len());
        while left < right {
            let middle = left + (right - left) / 2;
            if self.center_offset(middle) < offset {
                left = middle + 1;
            } else {
                right = middle;
            }
        }
        (left.saturating_sub(1)..=left.min(self.sizes.len().saturating_sub(1)))
            .min_by(|a, b| {
                (self.center_offset(*a) - offset)
                    .abs()
                    .total_cmp(&(self.center_offset(*b) - offset).abs())
            })
            .unwrap_or(0)
    }

    pub fn padding(&self, width: f32) -> f32 {
        ((width - self.sizes.first().map_or(0.0, |size| size.x)) * 0.5).max(0.0)
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
                    self.padding(viewport.x) + self.starts[index] + self.sizes[index].x * 0.5,
                    viewport.y * 0.5,
                ),
            self.sizes[index],
        )
    }

    pub fn visible(&self, viewport: Rect) -> Range<usize> {
        let padding = self.padding(viewport.width());
        let start = self
            .starts
            .partition_point(|left| *left + DIAMETER < viewport.left() - padding - 4.0);
        let end = self
            .starts
            .partition_point(|left| *left < viewport.right() - padding + 4.0);
        start..end.min(start + VISIBLE_PREVIEW_LIMIT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_aspects_keep_equal_diagonals_edge_gaps_and_stable_scroll_on_load() {
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
        let mut offset = 0.0;
        let mut previews = HashMap::new();
        let uvs = HashMap::new();
        layout.prepare(&snapshot, &previews, &uvs, &mut offset);
        offset = layout.center_offset(25_000) + 10.0;
        let before = layout.center_offset(25_000) - offset;
        for (index, dimensions) in [[160, 80], [80, 160], [100, 100]].into_iter().enumerate() {
            let texture = context.load_texture(
                format!("layout-{index}"),
                egui::ColorImage::filled(dimensions, Color32::WHITE),
                egui::TextureOptions::LINEAR,
            );
            previews.insert(snapshot.items[index].path.clone(), Ok((texture, None)));
        }
        layout.prepare(&snapshot, &previews, &uvs, &mut offset);
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
        layout.prepare(&snapshot, &previews, &uvs, &mut offset);
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
            assert!((pair[1].left() - pair[0].right() - 20.0).abs() < 0.001);
        }
        let measured = layout.sizes.clone();
        previews.clear();
        layout.prepare(&snapshot, &previews, &uvs, &mut offset);
        assert_eq!(
            layout.sizes, measured,
            "texture eviction preserves geometry"
        );
        for width in [320.0, 960.0, 100_000.0] {
            let viewport = Rect::from_min_size(egui::pos2(offset, 0.0), egui::vec2(width, 500.0));
            assert!(layout.visible(viewport).len() <= VISIBLE_PREVIEW_LIMIT);
        }
    }
}
