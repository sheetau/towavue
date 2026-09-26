use super::{SubtitleError, Text};
use ffmpeg_next::subtitle::Bitmap;

/// Palette runs keep an entire track bounded without retaining full RGBA pages.
/// Expand only active cues for upload, then release their temporary pixel buffer.
#[derive(Clone, Debug)]
pub struct SubtitleBitmap {
    pub x: i32,
    pub y: i32,
    canvas: Option<(u32, u32)>,
    width: u32,
    height: u32,
    palette: Vec<[u8; 4]>,
    runs: Vec<(u32, u8)>,
}

impl SubtitleBitmap {
    /// Authored canvas dimensions, when supplied by the subtitle decoder.
    pub fn canvas(&self) -> Option<(u32, u32)> {
        self.canvas
    }
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn rgba(&self) -> Vec<u8> {
        let mut pixels = Vec::with_capacity(self.width as usize * self.height as usize * 4);
        for &(count, color) in &self.runs {
            for _ in 0..count {
                pixels.extend_from_slice(&self.palette[usize::from(color)]);
            }
        }
        pixels
    }
    pub(super) fn storage_size(&self) -> usize {
        self.palette.len() * 4 + self.runs.len() * size_of::<(u32, u8)>()
    }
}

pub(super) fn copy(
    bitmap: &Bitmap<'_>,
    canvas: Option<(u32, u32)>,
    check: &impl Fn() -> Result<(), SubtitleError>,
) -> Result<SubtitleBitmap, SubtitleError> {
    let invalid = || SubtitleError::Message(Text::SubtitleInvalidData);
    // The caller keeps the AVSubtitle owner alive throughout this synchronous
    // copy. Native decoder output owns width-byte indexed rows and an ARGB
    // palette; no pointer/slice leaves this function. Validate scalar bounds
    // before constructing either borrowed slice, including signed row stride.
    let raw = unsafe { &*bitmap.as_ptr() };
    let width = usize::try_from(raw.w).map_err(|_| invalid())?;
    let height = usize::try_from(raw.h).map_err(|_| invalid())?;
    if width == 0
        || height == 0
        || width > 16384
        || height > 16384
        || width
            .checked_mul(height)
            .is_none_or(|pixels| pixels > 16 * 1024 * 1024)
    {
        return Err(SubtitleError::Message(Text::SubtitleTooLarge));
    }
    if raw.data[0].is_null()
        || raw.data[1].is_null()
        || !(1..=256).contains(&raw.nb_colors)
        || raw.linesize[0].unsigned_abs() < width as u32
    {
        return Err(invalid());
    }
    let colors = unsafe { std::slice::from_raw_parts(raw.data[1], raw.nb_colors as usize * 4) };
    let palette = colors
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| {
            let color = u32::from_ne_bytes(*bytes);
            [
                (color >> 16) as u8,
                (color >> 8) as u8,
                color as u8,
                (color >> 24) as u8,
            ]
        })
        .collect::<Vec<_>>();
    let mut runs = Vec::<(u32, u8)>::new();
    for y in 0..height {
        check()?;
        let offset = (y as isize)
            .checked_mul(raw.linesize[0] as isize)
            .ok_or_else(invalid)?;
        let row = unsafe { std::slice::from_raw_parts(raw.data[0].offset(offset), width) };
        for &color in row {
            if usize::from(color) >= palette.len() {
                return Err(invalid());
            }
            if let Some((count, previous)) = runs.last_mut()
                && *previous == color
            {
                *count += 1;
            } else {
                runs.push((1, color));
            }
        }
    }
    Ok(SubtitleBitmap {
        x: raw.x,
        y: raw.y,
        canvas,
        width: width as u32,
        height: height as u32,
        palette,
        runs,
    })
}
