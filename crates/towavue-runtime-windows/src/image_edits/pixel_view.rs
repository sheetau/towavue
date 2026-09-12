use super::{Cancellation, DecodedImageFrame, EditOperation};

struct View<'a> {
    frame: &'a DecodedImageFrame,
    width: u32,
    height: u32,
    origin: i64,
    x_step: i64,
    y_step: i64,
}

impl<'a> View<'a> {
    fn new(frame: &'a DecodedImageFrame, operations: &[EditOperation]) -> Option<Self> {
        let pixels = u64::from(frame.width) * u64::from(frame.height);
        if pixels == 0 || pixels > 128 * 1024 * 1024 || pixels * 4 != frame.rgba.len() as u64 {
            return None;
        }
        let mut view = Self {
            frame,
            width: frame.width,
            height: frame.height,
            origin: 0,
            x_step: 4,
            y_step: i64::from(frame.width) * 4,
        };
        // Each operation maps output pixel centers back into the same validated RGBA buffer.
        // Crops stay inside the preceding view; signed strides only reorder existing pixels.
        for operation in operations {
            match operation {
                EditOperation::FlipHorizontal => {
                    view.origin += i64::from(view.width - 1) * view.x_step;
                    view.x_step = -view.x_step;
                }
                EditOperation::FlipVertical => {
                    view.origin += i64::from(view.height - 1) * view.y_step;
                    view.y_step = -view.y_step;
                }
                EditOperation::RotateClockwise => {
                    view.origin += i64::from(view.height - 1) * view.y_step;
                    (view.x_step, view.y_step) = (-view.y_step, view.x_step);
                    (view.width, view.height) = (view.height, view.width);
                }
                EditOperation::RotateCounterclockwise => {
                    view.origin += i64::from(view.width - 1) * view.x_step;
                    (view.x_step, view.y_step) = (view.y_step, -view.x_step);
                    (view.width, view.height) = (view.height, view.width);
                }
                EditOperation::Crop(crop) => {
                    if crop.width == 0
                        || crop.height == 0
                        || crop.x.checked_add(crop.width)? > view.width
                        || crop.y.checked_add(crop.height)? > view.height
                    {
                        return None;
                    }
                    view.origin +=
                        i64::from(crop.x) * view.x_step + i64::from(crop.y) * view.y_step;
                    view.width = crop.width;
                    view.height = crop.height;
                }
                _ => return None,
            }
        }
        Some(view)
    }

    fn offset(&self, x: u32, y: u32) -> usize {
        (self.origin + i64::from(x) * self.x_step + i64::from(y) * self.y_step) as usize
    }
}

pub(super) fn compare(
    current: &DecodedImageFrame,
    current_operations: &[EditOperation],
    saved: &DecodedImageFrame,
    saved_operations: &[EditOperation],
    cancel: &Cancellation,
) -> Option<Result<bool, String>> {
    let mut current = View::new(current, current_operations)?;
    let mut saved = View::new(saved, saved_operations)?;
    // Equality does not depend on traversal order. Scan transposed views along source rows.
    if current.y_step.abs() == 4 && saved.y_step.abs() == 4 {
        for view in [&mut current, &mut saved] {
            (view.width, view.height) = (view.height, view.width);
            (view.x_step, view.y_step) = (view.y_step, view.x_step);
        }
    }
    Some((|| {
        if (current.width, current.height, current.frame.delay)
            != (saved.width, saved.height, saved.frame.delay)
        {
            return Ok(false);
        }
        // Mixed axes cannot both be scanned linearly; small tiles keep both sources cache-local.
        let mixed_axes = (current.x_step.abs() == 4) != (saved.x_step.abs() == 4);
        let (columns, rows) = if mixed_axes { (32, 32) } else { (16384, 1) };
        for top in (0..current.height).step_by(rows) {
            for start in (0..current.width).step_by(columns) {
                if cancel.is_cancelled() {
                    return Err("Image comparison cancelled".into());
                }
                let end = (start + columns as u32).min(current.width);
                for y in top..(top + rows as u32).min(current.height) {
                    if current.x_step == saved.x_step && current.x_step.abs() == 4 {
                        let x = if current.x_step > 0 { start } else { end - 1 };
                        let a = current.offset(x, y);
                        let b = saved.offset(x, y);
                        let bytes = (end - start) as usize * 4;
                        if current.frame.rgba[a..a + bytes] != saved.frame.rgba[b..b + bytes] {
                            return Ok(false);
                        }
                    } else {
                        for x in start..end {
                            let a = current.offset(x, y);
                            let b = saved.offset(x, y);
                            if current.frame.rgba[a..a + 4] != saved.frame.rgba[b..b + 4] {
                                return Ok(false);
                            }
                        }
                    }
                }
            }
        }
        Ok(true)
    })())
}
