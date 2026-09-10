use towavue_core::{EditOperation, MediaKind, UnitPoint};

use super::RenderError;
use super::resample::FilterSpec;
use crate::VideoOrientation;

const MAX_BYTES: u64 = 512 * 1024 * 1024;
type Size = (u32, u32);

#[derive(Clone, Debug)]
pub(super) struct Stage {
    pub size: Size,
    pub slot: usize,
    // Pixel-center mapping, followed by the active canvas and rotation boundary mode.
    pub constants: [[f32; 4]; 4],
    pub filter: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Slot {
    pub size: Size,
    pub floating_point: bool,
}

impl Slot {
    fn bytes(self) -> u64 {
        u64::from(self.size.0) * u64::from(self.size.1) * if self.floating_point { 8 } else { 4 }
    }
}

pub(crate) struct Plan {
    pub(super) stages: Vec<Stage>,
    pub(super) slots: Vec<Slot>,
    pub(super) filters: Vec<FilterSpec>,
    pub(crate) size: Size,
    pub(crate) pixel_aspect: f32,
}

impl Plan {
    pub(crate) fn new(
        source: Size,
        pixel_aspect: f32,
        orientation: VideoOrientation,
        operations: &[EditOperation],
        max_side: usize,
    ) -> Result<Self, RenderError> {
        let mut plan = Self {
            stages: Vec::new(),
            slots: Vec::new(),
            filters: Vec::new(),
            size: source,
            pixel_aspect,
        };
        if !pixel_aspect.is_finite() || pixel_aspect <= 0.0 {
            return Err(RenderError::InvalidVideoEdit);
        }
        plan.validate_size(source, max_side)?;
        if orientation != VideoOrientation::default() {
            let output = if orientation.swaps_axes() {
                plan.pixel_aspect = 1.0 / pixel_aspect;
                (source.1, source.0)
            } else {
                source
            };
            plan.affine(output, orientation.source_uv(), max_side)?;
        }
        for operation in operations {
            let size = plan.size;
            let identity = VideoOrientation::default().source_uv();
            match *operation {
                EditOperation::ResizeVideo(resize) if !resize.is_identity() => {
                    if size != resize.source_size()
                        || (plan.pixel_aspect - resize.source_pixel_aspect()).abs()
                            > plan.pixel_aspect.abs() * 0.00001
                    {
                        return Err(RenderError::InvalidVideoEdit);
                    }
                    let output = resize.size();
                    plan.resample((output.0, size.1), resize.filter(), true, max_side)?;
                    plan.resample(output, resize.filter(), false, max_side)?;
                    plan.pixel_aspect = 1.0;
                }
                EditOperation::RotateVideo(rotation) if rotation.tenths() != 0 => {
                    if size != rotation.source_size()
                        || (plan.pixel_aspect - rotation.source_pixel_aspect()).abs()
                            > plan.pixel_aspect.abs() * 0.00001
                    {
                        return Err(RenderError::InvalidVideoEdit);
                    }
                    if size != rotation.square_size() {
                        plan.affine(rotation.square_size(), identity, max_side)?;
                    }
                    let size = plan.size;
                    let raster = rotation.raster_size();
                    let corners = match rotation.tenths() {
                        900 => Some([3, 0, 1, 2]),
                        -900 => Some([1, 2, 3, 0]),
                        -1800 | 1800 => Some([2, 3, 0, 1]),
                        _ => None,
                    };
                    if let Some(corners) = corners {
                        plan.affine(raster, corners.map(|index| identity[index]), max_side)?;
                    } else {
                        let (sin, cos) = (f64::from(rotation.tenths()) * std::f64::consts::PI
                            / 1800.0)
                            .sin_cos();
                        let (sin, cos) = (sin as f32, cos as f32);
                        let center_x = (raster.0 - 1) as f32 * 0.5;
                        let center_y = (raster.1 - 1) as f32 * 0.5;
                        plan.push(
                            raster,
                            [
                                [
                                    (size.0 - 1) as f32 * 0.5 - center_x * cos - center_y * sin,
                                    (size.1 - 1) as f32 * 0.5 + center_x * sin - center_y * cos,
                                    0.0,
                                    0.0,
                                ],
                                [cos, -sin, 0.0, 0.0],
                                [sin, cos, 0.0, 0.0],
                                [raster.0 as f32, raster.1 as f32, 1.0, 0.0],
                            ],
                            max_side,
                        )?;
                    }
                    if raster != rotation.size() {
                        // Padding is a separate raster: later edits may crop or rotate it.
                        plan.push(
                            rotation.size(),
                            [
                                [0.0; 4],
                                [1.0, 0.0, 0.0, 0.0],
                                [0.0, 1.0, 0.0, 0.0],
                                [raster.0 as f32, raster.1 as f32, 0.0, 0.0],
                            ],
                            max_side,
                        )?;
                    }
                    plan.pixel_aspect = 1.0;
                }
                EditOperation::Crop(crop) => {
                    if crop.width == 0
                        || crop.height == 0
                        || crop.x.checked_add(crop.width).is_none_or(|x| x > size.0)
                        || crop.y.checked_add(crop.height).is_none_or(|y| y > size.1)
                    {
                        return Err(RenderError::InvalidVideoEdit);
                    }
                    plan.push(
                        (crop.width, crop.height),
                        [
                            [crop.x as f32, crop.y as f32, 0.0, 0.0],
                            [1.0, 0.0, 0.0, 0.0],
                            [0.0, 1.0, 0.0, 0.0],
                            [crop.width as f32, crop.height as f32, 0.0, 0.0],
                        ],
                        max_side,
                    )?;
                }
                EditOperation::RotateClockwise | EditOperation::RotateCounterclockwise => {
                    let corners = if *operation == EditOperation::RotateClockwise {
                        [3, 0, 1, 2]
                    } else {
                        [1, 2, 3, 0]
                    };
                    plan.affine((size.1, size.0), corners.map(|i| identity[i]), max_side)?;
                    plan.pixel_aspect = 1.0 / plan.pixel_aspect;
                }
                EditOperation::FlipHorizontal | EditOperation::FlipVertical => {
                    let corners = if *operation == EditOperation::FlipHorizontal {
                        [1, 0, 3, 2]
                    } else {
                        [3, 2, 1, 0]
                    };
                    plan.affine(size, corners.map(|i| identity[i]), max_side)?;
                }
                operation if !operation.applies_to(MediaKind::Video) => {
                    return Err(RenderError::InvalidVideoEdit);
                }
                _ => {}
            }
        }
        if !plan.pixel_aspect.is_finite() || plan.pixel_aspect <= 0.0 {
            return Err(RenderError::InvalidVideoEdit);
        }
        Ok(plan)
    }

