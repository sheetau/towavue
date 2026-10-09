//! Safe startup UI rasterization with the same sampling and blending rules as the GPU UI.
use egui::{Color32, ColorImage, TextureFilter, TextureId, TextureOptions, TextureWrapMode};
use std::{collections::HashMap, sync::Arc};

struct Texture {
    levels: Vec<Arc<ColorImage>>,
    options: TextureOptions,
}

#[derive(Default)]
pub(super) struct CpuUiRenderer {
    textures: HashMap<TextureId, Texture>,
    last_painted: egui::TexturesDelta,
}

pub(super) fn frame_byte_len([width, height]: [usize; 2]) -> Result<usize, String> {
    if width > i32::MAX as usize || height > i32::MAX as usize {
        return Err("CPU UI dimensions exceed native coordinates".into());
    }
    width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|bytes| *bytes <= isize::MAX as usize)
        .ok_or_else(|| "CPU UI frame size overflow".into())
}

fn unorm(value: f32) -> u8 {
    value.clamp(0.0, 255.0).round_ties_even() as u8
}

fn average_four(pixels: [Color32; 4]) -> Color32 {
    let [a, b, c, d] = pixels.map(|pixel| u32::from_le_bytes(pixel.to_array()));
    // Four independent byte lanes. High-part sums are at most 252 per lane;
    // remainder sums are at most 12, so neither addition carries across lanes.
    let high = ((a & 0xfcfc_fcfc) >> 2)
        + ((b & 0xfcfc_fcfc) >> 2)
        + ((c & 0xfcfc_fcfc) >> 2)
        + ((d & 0xfcfc_fcfc) >> 2);
    let low = (a & 0x0303_0303) + (b & 0x0303_0303) + (c & 0x0303_0303) + (d & 0x0303_0303);
    let quotient = high + ((low >> 2) & 0x0303_0303);
    // Round to nearest, ties to even: remainder 3, or remainder 2 and odd q.
    let round = ((low >> 1) & 0x0101_0101) & ((low | quotient) & 0x0101_0101);
    let [r, g, b, a] = (quotient + round).to_le_bytes();
    Color32::from_rgba_premultiplied(r, g, b, a)
}

fn mipmaps(base: Arc<ColorImage>, enabled: bool) -> Vec<Arc<ColorImage>> {
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
                        *dst = average_four([
                            top[x * 2],
                            top[x * 2 + 1],
                            bottom[x * 2],
                            bottom[x * 2 + 1],
                        ]);
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

fn address(index: i32, size: usize, mode: TextureWrapMode) -> usize {
    let size = size as i32;
    match mode {
        TextureWrapMode::ClampToEdge => index.clamp(0, size - 1) as usize,
        TextureWrapMode::Repeat => index.rem_euclid(size) as usize,
        TextureWrapMode::MirroredRepeat => {
            let index = index.rem_euclid(size * 2);
            (if index < size {
                index
            } else {
                size * 2 - 1 - index
            }) as usize
        }
    }
}

fn sample(image: &ColorImage, uv: [f32; 2], linear: bool, wrap: TextureWrapMode) -> [f32; 4] {
    let [w, h] = image.size;
    let fetch = |x: i32, y: i32| {
        image.pixels[address(y, h, wrap) * w + address(x, w, wrap)]
            .to_array()
            .map(|v| v as f32)
    };
    if !linear {
        // Keep point addressing at full precision. Rounding it to the minimum
        // eight fractional bits changed boundary texels on both tested drivers.
        return fetch(
            (uv[0] * w as f32).floor() as i32,
            (uv[1] * h as f32).floor() as i32,
        );
    }
    let [x, y] = [
        (uv[0] * w as f32 * 256.0 - 128.0).round_ties_even() / 256.0,
        (uv[1] * h as f32 * 256.0 - 128.0).round_ties_even() / 256.0,
    ];
    let [ix, iy] = [x.floor() as i32, y.floor() as i32];
    let [fx, fy] = [x - x.floor(), y - y.floor()];
    let [a, b, c, d] = [
        fetch(ix, iy),
        fetch(ix + 1, iy),
        fetch(ix, iy + 1),
        fetch(ix + 1, iy + 1),
    ];
    std::array::from_fn(|i| {
        (a[i] + (b[i] - a[i]) * fx) * (1.0 - fy) + (c[i] + (d[i] - c[i]) * fx) * fy
    })
}

struct Sampling<'a> {
    first: &'a ColorImage,
    second: Option<&'a ColorImage>,
    weight: f32,
    linear: bool,
    wrap: TextureWrapMode,
}
impl Sampling<'_> {
    fn sample(&self, uv: [f32; 2]) -> [f32; 4] {
        let a = sample(self.first, uv, self.linear, self.wrap);
        if let Some(second) = self.second {
            let b = sample(second, uv, self.linear, self.wrap);
            std::array::from_fn(|c| a[c] + (b[c] - a[c]) * self.weight)
        } else {
            a
        }
    }
}

