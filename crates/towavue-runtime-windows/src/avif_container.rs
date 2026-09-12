use std::fs;
use std::io::{Read, Seek, SeekFrom};

pub(crate) mod still;

/// Container cropping is separate from AV1 codec cropping and precedes orientation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CleanAperture {
    top: u32,
    bottom: u32,
    left: u32,
    right: u32,
}

impl CleanAperture {
    pub(crate) fn from_clap(data: &[u8], size: (u32, u32)) -> Result<Self, Error> {
        let data: &[u8; 32] = data.try_into().map_err(|_| invalid("invalid clap size"))?;
        let values: [u32; 8] = std::array::from_fn(|i| {
            let i = i * 4;
            u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]])
        });
        let axis = |canvas: u32, extent: u32, divisor: u32, offset: u32, offset_divisor: u32| {
            if divisor == 0 || offset_divisor == 0 || extent == 0 || !extent.is_multiple_of(divisor)
            {
                return Err(invalid("invalid or fractional clean aperture extent"));
            }
            let extent = extent / divisor;
            let denominator = 2 * i128::from(offset_divisor);
            let numerator = (i128::from(canvas) - i128::from(extent)) * i128::from(offset_divisor)
                + 2 * i128::from(offset as i32);
            if numerator < 0 || numerator % denominator != 0 {
                return Err(invalid("outside or fractional clean aperture origin"));
            }
            let start =
                u32::try_from(numerator / denominator).map_err(|_| invalid("aperture overflow"))?;
            let end = canvas
                .checked_sub(start)
                .and_then(|value| value.checked_sub(extent))
                .ok_or_else(|| invalid("clean aperture lies outside canvas"))?;
            Ok((start, end))
        };
        let (left, right) = axis(size.0, values[0], values[1], values[4], values[5])?;
        let (top, bottom) = axis(size.1, values[2], values[3], values[6], values[7])?;
        Ok(Self {
            top,
            bottom,
            left,
            right,
        })
    }

    pub(crate) fn from_bytes(data: &[u8]) -> Result<Self, Error> {
        let data: &[u8; 16] = data
            .try_into()
            .map_err(|_| invalid("invalid aperture side data"))?;
        let [top, bottom, left, right] = [0, 4, 8, 12].map(|offset| {
            u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ])
        });
        Ok(Self {
            top,
            bottom,
            left,
            right,
        })
    }

    pub(crate) fn rectangle(self, size: (u32, u32)) -> Result<towavue_core::PixelCrop, Error> {
        let width = size
            .0
            .checked_sub(self.left)
            .and_then(|value| value.checked_sub(self.right));
        let height = size
            .1
            .checked_sub(self.top)
            .and_then(|value| value.checked_sub(self.bottom));
        match (width, height) {
            (Some(width), Some(height)) if width > 0 && height > 0 => Ok(towavue_core::PixelCrop {
                x: self.left,
                y: self.top,
                width,
                height,
            }),
            _ => Err(invalid("clean aperture lies outside the decoded canvas")),
        }
    }
}

#[cfg(test)]
mod aperture_tests {
    use super::*;

    #[test]
    fn clap_rationals_preserve_integer_bounds_and_reject_ambiguous_geometry() {
        let read = |values: [u32; 8], size| {
            CleanAperture::from_clap(&values.map(u32::to_be_bytes).concat(), size)
        };
        let values = [4, 1, 2, 1, 1, 2, u32::MAX, 2];
        let rect = read(values, (7, 5))
            .expect("half-pixel center offsets")
            .rectangle((7, 5))
            .expect("crop");
        assert_eq!((rect.x, rect.y, rect.width, rect.height), (2, 1, 4, 2));
        for index in [1, 3, 5, 7] {
            let mut invalid = values;
            invalid[index] = 0;
            assert!(read(invalid, (7, 5)).is_err());
        }
        for (index, value) in [
            (0, 0),
            (0, 8),
            (1, 3),
            (4, 0),
            (4, (-99i32) as u32),
            (6, i32::MAX as u32),
        ] {
            let mut invalid = values;
            invalid[index] = value;
            assert!(
                read(invalid, (7, 5)).is_err(),
                "index={index}, value={value}"
            );
        }
        assert!(CleanAperture::from_clap(&[0; 31], (7, 5)).is_err());
        assert!(CleanAperture::from_clap(&[0; 33], (7, 5)).is_err());
        let full = [u32::MAX, 1, u32::MAX, 1, 0, u32::MAX, 0, u32::MAX];
        assert_eq!(
            read(full, (u32::MAX, u32::MAX)).expect("wide arithmetic"),
            CleanAperture::default()
        );
    }