    fn validate_size(&self, size: Size, max_side: usize) -> Result<(), RenderError> {
        if size.0 == 0 || size.1 == 0 {
            return Err(RenderError::InvalidVideoEdit);
        }
        if size.0 as usize > max_side
            || size.1 as usize > max_side
            || u64::from(size.0) * u64::from(size.1) * 4 > MAX_BYTES
        {
            return Err(RenderError::VideoEditBudget);
        }
        Ok(())
    }

    fn affine(
        &mut self,
        output: Size,
        uv: [UnitPoint; 4],
        max_side: usize,
    ) -> Result<(), RenderError> {
        let input = [self.size.0 as f32, self.size.1 as f32];
        let x = [
            (uv[1].x - uv[0].x) * input[0] / output.0 as f32,
            (uv[1].y - uv[0].y) * input[1] / output.0 as f32,
        ];
        let y = [
            (uv[3].x - uv[0].x) * input[0] / output.1 as f32,
            (uv[3].y - uv[0].y) * input[1] / output.1 as f32,
        ];
        self.push(
            output,
            [
                [
                    uv[0].x * input[0] + (x[0] + y[0]) * 0.5 - 0.5,
                    uv[0].y * input[1] + (x[1] + y[1]) * 0.5 - 0.5,
                    0.0,
                    0.0,
                ],
                [x[0], x[1], 0.0, 0.0],
                [y[0], y[1], 0.0, 0.0],
                [output.0 as f32, output.1 as f32, 0.0, 0.0],
            ],
            max_side,
        )
    }

    fn push(
        &mut self,
        size: Size,
        constants: [[f32; 4]; 4],
        max_side: usize,
    ) -> Result<(), RenderError> {
        self.push_stage(size, constants, None, false, max_side)
    }

    fn resample(
        &mut self,
        size: Size,
        filter: towavue_core::ResampleFilter,
        horizontal: bool,
        max_side: usize,
    ) -> Result<(), RenderError> {
        let spec = FilterSpec {
            source: if horizontal { self.size.0 } else { self.size.1 },
            target: if horizontal { size.0 } else { size.1 },
            filter,
        };
        let index = if let Some(index) = self.filters.iter().position(|value| *value == spec) {
            index
        } else {
            if self.bytes() + spec.bytes() > MAX_BYTES {
                return Err(RenderError::VideoEditBudget);
            }
            self.filters.push(spec);
            self.filters.len() - 1
        };
        self.push_stage(
            size,
            [
                [0.0; 4],
                [0.0; 4],
                [0.0; 4],
                [
                    size.0 as f32,
                    size.1 as f32,
                    if horizontal { 2.0 } else { 3.0 },
                    spec.taps() as f32,
                ],
            ],
            Some(index),
            horizontal,
            max_side,
        )
    }

    fn bytes(&self) -> u64 {
        self.slots.iter().map(|slot| slot.bytes()).sum::<u64>()
            + self
                .filters
                .iter()
                .map(|filter| filter.bytes())
                .sum::<u64>()
    }

    fn push_stage(
        &mut self,
        size: Size,
        constants: [[f32; 4]; 4],
        filter: Option<usize>,
        floating_point: bool,
        max_side: usize,
    ) -> Result<(), RenderError> {
        self.validate_size(size, max_side)?;
        let specification = Slot {
            size,
            floating_point,
        };
        let previous = self.stages.last().map(|stage| stage.slot);
        let slot = if let Some(slot) = self
            .slots
            .iter()
            .enumerate()
            .position(|(index, slot)| *slot == specification && Some(index) != previous)
        {
            slot
        } else {
            let bytes = self.bytes() + specification.bytes();
            if bytes > MAX_BYTES {
                return Err(RenderError::VideoEditBudget);
            }
            self.slots.push(specification);
            self.slots.len() - 1
        };
        self.stages.push(Stage {
            size,
            slot,
            constants,
            filter,
        });
        self.size = size;
        Ok(())
    }
}
