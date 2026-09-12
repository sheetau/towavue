use super::*;

#[derive(Clone)]
struct Tile {
    id: u32,
    position: (u32, u32),
    size: (u32, u32),
}

pub(super) struct Grid {
    pub(super) size: (u32, u32),
    coded: (u32, u32),
    origin: (u32, u32),
    tiles: Vec<Tile>,
    orientation: Option<crate::VideoOrientation>,
    pub(super) aperture: Option<container::CleanAperture>,
}

fn unsigned(value: i32) -> Result<u32, ImageDecodeError> {
    u32::try_from(value).map_err(|_| invalid("negative grid geometry"))
}

impl Grid {
    pub(super) fn read(
        input: &ffmpeg::format::context::Input,
        id: u32,
        limit: usize,
    ) -> Result<Option<Self>, ImageDecodeError> {
        let mut result = None;
        // SAFETY: input owns the demuxer's groups, streams and side-data arrays.
        // It remains borrowed and is neither mutated nor shared across threads
        // here. FFmpeg supplies allocated lengths; check union tags, nullable
        // pointers and indices before access. Only owned values escape.
        unsafe {
            let context = &*input.as_ptr();
            if context.nb_stream_groups == 0 {
                return Ok(None);
            }
            if context.nb_stream_groups > 65536 || context.stream_groups.is_null() {
                return Err(invalid("invalid stream group list"));
            }
            for &group in
                std::slice::from_raw_parts(context.stream_groups, context.nb_stream_groups as usize)
            {
                let group = group
                    .as_ref()
                    .ok_or_else(|| invalid("missing stream group"))?;
                if group.id != i64::from(id) {
                    continue;
                }
                if result.is_some()
                    || group.type_
                        != ffmpeg::ffi::AVStreamGroupParamsType::AV_STREAM_GROUP_PARAMS_TILE_GRID
                {
                    return Err(invalid("ambiguous or unsupported image group"));
                }
                let grid = group
                    .params
                    .tile_grid
                    .as_ref()
                    .ok_or_else(|| invalid("missing tile grid"))?;
                if grid.nb_tiles == 0
                    || grid.nb_tiles > 65536
                    || grid.offsets.is_null()
                    || group.nb_streams == 0
                    || group.nb_streams > 65536
                    || group.streams.is_null()
                {
                    return Err(invalid("invalid tile grid arrays"));
                }
                let streams = std::slice::from_raw_parts(group.streams, group.nb_streams as usize);
                let mut tiles = Vec::with_capacity(grid.nb_tiles as usize);
                for offset in std::slice::from_raw_parts(grid.offsets, grid.nb_tiles as usize) {
                    let stream = streams
                        .get(offset.idx as usize)
                        .and_then(|stream| stream.as_ref())
                        .ok_or_else(|| invalid("invalid group-relative tile index"))?;
                    let parameters = stream
                        .codecpar
                        .as_ref()
                        .ok_or_else(|| invalid("missing tile codec"))?;
                    if stream.id <= 0
                        || parameters.codec_id != ffmpeg::ffi::AVCodecID::AV_CODEC_ID_AV1
                    {
                        return Err(invalid("invalid AV1 tile"));
                    }
                    tiles.push(Tile {
                        id: stream.id as u32,
                        position: (unsigned(offset.horizontal)?, unsigned(offset.vertical)?),
                        size: (unsigned(parameters.width)?, unsigned(parameters.height)?),
                    });
                }
                let mut orientation = None;
                if grid.nb_coded_side_data < 0 || grid.nb_coded_side_data > 65536 {
                    return Err(invalid("invalid grid side data count"));
                }
                if grid.nb_coded_side_data > 0 {
                    if grid.coded_side_data.is_null() {
                        return Err(invalid("missing grid side data"));
                    }
                    for data in std::slice::from_raw_parts(
                        grid.coded_side_data,
                        grid.nb_coded_side_data as usize,
                    ) {
                        if data.type_
                            == ffmpeg::ffi::AVPacketSideDataType::AV_PKT_DATA_DISPLAYMATRIX
                        {
                            if data.data.is_null() || data.size != 36 || orientation.is_some() {
                                return Err(invalid("invalid grid display matrix"));
                            }
                            orientation = Some(
                                crate::VideoOrientation::from_bytes(Some(
                                    std::slice::from_raw_parts(data.data, data.size),
                                ))
                                .map_err(ImageDecodeError::Ffmpeg)?,
                            );
                        }
                    }
                }
                let grid = Self {
                    size: (unsigned(grid.width)?, unsigned(grid.height)?),
                    coded: (unsigned(grid.coded_width)?, unsigned(grid.coded_height)?),
                    origin: (
                        unsigned(grid.horizontal_offset)?,
                        unsigned(grid.vertical_offset)?,
                    ),
                    tiles,
                    orientation,
                    aperture: None,
                };
                grid.validate(limit)?;
                result = Some(grid);
            }
        }
        Ok(result)
    }