impl Texture {
    fn sampling(&self, footprint: f32) -> Sampling<'_> {
        let minifying = footprint > 1.0;
        let linear = if minifying {
            self.options.minification
        } else {
            self.options.magnification
        } == TextureFilter::Linear;
        let mut lod = if minifying && self.options.mipmap_mode.is_some() {
            let lod = 0.5 * footprint.log2();
            lod.min(1.0).max(lod - 0.5)
        } else {
            0.0
        };
        lod = lod.clamp(0.0, (self.levels.len() - 1) as f32);
        if self.options.mipmap_mode != Some(TextureFilter::Linear) {
            return Sampling {
                first: &self.levels[lod.round_ties_even() as usize],
                second: None,
                weight: 0.0,
                linear,
                wrap: self.options.wrap_mode,
            };
        }
        let lo = lod.floor() as usize;
        Sampling {
            first: &self.levels[lo],
            second: (lod > lo as f32)
                .then(|| self.levels[(lo + 1).min(self.levels.len() - 1)].as_ref()),
            weight: lod - lo as f32,
            linear,
            wrap: self.options.wrap_mode,
        }
    }
}

fn edge(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

fn inside(value: f32, a: [f32; 2], b: [f32; 2]) -> bool {
    value > 0.0 || (value == 0.0 && (b[1] < a[1] || (b[1] == a[1] && b[0] > a[0])))
}

impl CpuUiRenderer {
    pub(super) fn replay(&self) -> egui::TexturesDelta {
        self.last_painted.clone()
    }

    pub(super) fn render(
        &mut self,
        ctx: &egui::Context,
        output: &egui::FullOutput,
        size: [usize; 2],
    ) -> Result<Vec<u8>, String> {
        let update = self.update(output);
        let result = update.and_then(|()| {
            let pixels = self.draw(ctx, output, size)?;
            self.last_painted = egui::TexturesDelta {
                set: self
                    .textures
                    .iter()
                    .map(|(id, texture)| {
                        (
                            *id,
                            egui::epaint::ImageDelta::full(
                                texture.levels[0].clone(),
                                texture.options,
                            ),
                        )
                    })
                    .collect(),
                free: output.textures_delta.free.clone(),
            };
            Ok(pixels)
        });
        // Free after paint on both upload and draw failures, like the GPU path.
        for id in &output.textures_delta.free {
            self.textures.remove(id);
        }
        result
    }

    fn update(&mut self, output: &egui::FullOutput) -> Result<(), String> {
        for (id, delta) in &output.textures_delta.set {
            let egui::ImageData::Color(image) = &delta.image;
            // egui can submit an empty partial atlas update. It still validates
            // its offset and options below, but must preserve the existing image.
            if (delta.pos.is_none() && image.size.contains(&0))
                || image.size[0].checked_mul(image.size[1]) != Some(image.pixels.len())
            {
                return Err("invalid CPU UI texture dimensions".into());
            }
            let base = if let Some([x, y]) = delta.pos {
                let texture = self
                    .textures
                    .get_mut(id)
                    .ok_or("partial update without base")?;
                let mut base = texture.levels[0].clone();
                let base_mut = Arc::make_mut(&mut base);
                if x.checked_add(image.size[0])
                    .is_none_or(|end| end > base_mut.size[0])
                    || y.checked_add(image.size[1])
                        .is_none_or(|end| end > base_mut.size[1])
                {
                    return Err("partial update out of bounds".into());
                }
                for row in 0..image.size[1] {
                    let begin = (y + row) * base_mut.size[0] + x;
                    base_mut.pixels[begin..begin + image.size[0]].copy_from_slice(
                        &image.pixels[row * image.size[0]..(row + 1) * image.size[0]],
                    );
                }
                base
            } else {
                image.clone()
            };
            self.textures.insert(
                *id,
                Texture {
                    levels: mipmaps(base, delta.options.mipmap_mode.is_some()),
                    options: delta.options,
                },
            );
        }
        Ok(())
    }

    fn draw(
        &self,
        ctx: &egui::Context,
        output: &egui::FullOutput,
        [width, height]: [usize; 2],
    ) -> Result<Vec<u8>, String> {
        let bytes = frame_byte_len([width, height])?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(bytes)
            .map_err(|error| format!("Could not allocate CPU UI frame: {error}"))?;
        pixels.resize(bytes, 0_u8);
        for pixel in pixels.as_chunks_mut::<4>().0.iter_mut() {
            pixel[3] = 255;
        }
        let ppp = output.pixels_per_point;
        for primitive in ctx.tessellate(output.shapes.clone(), ppp / ctx.zoom_factor()) {
            let (mesh, invert) = match primitive.primitive {
                egui::epaint::Primitive::Mesh(mesh) => (mesh, false),
                egui::epaint::Primitive::Callback(callback) => (
                    callback
                        .callback
                        .downcast_ref::<egui_directx11::InvertMesh>()
                        .ok_or("unsupported native callback")?
                        .0
                        .clone(),
                    true,
                ),
            };
            if mesh.indices.is_empty() {
                continue;
            }
            if !mesh.is_valid() || mesh.indices.len() % 3 != 0 {
                return Err("invalid mesh".into());
            }
            let tex = self
                .textures
                .get(&mesh.texture_id)
                .ok_or("missing or native-only texture")?;
            let clip = primitive.clip_rect * ppp;
            let scissor = [
                (clip.min.x as i32).clamp(0, width as i32),
                (clip.min.y as i32).clamp(0, height as i32),
                (clip.max.x as i32).clamp(0, width as i32),
                (clip.max.y as i32).clamp(0, height as i32),
            ];
            for triangle in mesh.indices.as_chunks::<3>().0.iter() {
                let mut v = triangle.map_array(|index| mesh.vertices[index as usize]);
                let mut p = v.map(|v| [v.pos.x * ppp, v.pos.y * ppp]);
                let mut area = edge(p[0], p[1], p[2]);
                if area == 0.0 {
                    continue;
                }
                if area < 0.0 {
                    v.swap(1, 2);
                    p.swap(1, 2);
                    area = -area;
                }
                let snapped = p.map(|pos| pos.map(|x| (x * 256.0).round_ties_even() / 256.0));
                if edge(snapped[0], snapped[1], snapped[2]) <= 0.0 {
                    continue;
                }
                // Attribute interpolation, not only coverage, uses snapped vertices.
                p = snapped;
                area = edge(p[0], p[1], p[2]);
                let [min_x, min_y] = [
                    p.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min).floor() as i32,
                    p.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min).floor() as i32,
                ];
                let [max_x, max_y] = [
                    p.iter()
                        .map(|p| p[0])
                        .fold(f32::NEG_INFINITY, f32::max)
                        .ceil() as i32,
                    p.iter()
                        .map(|p| p[1])
                        .fold(f32::NEG_INFINITY, f32::max)
                        .ceil() as i32,
                ];
                let uv = v.map(|v| [v.uv.x, v.uv.y]);
                let dx: [f32; 2] = std::array::from_fn(|c| {
                    ((uv[1][c] - uv[0][c]) * (p[2][1] - p[0][1])
                        - (uv[2][c] - uv[0][c]) * (p[1][1] - p[0][1]))
                        / area
                        * tex.levels[0].size[c] as f32
                });
                let dy: [f32; 2] = std::array::from_fn(|c| {
                    ((p[1][0] - p[0][0]) * (uv[2][c] - uv[0][c])
                        - (p[2][0] - p[0][0]) * (uv[1][c] - uv[0][c]))
                        / area
                        * tex.levels[0].size[c] as f32
                });
                let footprint = (dx[0] * dx[0] + dx[1] * dx[1]).max(dy[0] * dy[0] + dy[1] * dy[1]);
                let sampling = tex.sampling(footprint);
                let uniform = v[0].color == v[1].color && v[0].color == v[2].color;
                let solid = if uniform && uv[0] == uv[1] && uv[0] == uv[2] {
                    let texel = sampling.sample(uv[0]);
                    Some(std::array::from_fn::<_, 4, _>(|c| {
                        texel[c] * v[0].color[c] as f32 / 255.0
                    }))
                } else {
                    None
                };
                let opaque_solid = solid
                    .filter(|color| !invert && color[3] == 255.0)
                    .map(|color| color.map(unorm));
                for y in min_y.max(scissor[1])..max_y.min(scissor[3]) {
                    for x in min_x.max(scissor[0])..max_x.min(scissor[2]) {
                        let at = [x as f32 + 0.5, y as f32 + 0.5];
                        if !(0..3).all(|i| {
                            inside(
                                edge(snapped[(i + 1) % 3], snapped[(i + 2) % 3], at),
                                snapped[(i + 1) % 3],
                                snapped[(i + 2) % 3],
                            )
                        }) {
                            continue;
                        }
                        let pixel_start = (y as usize * width + x as usize) * 4;
                        if let Some(color) = opaque_solid {
                            pixels[pixel_start..pixel_start + 4].copy_from_slice(&color);
                            continue;
                        }
                        let src = solid.unwrap_or_else(|| {
                            let bary: [f32; 3] = std::array::from_fn(|i| {
                                edge(p[(i + 1) % 3], p[(i + 2) % 3], at) / area
                            });
                            let texel = sampling.sample(std::array::from_fn(|c| {
                                (0..3).map(|i| bary[i] * uv[i][c]).sum()
                            }));
                            std::array::from_fn(|c| {
                                texel[c]
                                    * if uniform {
                                        v[0].color[c] as f32 / 255.0
                                    } else {
                                        (0..3)
                                            .map(|i| bary[i] * v[i].color[c] as f32 / 255.0)
                                            .sum::<f32>()
                                    }
                            })
                        });
                        let pixel = &mut pixels[pixel_start..][..4];
                        // Fully opaque ordinary fragments cannot depend on destination color.
                        // Retain ordinary floating-point sampling and final UNORM rounding.
                        if !invert && src[3] == 255.0 {
                            pixel.copy_from_slice(&src.map(unorm));
                            continue;
                        }
                        let dst: [f32; 4] = std::array::from_fn(|c| pixel[c] as f32);
                        for c in 0..3 {
                            pixel[c] = unorm(
                                src[c] * if invert { 1.0 - dst[c] / 255.0 } else { 1.0 }
                                    + dst[c] * (1.0 - src[3] / 255.0),
                            );
                        }
                        if !invert {
                            pixel[3] = unorm(src[3] * (1.0 - dst[3] / 255.0) + dst[3]);
                        }
                    }
                }
            }
        }
        Ok(pixels)
    }
}

