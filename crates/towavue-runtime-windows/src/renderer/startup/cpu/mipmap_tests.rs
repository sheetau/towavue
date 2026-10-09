use super::*;
fn reference(pixels: [Color32; 4]) -> Color32 {
    let channels: [u8; 4] = std::array::from_fn(|c| {
        let sum: u16 = pixels.iter().map(|p| u16::from(p[c])).sum();
        let q = sum / 4;
        (q + u16::from(sum % 4 > 2 || (sum % 4 == 2 && q % 2 == 1))) as u8
    });
    Color32::from_rgba_premultiplied(channels[0], channels[1], channels[2], channels[3])
}
#[test]
fn packed_average_covers_all_high_remainder_sums_and_lane_carries() {
    for high in 0..=252u16 {
        for low in 0..=12u16 {
            let mut h = high;
            let mut l = low;
            let values: [u8; 4] = std::array::from_fn(|_| {
                let hi = h.min(63);
                h -= hi;
                let lo = l.min(3);
                l -= lo;
                (hi * 4 + lo) as u8
            });
            for lane in 0..4 {
                for background in [0u8, 255] {
                    let pixels = values.map(|v| {
                        let mut p = [background; 4];
                        p[lane] = v;
                        Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3])
                    });
                    assert_eq!(average_four(pixels), reference(pixels));
                }
            }
        }
    }
    let mut random = 0x1234_5678u32;
    for _ in 0..100_000 {
        let pixels = std::array::from_fn(|_| {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            let [r, g, b, a] = random.to_le_bytes();
            Color32::from_rgba_premultiplied(r, g, b, a)
        });
        assert_eq!(average_four(pixels), reference(pixels));
    }
}

fn scalar_mipmaps(base: Arc<ColorImage>, enabled: bool) -> Vec<Arc<ColorImage>> {
    let mut levels = vec![base];
    if enabled {
        while levels.last().expect("retained base mip level").size != [1, 1] {
            let src = levels.last().expect("retained base mip level");
            let [sw, sh] = src.size;
            let [dw, dh] = [(sw / 2).max(1), (sh / 2).max(1)];
            let [rx, ry] = [sw as f32 / dw as f32, sh as f32 / dh as f32];
            let mut out = ColorImage::filled([dw, dh], Color32::TRANSPARENT);
            if sw % 2 == 0 && sh % 2 == 0 {
                // Exact 2x2 area integration is a four-byte sum per channel.
                // Odd extents retain the same overlap-weighted integration below.
                for (y, row) in out.pixels.chunks_exact_mut(dw).enumerate() {
                    let top = &src.pixels[y * 2 * sw..][..sw];
                    let bottom = &src.pixels[(y * 2 + 1) * sw..][..sw];
                    for (x, dst) in row.iter_mut().enumerate() {
                        let channels: [u8; 4] = std::array::from_fn(|c| {
                            let sum = top[x * 2][c] as u16
                                + top[x * 2 + 1][c] as u16
                                + bottom[x * 2][c] as u16
                                + bottom[x * 2 + 1][c] as u16;
                            let quotient = sum / 4;
                            (quotient
                                + u16::from(sum % 4 > 2 || (sum % 4 == 2 && quotient % 2 == 1)))
                                as u8
                        });
                        *dst = Color32::from_rgba_premultiplied(
                            channels[0],
                            channels[1],
                            channels[2],
                            channels[3],
                        );
                    }
                }
                levels.push(Arc::new(out));
                continue;
            }
            for y in 0..dh {
                for x in 0..dw {
                    let [bx, by, ex, ey] = [
                        x as f32 * rx,
                        y as f32 * ry,
                        (x + 1) as f32 * rx,
                        (y + 1) as f32 * ry,
                    ];
                    let mut sum = [0.0; 4];
                    for sy in by.floor() as usize..(by.floor() as usize + 3).min(sh) {
                        for sx in bx.floor() as usize..(bx.floor() as usize + 3).min(sw) {
                            let wx = (ex.min((sx + 1) as f32) - bx.max(sx as f32)).max(0.0);
                            let wy = (ey.min((sy + 1) as f32) - by.max(sy as f32)).max(0.0);
                            for (c, value) in sum.iter_mut().enumerate() {
                                *value += src.pixels[sy * sw + sx][c] as f32 * wx * wy;
                            }
                        }
                    }
                    out.pixels[y * dw + x] = Color32::from_rgba_premultiplied(
                        unorm(sum[0] / (rx * ry)),
                        unorm(sum[1] / (rx * ry)),
                        unorm(sum[2] / (rx * ry)),
                        unorm(sum[3] / (rx * ry)),
                    );
                }
            }
            levels.push(Arc::new(out));
        }
    }
    levels
}

#[test]
fn mip_chains_match_scalar_rounding_for_odd_even_and_single_axis_extents() {
    for size in [
        [1, 1],
        [1, 37],
        [38, 1],
        [2, 2],
        [3, 5],
        [4, 6],
        [17, 32],
        [63, 64],
        [135, 240],
        [192, 108],
        [191, 107],
    ] {
        let mut state = 0x3141_5926u32;
        let mut image = ColorImage::filled(size, Color32::TRANSPARENT);
        for pixel in &mut image.pixels {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let [r, g, b, a] = state.to_le_bytes();
            *pixel = Color32::from_rgba_premultiplied(r, g, b, a);
        }
        let image = Arc::new(image);
        for enabled in [false, true] {
            let expected = scalar_mipmaps(image.clone(), enabled);
            let actual = mipmaps(image.clone(), enabled);
            assert_eq!(actual.len(), expected.len());
            assert!(Arc::ptr_eq(&actual[0], &image));
            for (actual, expected) in actual.iter().zip(expected.iter()) {
                assert_eq!(actual.size, expected.size);
                assert_eq!(
                    actual.pixels, expected.pixels,
                    "source {size:?}, level {:?}",
                    actual.size
                );
            }
        }
    }
}