    fn validate(&self, limit: usize) -> Result<(), ImageDecodeError> {
        check_size(self.size, limit)?;
        check_size(self.coded, limit)?;
        let Some(first) = self.tiles.first() else {
            return Err(invalid("empty grid"));
        };
        let (w, h) = first.size;
        if w == 0
            || h == 0
            || !self.coded.0.is_multiple_of(w)
            || !self.coded.1.is_multiple_of(h)
            || u64::from(self.origin.0) + u64::from(self.size.0) > u64::from(self.coded.0)
            || u64::from(self.origin.1) + u64::from(self.size.1) > u64::from(self.coded.1)
        {
            return Err(invalid("invalid grid canvas or tile size"));
        }
        let columns = self.coded.0 / w;
        let rows = self.coded.1 / h;
        if columns == 0
            || rows == 0
            || u64::from(columns) * u64::from(rows) != self.tiles.len() as u64
        {
            return Err(invalid("incomplete grid"));
        }
        for (index, tile) in self.tiles.iter().enumerate() {
            let index = index as u32;
            if tile.size != first.size
                || tile.position != (index % columns * w, index / columns * h)
            {
                return Err(invalid("nonuniform or overlapping grid tiles"));
            }
        }
        Ok(())
    }

    pub(super) fn decode(
        &self,
        path: &Path,
        pixel: Pixel,
        limit: usize,
        current: &dyn Fn() -> bool,
    ) -> Result<PlaneFrame, ImageDecodeError> {
        check_current(current)?;
        let channels = if pixel == Pixel::RGBA { 4 } else { 1 };
        let mut pixels = vec![0; self.size.0 as usize * self.size.1 as usize * channels];
        for tile in &self.tiles {
            check_current(current)?;
            let mut decoder = PlaneDecoder::new(
                open_input(path)?,
                &Selection {
                    id: tile.id,
                    timing: None,
                },
                pixel,
                limit,
            )?;
            let frame = decoder
                .next(current)?
                .ok_or_else(|| invalid("empty grid tile"))?;
            if frame.size != tile.size
                || frame.aperture.is_some()
                || frame
                    .orientation
                    .is_some_and(|value| value != crate::VideoOrientation::default())
                || decoder.next(current)?.is_some()
            {
                return Err(invalid("invalid grid tile frame or transform"));
            }
            self.copy_tile(tile, &frame.pixels, channels, &mut pixels, current)?;
        }
        Ok(PlaneFrame {
            size: self.size,
            pixels,
            time: 0,
            duration: None,
            orientation: self.orientation,
            aperture: self.aperture,
        })
    }

    fn copy_tile(
        &self,
        tile: &Tile,
        source: &[u8],
        channels: usize,
        output: &mut [u8],
        current: &dyn Fn() -> bool,
    ) -> Result<(), ImageDecodeError> {
        let left = tile.position.0.max(self.origin.0);
        let top = tile.position.1.max(self.origin.1);
        let right = (tile.position.0 + tile.size.0).min(self.origin.0 + self.size.0);
        let bottom = (tile.position.1 + tile.size.1).min(self.origin.1 + self.size.1);
        if left >= right || top >= bottom {
            return Ok(());
        }
        for y in top..bottom {
            check_current(current)?;
            let src = ((y - tile.position.1) as usize * tile.size.0 as usize
                + (left - tile.position.0) as usize)
                * channels;
            let dst = ((y - self.origin.1) as usize * self.size.0 as usize
                + (left - self.origin.0) as usize)
                * channels;
            let count = (right - left) as usize * channels;
            output[dst..dst + count].copy_from_slice(&source[src..src + count]);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Grid {
        Grid {
            size: (5, 3),
            coded: (6, 4),
            origin: (1, 1),
            tiles: (0..4)
                .map(|index| Tile {
                    id: index + 1,
                    position: (index % 2 * 3, index / 2 * 2),
                    size: (3, 2),
                })
                .collect(),
            orientation: None,
            aperture: None,
        }
    }

    #[test]
    fn tile_rows_clip_to_the_canvas_for_color_and_alpha_and_cancel() {
        let grid = grid();
        grid.validate(6 * 4 * 4).expect("bounded grid");
        for channels in [1, 4] {
            let mut output = vec![0; 5 * 3 * channels];
            for tile in &grid.tiles {
                let source: Vec<_> = (0..2)
                    .flat_map(|y| {
                        (0..3).flat_map(move |x| {
                            let value = ((y + tile.position.1) * 6 + x + tile.position.0) as u8;
                            std::iter::repeat_n(value, channels)
                        })
                    })
                    .collect();
                grid.copy_tile(tile, &source, channels, &mut output, &|| true)
                    .expect("tile");
                assert!(matches!(
                    grid.copy_tile(tile, &source, channels, &mut output, &|| false),
                    Err(ImageDecodeError::Cancelled)
                ));
            }
            let expected: Vec<_> = (1..4)
                .flat_map(|y| {
                    (1..6).flat_map(move |x| std::iter::repeat_n((y * 6 + x) as u8, channels))
                })
                .collect();
            assert_eq!(output, expected);
        }
    }

    #[test]
    fn grid_layout_rejects_gaps_overlaps_dimensions_and_budget_excess() {
        assert!(grid().validate(6 * 4 * 4 - 1).is_err());
        assert!(unsigned(-1).is_err());
        for case in 0..8 {
            let mut grid = grid();
            match case {
                0 => grid.tiles.clear(),
                1 => {
                    grid.tiles.pop();
                }
                2 => grid.tiles[1].position = (0, 0),
                3 => grid.tiles[2].position = (0, 3),
                4 => grid.tiles[1].size = (2, 2),
                5 => grid.tiles[0].size = (0, 2),
                6 => grid.origin = (2, 2),
                _ => grid.size = (u32::MAX, 2),
            }
            assert!(grid.validate(1024).is_err(), "case={case}");
        }
    }
}