trait Triple {
    fn map_array<T>(&self, f: impl Fn(u32) -> T) -> [T; 3];
}
impl Triple for [u32] {
    fn map_array<T>(&self, f: impl Fn(u32) -> T) -> [T; 3] {
        [f(self[0]), f(self[1]), f(self[2])]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{
        Pos2, Rect, Shape,
        epaint::{ClippedShape, ImageDelta, Mesh},
    };

    fn fixture(ctx: &egui::Context, id: TextureId) -> egui::FullOutput {
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        output.pixels_per_point = 1.0;
        let rect = Rect::from_min_max(Pos2::ZERO, egui::pos2(4.0, 4.0));
        let mut mesh = Mesh::with_texture(id);
        mesh.add_rect_with_uv(
            rect,
            Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        output.shapes = vec![ClippedShape {
            clip_rect: rect,
            shape: Shape::mesh(mesh),
        }];
        output
    }

    #[test]
    fn failed_upload_still_releases_last_frame_texture() {
        let ctx = egui::Context::default();
        let id = TextureId::Managed(7);
        let mut output = fixture(&ctx, id);
        output.textures_delta.set.push((
            id,
            ImageDelta::full(
                ColorImage::filled([2, 2], Color32::RED),
                TextureOptions::NEAREST,
            ),
        ));
        let mut cpu = CpuUiRenderer::default();
        let pixels = cpu
            .render(&ctx, &output, [4, 4])
            .expect("native startup control");
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [255, 0, 0, 255])
        );
        output.textures_delta.clear();
        output.textures_delta.set.push((
            id,
            ImageDelta::partial(
                [2, 2],
                ColorImage::filled([1, 1], Color32::WHITE),
                TextureOptions::LINEAR,
            ),
        ));
        output.textures_delta.free.push(id);
        assert_eq!(
            cpu.render(&ctx, &output, [4, 4])
                .expect_err("deliberate invalid rendering input"),
            "partial update out of bounds"
        );
        output.textures_delta.clear();
        assert_eq!(
            cpu.render(&ctx, &output, [4, 4])
                .expect_err("deliberate invalid rendering input"),
            "missing or native-only texture"
        );
    }

