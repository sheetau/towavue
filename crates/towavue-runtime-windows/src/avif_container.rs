use std::fs;
use std::io::{Read, Seek, SeekFrom};

pub(crate) mod still;

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
