use super::*;
use crate::{Renderer, split_output};

struct Surface {
    device: ID3D11Device,
    immediate: ID3D11DeviceContext,
    target: ID3D11Texture2D,
    view: ID3D11RenderTargetView,
    renderer: Renderer,
    context: egui::Context,
    size: [usize; 2],
}

impl Surface {
    fn new(driver: D3D_DRIVER_TYPE, size: [usize; 2]) -> Result<Option<Self>> {
        let Some((device, immediate)) = device(driver)? else {
            return Ok(None);
        };
        Self::with_device(device, immediate, size).map(Some)
    }

    fn with_device(
        device: ID3D11Device,
        immediate: ID3D11DeviceContext,
        size: [usize; 2],
    ) -> Result<Self> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: size[0] as u32,
            Height: size[1] as u32,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            ..Default::default()
        };
        let mut target = None;
        let mut view = None;
        // The fixture owns this offscreen target, view and immediate context.
        unsafe {
            device.CreateTexture2D(&desc, None, Some(&mut target))?;
            device.CreateRenderTargetView(
                target.as_ref().expect("target"),
                None,
                Some(&mut view),
            )?;
        }
        let renderer = Renderer::new(&device)?;
        Ok(Self {
            device,
            immediate,
            target: target.expect("target"),
            view: view.expect("view"),
            renderer,
            context: egui::Context::default(),
            size,
        })
    }

    fn draw(&mut self, texture: TextureId, size: [usize; 2]) -> Result<Vec<Color32>> {
        self.submit(texture, size)?;
        readback(&self.device, &self.immediate, &self.target)
    }

    fn submit(&mut self, texture: TextureId, size: [usize; 2]) -> Result<()> {
        let output = self.context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(self.size[0] as f32, self.size[1] as f32),
                )),
                ..Default::default()
            },
            |ui| {
                ui.ctx().layer_painter(egui::LayerId::background()).image(
                    texture,
                    egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(size[0] as f32, size[1] as f32),
                    ),
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            },
        );
        // Clear/draw/readback are serialized on the owned immediate context.
        unsafe {
            self.immediate.ClearRenderTargetView(&self.view, &[0.0; 4]);
        }
        self.renderer.render(
            &self.immediate,
            &self.view,
            &self.context,
            split_output(output).0,
        )
    }
}

fn checker(size: [usize; 2]) -> egui::ColorImage {
    egui::ColorImage::new(
        size,
        (0..size[0] * size[1])
            .map(|n| {
                if (n % size[0] + n / size[0]) % 2 == 0 {
                    Color32::WHITE
                } else {
                    Color32::BLACK
                }
            })
            .collect(),
    )
}

fn average_error(pixels: &[Color32], stride: usize, size: [usize; 2], expected: u8) -> f64 {
    let mut sum = 0_u64;
    let mut count = 0;
    for y in 1..size[1] - 1 {
        for x in 1..size[0] - 1 {
            let pixel = pixels[y * stride + x];
            sum += u64::from(pixel.r().abs_diff(expected));
            count += 1;
        }
    }
    sum as f64 / count as f64
}

fn mip_pixels(
    surface: &Surface,
    texture: TextureId,
    level: u32,
) -> Result<(Vec<Color32>, [usize; 2])> {
    let Texture::Managed(texture) = &surface.renderer.texture_pool.pool[&texture] else {
        unreachable!()
    };
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        texture.tex.GetDesc(&mut desc);
    }
    desc.Width = (desc.Width >> level).max(1);
    desc.Height = (desc.Height >> level).max(1);
    desc.MipLevels = 1;
    desc.BindFlags = D3D11_BIND_SHADER_RESOURCE.0 as u32;
    desc.MiscFlags = 0;
    let mut copy = None;
    unsafe {
        surface
            .device
            .CreateTexture2D(&desc, None, Some(&mut copy))?;
        surface.immediate.CopySubresourceRegion(
            copy.as_ref().unwrap(),
            0,
            0,
            0,
            0,
            &texture.tex,
            level,
            None,
        );
    }
    Ok((
        readback(&surface.device, &surface.immediate, &copy.unwrap())?,
        [desc.Width as usize, desc.Height as usize],
    ))
}

