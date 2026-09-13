use super::*;

#[test]
fn anchored_text_pixels_remain_stable_when_only_target_width_changes() -> Result<()> {
    for driver in [D3D_DRIVER_TYPE_WARP, D3D_DRIVER_TYPE_HARDWARE] {
        let mut device = None;
        let mut immediate = None;
        // The test owns offscreen resources; it reads no window or desktop pixels.
        let created = unsafe {
            D3D11CreateDevice(
                None,
                driver,
                windows::Win32::Foundation::HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut immediate),
            )
        };
        if let Err(error) = created {
            if driver == D3D_DRIVER_TYPE_HARDWARE {
                eprintln!("SKIP hardware resize readback: {error}");
                continue;
            }
            return Err(error);
        }
        let device = device.expect("device");
        let immediate = immediate.expect("context");
        for density in [1.0, 1.25, 1.5, 2.0] {
            let context = egui::Context::default();
            context.set_pixels_per_point(density);
            let mut renderer = Renderer::new(&device)?;
            let mut baseline: Option<Vec<u8>> = None;
            for width in (640..681).chain((624..640).rev()).chain(640..650) {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: 128,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_RENDER_TARGET.0 as _,
                    ..Default::default()
                };
                let (mut target, mut staging, mut view) = (None, None, None);
                // Targets and views stay owned through rendering and synchronous readback.
                unsafe {
                    device.CreateTexture2D(&desc, None, Some(&mut target))?;
                    device.CreateTexture2D(
                        &D3D11_TEXTURE2D_DESC {
                            Usage: D3D11_USAGE_STAGING,
                            BindFlags: 0,
                            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as _,
                            ..desc
                        },
                        None,
                        Some(&mut staging),
                    )?;
                    device.CreateRenderTargetView(
                        target.as_ref().expect("target"),
                        None,
                        Some(&mut view),
                    )?;
                }
                let target = target.expect("target");
                let staging = staging.expect("staging");
                let view = view.expect("view");
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width as f32 / density, 128.0 / density),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        let painter = ui.ctx().layer_painter(egui::LayerId::background());
                        for (y, text) in [
                            (8.0, "track.wav   00:00 / 03:00"),
                            (32.0, "C:\\Media\\track.wav"),
                        ] {
                            painter.text(
                                egui::pos2(10.0, y),
                                egui::Align2::LEFT_TOP,
                                text,
                                egui::FontId::proportional(12.0),
                                egui::Color32::from_gray(128),
                            );
                        }
                    },
                );
                let mut output = split_output(output).0;
                // Match towavue's existing workaround for the upstream double zoom.
                output.pixels_per_point /= context.zoom_factor();
                unsafe {
                    immediate.ClearRenderTargetView(&view, &[0.0, 0.0, 0.0, 1.0]);
                }
                renderer.render(&immediate, &view, &context, output)?;
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                let mut pixels = vec![0; 400 * 128 * 4];
                // Map completes the copy; copy each fixed left-hand row while mapped,
                // respecting row pitch, then release the mapping before comparisons.
                unsafe {
                    immediate.CopyResource(&staging, &target);
                    immediate.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
                    for (row, bytes) in pixels.chunks_exact_mut(400 * 4).enumerate() {
                        std::ptr::copy_nonoverlapping(
                            mapped
                                .pData
                                .cast::<u8>()
                                .add(row * mapped.RowPitch as usize),
                            bytes.as_mut_ptr(),
                            bytes.len(),
                        );
                    }
                    immediate.Unmap(&staging, 0);
                }
                if let Some(baseline) = &baseline {
                    let differences: Vec<_> = pixels
                        .iter()
                        .zip(baseline)
                        .map(|(a, b)| a.abs_diff(*b))
                        .filter(|difference| *difference != 0)
                        .collect();
                    assert!(
                        differences.is_empty(),
                        "driver={driver:?}, density={density}, width={width}: changed bytes={}, max delta={:?}",
                        differences.len(),
                        differences.iter().max()
                    );
                } else {
                    assert!(
                        pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[0] > 20),
                        "render actual glyphs"
                    );
                    baseline = Some(pixels);
                }
            }
        }
    }
    Ok(())
}