    #[test]
    fn clean_aperture_side_data_rejects_truncation_empty_and_overflowing_bounds() {
        for data in [&[][..], &[0; 15], &[0; 17]] {
            assert!(CleanAperture::from_bytes(data).is_err());
        }
        let read = |values: [u32; 4]| {
            CleanAperture::from_bytes(&values.map(u32::to_le_bytes).concat()).expect("side data")
        };
        let crop = read([1, 2, 2, 1]).rectangle((7, 5)).expect("valid crop");
        assert_eq!(
            crop,
            towavue_core::PixelCrop {
                x: 2,
                y: 1,
                width: 4,
                height: 2
            }
        );
        for values in [
            [5, 0, 0, 0],
            [0, 0, 7, 0],
            [0, 0, 4, 4],
            [0, u32::MAX, 0, 0],
            [0, 0, u32::MAX, u32::MAX],
        ] {
            assert!(read(values).rectangle((7, 5)).is_err());
        }
        assert!(CleanAperture::default().rectangle((0, 5)).is_err());
        assert!(CleanAperture::default().rectangle((7, 0)).is_err());
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("AVIF request was superseded")]
    Cancelled,
}

fn invalid(error: impl std::fmt::Display) -> Error {
    Error::Invalid(error.to_string())
}
fn check_current(current: &dyn Fn() -> bool) -> Result<(), Error> {
    if current() {
        Ok(())
    } else {
        Err(Error::Cancelled)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct BoxRange {
    pub(crate) kind: [u8; 4],
    pub(crate) header: u64,
    pub(crate) start: u64,
    pub(crate) end: u64,
}

pub(crate) fn boxes(
    file: &mut fs::File,
    start: u64,
    end: u64,
    current: &dyn Fn() -> bool,
) -> Result<Vec<BoxRange>, Error> {
    let mut result = Vec::new();
    let mut position = start;
    while position < end {
        check_current(current)?;
        if end - position < 8 || result.len() == 65536 {
            return Err(invalid("invalid box boundary or too many boxes"));
        }
        file.seek(SeekFrom::Start(position)).map_err(Error::Io)?;
        let mut header = [0; 8];
        file.read_exact(&mut header).map_err(Error::Io)?;
        let size = u32::from_be_bytes(header[..4].try_into().expect("box size"));
        let (size, header_size) = match size {
            0 => (end - position, 8),
            1 => {
                let mut size = [0; 8];
                if end - position < 16 {
                    return Err(invalid("incomplete large box"));
                }
                file.read_exact(&mut size).map_err(Error::Io)?;
                (u64::from_be_bytes(size), 16)
            }
            size => (u64::from(size), 8),
        };
        if size < header_size || size > end - position {
            return Err(invalid("box exceeds parent bounds"));
        }
        result.push(BoxRange {
            kind: header[4..].try_into().expect("box kind"),
            header: position,
            start: position + header_size,
            end: position + size,
        });
        position += size;
    }
    Ok(result)
}

pub(crate) fn one(boxes: &[BoxRange], kind: &[u8; 4]) -> Result<Option<BoxRange>, Error> {
    let mut found = boxes.iter().filter(|item| &item.kind == kind);
    let result = found.next().copied();
    if found.next().is_some() {
        return Err(invalid(format!(
            "duplicate {} box",
            String::from_utf8_lossy(kind)
        )));
    }
    Ok(result)
}

pub(crate) fn bytes(file: &mut fs::File, item: BoxRange, limit: usize) -> Result<Vec<u8>, Error> {
    if item.end - item.start > limit as u64 {
        return Err(invalid("oversized control box"));
    }
    file.seek(SeekFrom::Start(item.start)).map_err(Error::Io)?;
    let mut bytes = vec![0; (item.end - item.start) as usize];
    file.read_exact(&mut bytes).map_err(Error::Io)?;
    Ok(bytes)
}

pub(crate) fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    bytes
        .get(offset..offset + 4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_be_bytes)
        .ok_or_else(|| invalid("truncated control field"))
}
pub(crate) fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, Error> {
    bytes
        .get(offset..offset + 8)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u64::from_be_bytes)
        .ok_or_else(|| invalid("truncated control field"))
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Track {
    pub(crate) id: u32,
    pub(crate) timescale: u32,
    pub(crate) duration: u64,
    // None means that no edit list declares repetition; 0 means infinite.
    pub(crate) loops: Option<u32>,
    pub(crate) alpha_for: Option<u32>,
    pub(crate) premultiplied_with: Option<u32>,
}

pub(crate) fn track(
    file: &mut fs::File,
    item: BoxRange,
    current: &dyn Fn() -> bool,
    read_repetition: bool,
) -> Result<Track, Error> {
    let children = boxes(file, item.start, item.end, current)?;
    let header = one(&children, b"tkhd")?.ok_or_else(|| invalid("track has no tkhd"))?;
    let header = bytes(file, header, 128)?;
    let (id, duration) = match header.first() {
        Some(0) => (u32_at(&header, 12)?, u64::from(u32_at(&header, 20)?)),
        Some(1) => (u32_at(&header, 20)?, u64_at(&header, 28)?),
        _ => return Err(invalid("unsupported tkhd version")),
    };
    if id == 0 {
        return Err(invalid("invalid track ID"));
    }
    let mut loops = None;
    if read_repetition && let Some(edts) = one(&children, b"edts")? {
        let edits = boxes(file, edts.start, edts.end, current)?;
        let edit = one(&edits, b"elst")?.ok_or_else(|| invalid("edts has no elst"))?;
        let edit = bytes(file, edit, 64)?;
        if u32_at(&edit, 4)? != 1 {
            return Err(invalid("multiple edit-list segments are not supported"));
        }
        let (segment, time, rate) = match edit.first() {
            Some(0) if edit.len() == 20 => (
                u64::from(u32_at(&edit, 8)?),
                u64::from(u32_at(&edit, 12)?),
                u32_at(&edit, 16)?,
            ),
            Some(1) if edit.len() == 28 => {
                (u64_at(&edit, 8)?, u64_at(&edit, 16)?, u32_at(&edit, 24)?)
            }
            _ => return Err(invalid("invalid elst version or length")),
        };
        if segment == 0 || time != 0 || rate != 65536 || edit[1..3] != [0, 0] || edit[3] & !1 != 0 {
            return Err(invalid("unsupported edit-list time, rate or flags"));
        }
        loops = Some(if edit[3] & 1 == 0 {
            1
        } else {
            if duration == 0 {
                return Err(invalid("repeating track has zero duration"));
            }
            let count = duration.div_ceil(segment);
            if count > i32::MAX as u64 {
                0
            } else {
                count as u32
            }
        });
    }
    let (mut alpha_for, mut premultiplied_with) = (None, None);
    if let Some(references) = one(&children, b"tref")? {
        let references = boxes(file, references.start, references.end, current)?;
        for (kind, value) in [
            (b"auxl", &mut alpha_for),
            (b"prem", &mut premultiplied_with),
        ] {
            if let Some(reference) = one(&references, kind)? {
                let reference = bytes(file, reference, 4)?;
                *value = Some(u32_at(&reference, 0)?);
            }
        }
    }
    let media = one(&children, b"mdia")?.ok_or_else(|| invalid("track has no mdia"))?;
    let media = boxes(file, media.start, media.end, current)?;
    if alpha_for.is_some() {
        // An auxiliary reference alone does not establish that the samples are alpha.
        let mut parent = media.clone();
        for kind in [b"minf", b"stbl"] {
            let item = one(&parent, kind)?.ok_or_else(|| invalid("missing alpha sample table"))?;
            parent = boxes(file, item.start, item.end, current)?;
        }
        let descriptions = one(&parent, b"stsd")?.ok_or_else(|| invalid("missing alpha stsd"))?;
        if descriptions.end - descriptions.start < 8 {
            return Err(invalid("truncated sample descriptions"));
        }
        let descriptions = boxes(file, descriptions.start + 8, descriptions.end, current)?;
        if descriptions.len() != 1 || descriptions[0].kind != *b"av01" {
            return Err(invalid("unsupported alpha sample description"));
        }
        let description = descriptions[0];
        if description.end - description.start < 78 {
            return Err(invalid("truncated visual sample entry"));
        }
        let properties = boxes(file, description.start + 78, description.end, current)?;
        let alpha = one(&properties, b"auxi")?.ok_or_else(|| invalid("missing alpha type"))?;
        if bytes(file, alpha, 48)? != b"\0\0\0\0urn:mpeg:mpegB:cicp:systems:auxiliary:alpha\0" {
            return Err(invalid("unsupported auxiliary type"));
        }
    }
    let media = one(&media, b"mdhd")?.ok_or_else(|| invalid("track has no mdhd"))?;
    let media = bytes(file, media, 64)?;
    let (timescale, media_duration) = match media.first() {
        Some(0) => (u32_at(&media, 12)?, u64::from(u32_at(&media, 16)?)),
        Some(1) => (u32_at(&media, 20)?, u64_at(&media, 24)?),
        _ => return Err(invalid("unsupported mdhd version")),
    };
    if timescale == 0 || timescale > i32::MAX as u32 || media_duration == 0 {
        return Err(invalid("invalid media time base or duration"));
    }
    Ok(Track {
        id,
        timescale,
        duration: media_duration,
        loops,
        alpha_for,
        premultiplied_with,
    })
}