#[test]
fn mipmapped_minification_reduces_aliasing_on_warp_and_hardware() -> Result<()> {
    for driver in [D3D_DRIVER_TYPE_WARP, D3D_DRIVER_TYPE_HARDWARE] {
        let Some(mut surface) = Surface::new(driver, [128, 128])? else {
            continue;
        };
        for source in [[256, 256], [257, 131], [1023, 767]] {
            let texture = surface.context.load_texture(
                "checker",
                checker(source),
                egui::TextureOptions::LINEAR,
            );
            for size in [[96, 96], [37, 29], [13, 11]] {
                let mut texture = texture.clone();
                texture.set_partial(
                    [0, 0],
                    egui::ColorImage::new([0, 0], vec![]),
                    egui::TextureOptions::LINEAR,
                );
                let before = surface.draw(texture.id(), size)?;
                texture.set_partial(
                    [0, 0],
                    egui::ColorImage::new([0, 0], vec![]),
                    egui::TextureOptions::LINEAR
                        .with_mipmap_mode(Some(egui::TextureFilter::Linear)),
                );
                let after = surface.draw(texture.id(), size)?;
                let old_error = average_error(&before, 128, size, 128);
                let new_error = average_error(&after, 128, size, 128);
                eprintln!(
                    "MINIFY_QUALITY driver={} source={source:?} destination={size:?} old_mae={old_error:.3} mip_mae={new_error:.3}",
                    driver.0
                );
                assert!(old_error > 10.0, "fixture must expose base-level aliasing");
                if new_error >= 3.0 {
                    for level in 0..4 {
                        let (pixels, dimensions) = mip_pixels(&surface, texture.id(), level)?;
                        eprintln!(
                            "MINIFY_LEVEL level={level} dimensions={dimensions:?} mae={:.3}",
                            average_error(&pixels, dimensions[0], dimensions, 128)
                        );
                    }
                }
                assert!(
                    new_error < 3.0,
                    "mipmapped fine pattern must average toward gray: {new_error}"
                );
            }
        }
    }
    Ok(())
}

fn verify_mip_areas(surface: &Surface, id: TextureId, original: &egui::ColorImage) -> Result<()> {
    let (mut previous, mut size) = mip_pixels(surface, id, 0)?;
    assert!(
        previous == original.pixels,
        "base image must remain byte-identical"
    );
    let mut level = 1;
    while size != [1, 1] {
        let (actual, next) = mip_pixels(surface, id, level)?;
        for y in 0..next[1] {
            for x in 0..next[0] {
                let begin = [
                    x as f64 * size[0] as f64 / next[0] as f64,
                    y as f64 * size[1] as f64 / next[1] as f64,
                ];
                let end = [
                    (x + 1) as f64 * size[0] as f64 / next[0] as f64,
                    (y + 1) as f64 * size[1] as f64 / next[1] as f64,
                ];
                let mut sums = [0.0; 4];
                for sy in begin[1].floor() as usize..end[1].ceil() as usize {
                    for sx in begin[0].floor() as usize..end[0].ceil() as usize {
                        let weight = (end[0].min((sx + 1) as f64) - begin[0].max(sx as f64))
                            * (end[1].min((sy + 1) as f64) - begin[1].max(sy as f64));
                        for (sum, channel) in
                            sums.iter_mut().zip(previous[sy * size[0] + sx].to_array())
                        {
                            *sum += f64::from(channel) * weight;
                        }
                    }
                }
                let area = (end[0] - begin[0]) * (end[1] - begin[1]);
                for (sum, channel) in sums.into_iter().zip(actual[y * next[0] + x].to_array()) {
                    assert!(
                        (sum / area - f64::from(channel)).abs() <= 1.1,
                        "mip {level} at {x},{y}: expected {}, actual {channel}",
                        sum / area
                    );
                }
            }
        }
        previous = actual;
        size = next;
        level += 1;
    }
    Ok(())
}

