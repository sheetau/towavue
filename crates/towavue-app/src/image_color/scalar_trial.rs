//! Historical scalar conversion for verification against the production packed path.
use egui::Color32;

pub(crate) fn color_image(
    frame: &towavue_runtime_windows::DecodedImageFrame,
    lookup: bool,
) -> egui::ColorImage {
    let size = [frame.width as usize, frame.height as usize];
    assert_eq!(size[0] * size[1] * 4, frame.rgba.len());
    let mut pixels = Vec::with_capacity(size[0] * size[1]);
    for row in frame.rgba.chunks_exact(size[0].max(1) * 4) {
        let row = row.as_chunks::<4>().0;
        // Opaque rows need no alpha conversion; mixed rows keep egui's exact rounding.
        // Reduce whole pixels in bounded blocks while retaining early exit for mixed rows.
        let alpha_mask = u32::from_ne_bytes([0, 0, 0, 255]);
        if row.chunks(32).all(|block| {
            block
                .iter()
                .fold(u32::MAX, |bits, pixel| bits & u32::from_ne_bytes(*pixel))
                & alpha_mask
                == alpha_mask
        }) {
            pixels.extend(
                row.iter()
                    .map(|p| Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3])),
            );
        } else {
            if lookup {
                pixels.extend(
                    row.iter()
                        .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])),
                );
                continue;
            }
            pixels.extend(row.iter().copied().map(premultiplied_color));
        }
    }
    egui::ColorImage::new(size, pixels)
}

fn premultiplied_color([r, g, b, a]: [u8; 4]) -> Color32 {
    // Rounded component * alpha / 255 matches egui, including transparent hidden RGB.
    // Bounded u16 arithmetic avoids per-pixel table lookups and initialization checks.
    let channel = |value: u8| {
        let product = u16::from(value) * u16::from(a) + 128;
        ((product + (product >> 8)) >> 8) as u8
    };
    Color32::from_rgba_premultiplied(channel(r), channel(g), channel(b), a)
}

#[test]
fn integer_premultiplication_matches_every_egui_component_and_alpha() {
    for alpha in 0_u8..=255 {
        for value in 0_u8..=255 {
            let pixel = [value, 255 - value, value.wrapping_mul(73), alpha];
            assert_eq!(
                premultiplied_color(pixel),
                Color32::from_rgba_unmultiplied(pixel[0], pixel[1], pixel[2], pixel[3]),
                "component={value}, alpha={alpha}"
            );
        }
    }
}
