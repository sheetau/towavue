//! Exact packed RGBA premultiplication for image presentation.

use egui::{Color32, ColorImage};

pub fn premultiplied_color_image(frame: &crate::DecodedImageFrame) -> ColorImage {
    let size = [frame.width as usize, frame.height as usize];
    assert_eq!(
        size[0].checked_mul(size[1]).and_then(|n| n.checked_mul(4)),
        Some(frame.rgba.len())
    );
    ColorImage::new(size, pixels(&frame.rgba))
}

fn pixels(rgba: &[u8]) -> Vec<Color32> {
    assert_eq!(rgba.len() % 4, 0);
    #[cfg(not(target_arch = "x86_64"))]
    return rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
        .collect();

    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::*;

        const {
            assert!(std::mem::size_of::<Color32>() == 4);
        }
        let count = rgba.len() / 4;
        let mut output = Vec::<Color32>::with_capacity(count);
        let mut index = 0;
        // SAFETY: SSE2 is baseline on x86-64. Each iteration reads four complete
        // pixels and initializes four reserved output slots with unaligned access.
        // Pinned ecolor's Color32 is repr(C), a single [u8; 4] field (alignment 4);
        // every byte pattern is valid. The tail initializes the remaining slots
        // before set_len, and neither the pointers nor uninitialized slots escape.
        unsafe {
            let zero = _mm_setzero_si128();
            let mask = _mm_set1_epi32(0xff00_0000_u32 as i32);
            let alpha_lane = _mm_set_epi16(255, 0, 0, 0, 255, 0, 0, 0);
            let bias = _mm_set1_epi16(128);
            while index + 4 <= count {
                let source = _mm_loadu_si128(rgba.as_ptr().add(index * 4).cast());
                let opaque =
                    _mm_movemask_epi8(_mm_cmpeq_epi8(_mm_and_si128(source, mask), mask)) == 0xffff;
                let result = if opaque {
                    source
                } else {
                    let convert = |components| {
                        let alpha =
                            _mm_shufflehi_epi16::<0xff>(_mm_shufflelo_epi16::<0xff>(components));
                        // Multiply the alpha channel by 255 to preserve it. Both
                        // additions stay below 65536; the final lanes are 0..=255.
                        let product = _mm_add_epi16(
                            _mm_mullo_epi16(components, _mm_or_si128(alpha, alpha_lane)),
                            bias,
                        );
                        _mm_srli_epi16::<8>(_mm_add_epi16(product, _mm_srli_epi16::<8>(product)))
                    };
                    _mm_packus_epi16(
                        convert(_mm_unpacklo_epi8(source, zero)),
                        convert(_mm_unpackhi_epi8(source, zero)),
                    )
                };
                _mm_storeu_si128(output.as_mut_ptr().add(index).cast(), result);
                index += 4;
            }
            for p in rgba[index * 4..].as_chunks::<4>().0 {
                output
                    .as_mut_ptr()
                    .add(index)
                    .write(Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]));
                index += 1;
            }
            output.set_len(count);
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_colors_preserve_every_component_alpha_and_unaligned_tail() {
        let rgba: Vec<_> = (0..=255_u8)
            .flat_map(|alpha| {
                (0..=255_u8)
                    .flat_map(move |value| [value, 255 - value, value.wrapping_mul(73), alpha])
            })
            .collect();
        let expected: Vec<_> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
            .collect();
        assert!(pixels(&rgba) == expected);
        for offset in 0..16 {
            for count in 0..36 {
                for alpha in [0, 128, 255] {
                    let mut bytes = vec![0x5a; offset];
                    bytes.extend((0..count).flat_map(|n| {
                        [
                            n as u8,
                            (n * 7) as u8,
                            201,
                            if n % 3 == 0 { alpha } else { 255 },
                        ]
                    }));
                    let before = bytes.clone();
                    let input = &bytes[offset..];
                    let expected: Vec<_> = input
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
                        .collect();
                    assert!(
                        pixels(input) == expected,
                        "offset={offset}, count={count}, alpha={alpha}"
                    );
                    assert_eq!(bytes, before, "input remains unchanged");
                }
            }
        }
    }
}
