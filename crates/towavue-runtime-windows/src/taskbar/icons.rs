use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, HICON, ICONINFO};

struct Icon(HICON);

impl Drop for Icon {
    fn drop(&mut self) {
        // SAFETY: uniquely owned, nonshared icon created below.
        let _ = unsafe { DestroyIcon(self.0) };
    }
}

/// Previous, play, pause and next icons. The Shell copies their pixels on update.
pub struct TaskbarIcons([Icon; 4]);

impl TaskbarIcons {
    /// Accept square, top-down premultiplied RGBA images, up to 256 pixels per side.
    pub fn new(size: u32, images: [&[u8]; 4]) -> Result<Self, Box<dyn std::error::Error>> {
        if !(1..=256).contains(&size)
            || images
                .iter()
                .any(|image| image.len() != size as usize * size as usize * 4)
        {
            return Err("Invalid taskbar icon dimensions".into());
        }
        Ok(Self([
            icon(size, images[0])?,
            icon(size, images[1])?,
            icon(size, images[2])?,
            icon(size, images[3])?,
        ]))
    }

    pub(super) fn handle(&self, index: usize) -> HICON {
        self.0[index].0
    }
}

fn icon(size: u32, rgba: &[u8]) -> windows::core::Result<Icon> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    // SAFETY: validated bounded dimensions; the top-down DIB owns size*size*4 bytes.
    let color = unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)? };
    // SAFETY: successful 32-bit DIB allocation; exclusive write before native copying.
    let pixels = unsafe { std::slice::from_raw_parts_mut(bits.cast::<u8>(), rgba.len()) };
    for (source, target) in rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(pixels.as_chunks_mut::<4>().0)
    {
        // CreateIconIndirect applies alpha when preparing the icon. Supplying
        // premultiplied channels here would darken antialiased edges twice.
        let straight = |channel: u8| {
            if source[3] == 0 {
                0
            } else {
                ((u32::from(channel) * 255 + u32::from(source[3]) / 2) / u32::from(source[3]))
                    .min(255) as u8
            }
        };
        target.copy_from_slice(&[
            straight(source[2]),
            straight(source[1]),
            straight(source[0]),
            source[3],
        ]);
    }
    // A word-aligned monochrome mask is still required for alpha icons.
    let mask_bytes = vec![0u8; (size as usize).div_ceil(16) * 2 * size as usize];
    // SAFETY: bounded word-aligned rows; CreateBitmap copies the supplied bytes.
    let mask = unsafe {
        CreateBitmap(
            size as i32,
            size as i32,
            1,
            1,
            Some(mask_bytes.as_ptr().cast()),
        )
    };
    let result = if mask.0.is_null() {
        Err(windows::core::Error::from_thread())
    } else {
        // SAFETY: both bitmaps remain alive during copying. The returned icon owns
        // independent copies; neither bitmap was selected into a device context.
        unsafe {
            CreateIconIndirect(&ICONINFO {
                fIcon: true.into(),
                hbmMask: mask,
                hbmColor: color,
                ..Default::default()
            })
        }
        .map(Icon)
    };
    // SAFETY: these uniquely owned temporary bitmaps are no longer in use.
    unsafe {
        if !mask.0.is_null() {
            let _ = DeleteObject(mask.into());
        }
        let _ = DeleteObject(color.into());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::{CreateCompatibleDC, DeleteDC, GdiFlush, SelectObject};
    use windows::Win32::UI::WindowsAndMessaging::{DI_NORMAL, DrawIconEx};

    #[test]
    fn native_icon_preserves_top_down_colors_transparency_and_premultiplied_alpha() {
        let mut rgba = vec![0; 4 * 4 * 4];
        rgba[..4].copy_from_slice(&[255, 0, 0, 255]);
        rgba[60..].copy_from_slice(&[0, 128, 0, 128]);
        let icon = icon(4, &rgba).expect("colored alpha icon");
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 4,
                biHeight: -4,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        // SAFETY: all GDI objects are owned by this test thread. Select the bounded
        // DIB only for drawing, flush before reading, then restore/delete objects.
        let result = unsafe {
            let bitmap =
                CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).expect("DIB");
            let dc = CreateCompatibleDC(None);
            assert!(!dc.0.is_null());
            let previous = SelectObject(dc, bitmap.into());
            let pixels = std::slice::from_raw_parts_mut(bits.cast::<u8>(), 64);
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel.copy_from_slice(&[255, 0, 0, 255]);
            }
            let draw = DrawIconEx(dc, 0, 0, icon.0, 4, 4, 0, None, DI_NORMAL);
            let flushed = GdiFlush();
            let result = pixels.to_vec();
            SelectObject(dc, previous);
            let _ = DeleteDC(dc);
            let _ = DeleteObject(bitmap.into());
            draw.expect("DrawIconEx");
            assert!(flushed.as_bool());
            result
        };
        assert_eq!(&result[..3], &[0, 0, 255]);
        assert_eq!(&result[4..7], &[255, 0, 0]);
        assert_eq!(&result[60..63], &[127, 128, 0]);
    }
}