    #[test]
    fn malformed_texture_and_overflowing_partial_update_still_apply_frees() {
        let ctx = egui::Context::default();
        let id = TextureId::Managed(7);
        let mut base = fixture(&ctx, id);
        base.textures_delta.set.push((
            id,
            ImageDelta::full(
                ColorImage::filled([2, 2], Color32::RED),
                TextureOptions::NEAREST,
            ),
        ));
        let mut short = ColorImage::filled([2, 2], Color32::RED);
        short.pixels.pop();
        for invalid in [
            ImageDelta::full(
                ColorImage::filled([0, 1], Color32::RED),
                TextureOptions::NEAREST,
            ),
            ImageDelta::full(short, TextureOptions::NEAREST),
            ImageDelta::partial(
                [usize::MAX, 0],
                ColorImage::filled([1, 1], Color32::RED),
                TextureOptions::NEAREST,
            ),
        ] {
            let mut cpu = CpuUiRenderer::default();
            cpu.render(&ctx, &base, [4, 4])
                .expect("retained base texture");
            let mut output = base.clone();
            output.textures_delta.clear();
            output.textures_delta.set.push((id, invalid));
            output.textures_delta.free.push(id);
            assert!(cpu.render(&ctx, &output, [4, 4]).is_err());
            output.textures_delta.clear();
            assert_eq!(
                cpu.render(&ctx, &output, [4, 4])
                    .expect_err("failed upload still frees texture"),
                "missing or native-only texture"
            );
        }
    }

