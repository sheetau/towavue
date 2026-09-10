use super::*;
use crate::{NativeCaption, PlaybackSession};
use std::sync::Arc;
use std::time::{Duration, Instant};
use towavue_core::{PlaybackRange, UnitPoint};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_USAGE_STAGING,
};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::window::{Window, WindowId};

// Test-only readback. The local staging resource and row slices remain valid until
// Unmap; no mapped pointer escapes. Production playback never takes this CPU path.
fn pixels(renderer: &FrameRenderer) -> Vec<u8> {
    unsafe {
        let source: ID3D11Texture2D = renderer.swap_chain.GetBuffer(0).expect("back buffer");
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        source.GetDesc(&mut desc);
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        desc.MiscFlags = 0;
        let mut staging = None;
        renderer
            .graphics_device
            .device
            .CreateTexture2D(&desc, None, Some(&mut staging))
            .expect("staging");
        let staging = staging.expect("staging texture");
        renderer.context.CopyResource(&staging, &source);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        renderer
            .context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .expect("readback");
        let row_bytes = desc.Width as usize * 4;
        assert!(row_bytes <= mapped.RowPitch as usize);
        let mut result = Vec::new();
        for row in 0..desc.Height as usize {
            result.extend_from_slice(std::slice::from_raw_parts(
                mapped
                    .pData
                    .cast::<u8>()
                    .add(row * mapped.RowPitch as usize),
                row_bytes,
            ));
        }
        renderer.context.Unmap(&staging, 0);
        result
    }
}

fn draw(renderer: &mut FrameRenderer, session: &mut PlaybackSession) -> Vec<u8> {
    let (width, height) = renderer.buffer_dimensions.expect("sized surface");
    renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
    let uv = [
        UnitPoint { x: 0.0, y: 0.0 },
        UnitPoint { x: 1.0, y: 0.0 },
        UnitPoint { x: 1.0, y: 1.0 },
        UnitPoint { x: 0.0, y: 1.0 },
    ];
    assert!(
        session
            .draw_current(
                renderer,
                egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width as f32, height as f32)
                ),
                uv,
            )
            .expect("draw shared hardware frame")
    );
    let context = egui::Context::default();
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width as f32, height as f32),
            )),
            ..Default::default()
        },
        |ui| {
            ui.ctx()
                .layer_painter(egui::LayerId::background())
                .rect_filled(
                    egui::Rect::from_min_size(egui::pos2(4.0, 4.0), egui::vec2(8.0, 8.0)),
                    0.0,
                    egui::Color32::MAGENTA,
                );
        },
    );
    renderer
        .render_ui(&context, output)
        .expect("independent UI renderer");
    let result = pixels(renderer);
    let offset = (6 * width as usize + 6) * 4;
    assert_eq!(&result[offset..offset + 4], &[255, 0, 255, 255]);
    renderer.present_surface().expect("present");
    result
}

#[test]
#[ignore = "requires native hidden windows and hardware D3D11VA; generated silent video only"]
fn shared_window_surfaces_preserve_live_hardware_session() {
    struct Trial {
        path: std::path::PathBuf,
        completed: bool,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let windows: Vec<_> = (0..2)
                .map(|_| {
                    Arc::new(
                        event_loop
                            .create_window(
                                Window::default_attributes()
                                    .with_visible(false)
                                    .with_inner_size(winit::dpi::PhysicalSize::new(160, 96)),
                            )
                            .expect("owned hidden window"),
                    )
                })
                .collect();
            let mut first = match FrameRenderer::new(&windows[0]) {
                Ok(renderer) => renderer,
                Err(error) => {
                    eprintln!("SKIP shared window surfaces: hardware D3D11 unavailable: {error}");
                    event_loop.exit();
                    return;
                }
            };
            let device = first.graphics_device();
            let caption = NativeCaption::new(windows[1].clone()).expect("caption");
            let mut second = FrameRenderer::with_native_caption_on_device(&caption, device.clone())
                .expect("shared caption surface");
            assert!(first.graphics_device.device == second.graphics_device.device);
            assert!(first.context == second.context);
            assert!(first.swap_chain != second.swap_chain);
            assert_eq!(first.max_texture_side(), second.max_texture_side());
            let mut session = PlaybackSession::open(
                &self.path,
                device.clone(),
                0.0,
                1.0,
                PlaybackRange::default(),
                |_| {},
            )
            .expect("silent session");
            let deadline = Instant::now() + Duration::from_secs(10);
            while session.pending_video_time().is_none() {
                assert!(Instant::now() < deadline, "frame deadline");
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(session.advance_pending());
            if session.metrics().hardware_frame_count == 0 {
                eprintln!("SKIP shared window surfaces: decoder selected software, not D3D11VA");
                event_loop.exit();
                return;
            }
            session.set_paused(true).expect("pause silent session");
            let generation = session.generation();
            let time = session.current_video_time();
            for (width, height) in [(160, 96), (93, 61), (240, 144)] {
                first.resize_surface(width, height).expect("first resize");
                second.resize_surface(width, height).expect("second resize");
                let expected = draw(&mut first, &mut session);
                assert!(
                    expected
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|p| p[0] < 100 && p[1] > 160 && p[2] < 100),
                    "video contains green distinct from the UI marker"
                );
                for _ in 0..4 {
                    assert_eq!(draw(&mut second, &mut session), expected);
                    assert_eq!(draw(&mut first, &mut session), expected);
                }
            }
            let expected = draw(&mut second, &mut session);
            first.release_surface();
            assert_eq!(draw(&mut second, &mut session), expected);
            let mut first = FrameRenderer::with_graphics_device(&windows[0], device)
                .expect("replacement surface on surviving device");
            assert!(first.graphics_device.device == second.graphics_device.device);
            first.resize_surface(240, 144).expect("replacement resize");
            assert_eq!(draw(&mut first, &mut session), expected);
            second.release_surface();
            assert_eq!(draw(&mut first, &mut session), expected);
            assert_eq!(session.generation(), generation);
            assert_eq!(session.current_video_time(), time);
            assert_eq!(session.metrics().cpu_transfer_count, 0);
            assert_eq!(session.metrics().presented_frame_count, 1);
            session.set_paused(false).expect("resume silent session");
            let deadline = Instant::now() + Duration::from_secs(5);
            while session.pending_video_time().is_none() {
                assert!(Instant::now() < deadline, "next frame deadline");
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(session.advance_pending());
            assert!(session.current_video_time() > time);
            draw(&mut first, &mut session);
            assert_eq!(session.generation(), generation);
            assert_eq!(session.metrics().cpu_transfer_count, 0);
            assert_eq!(session.metrics().presented_frame_count, 2);
            drop(session);
            first.release_surface();
            self.completed = true;
            eprintln!(
                "PASS shared window surfaces: identical COM device/context, independent swap chains; 3 sizes, alternate video/UI drawing and surface release/recreation; stable frame/generation then next-frame advance, playback CPU transfers 0 (test-only readback excluded)"
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("towavue-shared-windows-{unique}.mp4"));
    let output =
        std::process::Command::new(crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg"))
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x96:rate=30:duration=1",
                "-c:v",
                "libopenh264",
                "-b:v",
                "250k",
                "-an",
            ])
            .arg(&path)
            .output()
            .expect("generate silent video");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut trial = Trial {
        path: path.clone(),
        completed: false,
    };
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut trial)
        .expect("native trial");
    std::fs::remove_file(path).expect("remove owned generated silent video");
    if !trial.completed {
        eprintln!("SKIP shared window surfaces: native hardware trial did not complete");
    }
}