#[test]
fn mip_areas_preserve_base_alpha_thin_edges_and_partial_updates() -> Result<()> {
    use windows::core::Interface;
    let options = egui::TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear));
    for driver in [D3D_DRIVER_TYPE_WARP, D3D_DRIVER_TYPE_HARDWARE] {
        let Some(mut surface) = Surface::new(driver, [160, 128])? else {
            continue;
        };
        for size in [[1, 1], [1, 17], [17, 1], [3, 5], [65, 37]] {
            let mut original = egui::ColorImage::new(
                size,
                (0..size[0] * size[1])
                    .map(|n| {
                        Color32::from_rgba_unmultiplied(
                            (n * 31) as u8,
                            (n * 67) as u8,
                            (n * 13) as u8,
                            (n * 71) as u8,
                        )
                    })
                    .collect(),
            );
            let mut texture = surface
                .context
                .load_texture("alpha", original.clone(), options);
            let magnified = [size[0] * 2, size[1] * 2];
            let before = surface.draw(texture.id(), magnified)?;
            verify_mip_areas(&surface, texture.id(), &original)?;

            texture.set_partial(
                [0, 0],
                egui::ColorImage::new([0, 0], vec![]),
                egui::TextureOptions::LINEAR,
            );
            assert!(
                surface.draw(texture.id(), magnified)? == before,
                "mip mode cannot alter magnified linear pixels"
            );
            let Texture::Managed(light) = &surface.renderer.texture_pool.pool[&texture.id()] else {
                unreachable!()
            };
            let mut light_desc = D3D11_TEXTURE2D_DESC::default();
            unsafe {
                light.tex.GetDesc(&mut light_desc);
            }
            assert_eq!(
                light_desc.MipLevels, 1,
                "light mode releases the extra chain on draw"
            );
            assert!(mip_pixels(&surface, texture.id(), 0)?.0 == original.pixels);
            texture.set_partial(
                [0, 0],
                egui::ColorImage::new([0, 0], vec![]),
                egui::TextureOptions::NEAREST,
            );
            let point_pixels = surface.draw(texture.id(), magnified)?;
            let point_magnification = egui::TextureOptions {
                magnification: egui::TextureFilter::Nearest,
                ..options
            };
            texture.set_partial(
                [0, 0],
                egui::ColorImage::new([0, 0], vec![]),
                point_magnification,
            );
            assert!(
                surface.draw(texture.id(), magnified)? == point_pixels,
                "high-quality minification preserves exact nearest magnification"
            );
            let allocation = surface
                .renderer
                .texture_pool
                .get_srv(texture.id())
                .unwrap()
                .as_raw();
            for sampling in [point_magnification, options] {
                texture.set_partial([0, 0], egui::ColorImage::new([0, 0], vec![]), sampling);
                surface.draw(texture.id(), magnified)?;
                assert_eq!(
                    surface
                        .renderer
                        .texture_pool
                        .get_srv(texture.id())
                        .unwrap()
                        .as_raw(),
                    allocation,
                    "sampling changes reuse the mip resource"
                );
            }
            original.pixels[0] = Color32::from_rgba_premultiplied(70, 20, 10, 90);
            texture.set_partial(
                [0, 0],
                egui::ColorImage::new([1, 1], vec![original.pixels[0]]),
                options,
            );
            surface.draw(texture.id(), [16, 16])?;
            verify_mip_areas(&surface, texture.id(), &original)?;
            assert_eq!(
                surface
                    .renderer
                    .texture_pool
                    .get_srv(texture.id())
                    .unwrap()
                    .as_raw(),
                allocation,
                "partial updates reuse storage and refresh the chain"
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "generated large-image mip cost control; use Release on hardware"]
#[allow(clippy::assertions_on_constants)]
fn mip_generation_reports_upload_draw_and_texel_costs() -> Result<()> {
    use std::time::{Duration, Instant};
    use windows::core::BOOL;
    assert!(!cfg!(debug_assertions), "use Release");
    let (device, immediate) = device_with_flags(
        D3D_DRIVER_TYPE_HARDWARE,
        D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
        true,
    )?
    .expect("hardware required");
    let mut surface = Surface::with_device(device, immediate, [640, 480])?;
    let mut query = None;
    unsafe {
        surface.device.CreateQuery(
            &D3D11_QUERY_DESC {
                Query: D3D11_QUERY_EVENT,
                MiscFlags: 0,
            },
            Some(&mut query),
        )?;
    }
    let query = query.unwrap();
    let wait = |ctx: &ID3D11DeviceContext| -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut complete = BOOL(0);
        unsafe {
            ctx.End(&query);
            ctx.Flush();
            while !complete.as_bool() {
                ctx.GetData(
                    &query,
                    Some((&mut complete as *mut BOOL).cast()),
                    mem::size_of::<BOOL>() as u32,
                    0,
                )?;
                assert!(Instant::now() < deadline, "GPU completion deadline");
                if !complete.as_bool() {
                    std::thread::yield_now();
                }
            }
        }
        Ok(())
    };
    let id = TextureId::Managed(1_000_000);
    surface.submit(id, [640, 480])?;
    wait(&surface.immediate)?;
    for size in [[4096, 2304], [8706, 5949]] {
        let original = Arc::new(checker(size));
        for round in 0..3 {
            for mip in if round % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            } {
                let options = egui::TextureOptions::LINEAR
                    .with_mipmap_mode(mip.then_some(egui::TextureFilter::Linear));
                let started = Instant::now();
                surface.renderer.texture_pool.update(
                    &surface.immediate,
                    TexturesDelta {
                        set: vec![(
                            id,
                            egui::epaint::ImageDelta::full(original.clone(), options),
                        )],
                        free: vec![],
                    },
                )?;
                surface
                    .renderer
                    .texture_pool
                    .prepare_for_draw(&surface.immediate, id)?;
                let submitted = started.elapsed();
                wait(&surface.immediate)?;
                let completed = started.elapsed();
                let mut draw_total = Duration::ZERO;
                for _ in 0..5 {
                    let start = Instant::now();
                    surface.submit(id, [640, 480])?;
                    wait(&surface.immediate)?;
                    draw_total += start.elapsed();
                }
                let Texture::Managed(texture) = &surface.renderer.texture_pool.pool[&id] else {
                    unreachable!()
                };
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                unsafe {
                    texture.tex.GetDesc(&mut desc);
                }
                let bytes: u64 = (0..desc.MipLevels)
                    .map(|level| {
                        u64::from((desc.Width >> level).max(1))
                            * u64::from((desc.Height >> level).max(1))
                            * 4
                    })
                    .sum();
                assert!(
                    mip_pixels(&surface, id, 0)?.0 == original.pixels,
                    "base equality outside timing"
                );
                assert_eq!(Arc::strong_count(&original), 1, "no retained CPU shadow");
                eprintln!(
                    "MINIFY_COST size={size:?} round={round} mip={mip} submit_ms={:.3} complete_ms={:.3} draw_complete_ms={:.3} logical_texel_bytes={bytes}",
                    submitted.as_secs_f64() * 1000.0,
                    completed.as_secs_f64() * 1000.0,
                    draw_total.as_secs_f64() * 200.0
                );
            }
        }
    }
    Ok(())
}
