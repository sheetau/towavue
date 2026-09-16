use crate::{EditOperation, ImageRotation, VideoRotation};

fn angle(a: i16, b: i16) -> i16 {
    ((i32::from(a) + i32::from(b) + 1800).rem_euclid(3600) - 1800) as i16
}

/// Compose contiguous free rotations from the last non-rotation edit's canvas.
/// Every other operation is a barrier. Mismatched source geometry is retained
/// for the normal renderer/export validation rather than silently repaired.
pub fn compose_rotations(operations: &[EditOperation]) -> Vec<EditOperation> {
    let mut output = Vec::with_capacity(operations.len());
    let mut remaining = operations.iter().copied().peekable();
    while let Some(operation) = remaining.next() {
        match operation {
            EditOperation::RotateImage(mut rotation) => {
                if rotation.tenths() == 1800 {
                    rotation = ImageRotation::new(-1800, rotation.source_size())
                        .expect("half-turn preserves the validated canvas");
                }
                while let Some(EditOperation::RotateImage(next)) = remaining.peek().copied() {
                    if next.source_size() != rotation.size() {
                        break;
                    }
                    let Some(combined) = ImageRotation::new(
                        angle(rotation.tenths(), next.tenths()),
                        rotation.source_size(),
                    ) else {
                        break;
                    };
                    rotation = combined;
                    remaining.next();
                }
                if rotation.tenths() != 0 {
                    output.push(EditOperation::RotateImage(rotation));
                }
            }
            EditOperation::RotateVideo(mut rotation) => {
                if rotation.tenths() == 1800 {
                    rotation = VideoRotation::new(
                        -1800,
                        rotation.source_size(),
                        rotation.source_pixel_aspect(),
                    )
                    .expect("half-turn preserves the validated canvas");
                }
                while let Some(EditOperation::RotateVideo(next)) = remaining.peek().copied() {
                    // A complete turn restores the original SAR and odd dimensions;
                    // a nonzero rotation has already normalized/padded its output.
                    let (size, aspect) = if rotation.tenths() == 0 {
                        (rotation.source_size(), rotation.source_pixel_aspect())
                    } else {
                        (rotation.size(), 1.0)
                    };
                    if next.source_size() != size || next.source_pixel_aspect() != aspect {
                        break;
                    }
                    let Some(combined) = VideoRotation::new(
                        angle(rotation.tenths(), next.tenths()),
                        rotation.source_size(),
                        rotation.source_pixel_aspect(),
                    ) else {
                        break;
                    };
                    rotation = combined;
                    remaining.next();
                }
                if rotation.tenths() != 0 {
                    output.push(EditOperation::RotateVideo(rotation));
                }
            }
            _ => output.push(operation),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EditHistory, ImageResize, MediaKind, ResampleFilter};

    #[test]
    fn repeated_image_steps_keep_one_bounded_canvas_and_every_undo_checkpoint() {
        let source = (801, 603);
        let mut history = EditHistory::default();
        let mut checkpoints = vec![Vec::new()];
        let mut size = source;
        for step in 1..=144 {
            assert!(history.push(
                EditOperation::RotateImage(
                    ImageRotation::new(50, size).expect("rotation test fixture")
                ),
                MediaKind::Image,
            ));
            let total = angle(0, step * 50 % 3600);
            let expected = ImageRotation::new(total, source).expect("rotation test fixture");
            size = expected.size();
            let operations: Vec<_> = (total != 0)
                .then_some(EditOperation::RotateImage(expected))
                .into_iter()
                .collect();
            assert_eq!(history.operations(), operations);
            if step == 5 {
                history.mark_saved();
            }
            assert_eq!(history.is_dirty(), step % 72 != 5);
            checkpoints.push(operations);
        }
        for expected in checkpoints[..144].iter().rev() {
            assert!(history.undo());
            assert_eq!(history.operations(), expected);
        }
        assert!(!history.undo());
        for expected in checkpoints.iter().skip(1) {
            assert!(history.redo());
            assert_eq!(history.operations(), expected);
        }
        history.mark_exported(&checkpoints[5]);
        for _ in 5..144 {
            assert!(history.undo());
        }
        assert!(
            !history.is_dirty(),
            "exported plan length is not its undo cursor"
        );
        assert!(history.undo());
        let before = history.clone();
        assert!(!history.push(
            EditOperation::RotateImage(
                ImageRotation::new(0, source).expect("rotation test fixture")
            ),
            MediaKind::Image,
        ));
        assert_eq!(history, before, "zero input preserves redo");
        let current = ImageRotation::new(200, source).expect("rotation test fixture");
        assert!(history.push(
            EditOperation::RotateImage(
                ImageRotation::new(-200, current.size()).expect("rotation test fixture")
            ),
            MediaKind::Image,
        ));
        assert!(history.operations().is_empty());
        assert!(!history.redo(), "new branch discards only future steps");
    }

    #[test]
    fn repeated_video_steps_restore_original_aspect_and_odd_dimensions() {
        for aspect in [0.75, 1.0, 1.5] {
            let source = (65, 49);
            let mut history = EditHistory::default();
            let (mut size, mut sar) = (source, aspect);
            for step in 1..=145 {
                let delta = VideoRotation::new(-50, size, sar).expect("rotation test fixture");
                assert!(history.push(EditOperation::RotateVideo(delta), MediaKind::Video));
                let total = angle(0, -(step * 50 % 3600));
                if total == 0 {
                    assert!(history.operations().is_empty());
                    (size, sar) = (source, aspect);
                } else {
                    let expected =
                        VideoRotation::new(total, source, aspect).expect("rotation test fixture");
                    assert_eq!(history.operations(), [EditOperation::RotateVideo(expected)]);
                    (size, sar) = (expected.size(), 1.0);
                }
            }
            assert!(history.undo());
            assert!(history.operations().is_empty());
            assert!(history.redo());
            assert_eq!(history.operations().len(), 1);
        }
        let mut history = EditHistory::default();
        let size = (64, 48);
        history.push(
            EditOperation::RotateVideo(
                VideoRotation::new(1800, size, 1.0).expect("rotation test fixture"),
            ),
            MediaKind::Video,
        );
        history.mark_saved();
        for _ in 0..4 {
            history.push(
                EditOperation::RotateVideo(
                    VideoRotation::new(900, size, 1.0).expect("rotation test fixture"),
                ),
                MediaKind::Video,
            );
            // Use the current composed geometry for the next delta.
            if let Some(EditOperation::RotateVideo(rotation)) = history.operations().last() {
                let next =
                    VideoRotation::new(-900, rotation.size(), 1.0).expect("rotation test fixture");
                history.push(EditOperation::RotateVideo(next), MediaKind::Video);
            }
            assert!(
                !history.is_dirty(),
                "canonical half-turns compare by content"
            );
        }
    }

    #[test]
    fn other_edits_and_mismatched_sources_keep_their_order_and_separate_canvases() {
        let first = ImageRotation::new(50, (80, 60)).expect("rotation test fixture");
        for barrier in [
            EditOperation::Resize(
                ImageResize::new(40, 30, ResampleFilter::Nearest).expect("rotation test fixture"),
            ),
            EditOperation::FlipHorizontal,
            EditOperation::RotateClockwise,
            EditOperation::SetVolume(0.5),
        ] {
            let next_source = match barrier {
                EditOperation::Resize(value) => value.size(),
                EditOperation::RotateClockwise => (first.size().1, first.size().0),
                _ => first.size(),
            };
            let second = ImageRotation::new(50, next_source).expect("rotation test fixture");
            let third = ImageRotation::new(-100, second.size()).expect("rotation test fixture");
            let input = [
                EditOperation::RotateImage(first),
                barrier,
                EditOperation::RotateImage(second),
                EditOperation::RotateImage(third),
            ];
            assert_eq!(
                compose_rotations(&input),
                [
                    EditOperation::RotateImage(first),
                    barrier,
                    EditOperation::RotateImage(
                        ImageRotation::new(-50, next_source).expect("rotation test fixture")
                    ),
                ]
            );
        }
        let invalid = [
            EditOperation::RotateImage(first),
            EditOperation::RotateImage(
                ImageRotation::new(50, (7, 3)).expect("rotation test fixture"),
            ),
        ];
        assert_eq!(compose_rotations(&invalid), invalid);
    }
}