    #[test]
    fn empty_partial_updates_preserve_pixels_and_last_frame_replay() {
        let ctx = egui::Context::default();
        let id = TextureId::Managed(7);
        let mut output = fixture(&ctx, id);
        output.textures_delta.set.push((
            id,
            ImageDelta::full(
                ColorImage::filled([2, 2], Color32::RED),
                TextureOptions::NEAREST,
            ),
        ));
        let mut cpu = CpuUiRenderer::default();
        let expected = cpu.render(&ctx, &output, [4, 4]).expect("base image");
        for (pos, size) in [([0, 0], [0, 0]), ([2, 0], [0, 2]), ([0, 2], [2, 0])] {
            output.textures_delta.clear();
            output.textures_delta.set.push((
                id,
                ImageDelta::partial(
                    pos,
                    ColorImage::filled(size, Color32::TRANSPARENT),
                    TextureOptions::NEAREST,
                ),
            ));
            assert_eq!(
                cpu.render(&ctx, &output, [4, 4])
                    .expect("empty in-bounds patch"),
                expected
            );
        }
        output.textures_delta.clear();
        output.textures_delta.free.push(id);
        assert_eq!(
            cpu.render(&ctx, &output, [4, 4])
                .expect("last frame before free"),
            expected
        );
        let replay = cpu.replay();
        let (_, delta) = replay
            .set
            .iter()
            .find(|(texture, _)| *texture == id)
            .expect("last painted texture");
        let egui::ImageData::Color(image) = &delta.image;
        assert_eq!(image.pixels, vec![Color32::RED; 4]);
        assert_eq!(replay.free, vec![id]);
        output.textures_delta.clear();
        assert_eq!(
            cpu.render(&ctx, &output, [4, 4])
                .expect_err("texture was freed after paint"),
            "missing or native-only texture"
        );
    }

    #[test]
    fn native_textures_and_unknown_callbacks_cannot_silently_disappear() {
        let ctx = egui::Context::default();
        let mut output = fixture(&ctx, TextureId::User(5));
        let mut cpu = CpuUiRenderer::default();
        assert_eq!(
            cpu.render(&ctx, &output, [4, 4])
                .expect_err("deliberate invalid rendering input"),
            "missing or native-only texture"
        );
        let rect = Rect::from_min_max(Pos2::ZERO, egui::pos2(4.0, 4.0));
        output.shapes = vec![ClippedShape {
            clip_rect: rect,
            shape: Shape::Callback(egui::PaintCallback {
                rect,
                callback: Arc::new(()),
            }),
        }];
        output.textures_delta.clear();
        assert_eq!(
            cpu.render(&ctx, &output, [4, 4])
                .expect_err("deliberate invalid rendering input"),
            "unsupported native callback"
        );
    }
}

#[cfg(test)]
mod mipmap_tests;
