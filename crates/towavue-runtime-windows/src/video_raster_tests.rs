use super::*;

#[path = "video_resample_tests.rs"]
mod resample_tests;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use towavue_core::{EditOperation as Edit, PixelCrop, VideoRotation};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_USAGE_STAGING,
};

fn plan(size: (u32, u32), aspect: f32, edits: &[Edit]) -> Plan {
    Plan::new(
        size,
        aspect,
        crate::VideoOrientation::default(),
        edits,
        16384,
    )
    .expect("plan")
}

fn pixels(size: (u32, u32)) -> Vec<u8> {
    (0..size.1)
        .flat_map(|y| {
            (0..size.0).flat_map(move |x| {
                [
                    (x * 13 + y * 7 + 20) as u8,
                    (y * 17 + x * 3 + 30) as u8,
                    (x * 9 + y * 11 + 40) as u8,
                    255,
                ]
            })
        })
        .collect()
}

struct Gpu {
    graphics: GraphicsDevice,
    context: ID3D11DeviceContext,
    blitter: SoftwareBlitter,
    raster: VideoRaster,
}

impl Gpu {
    fn new() -> Self {
        let graphics = GraphicsDevice::warp_for_test().expect("offscreen WARP device");
        Self::with_graphics(graphics)
    }

    fn with_graphics(graphics: GraphicsDevice) -> Self {
        // This test owns its windowless device and uses its context on this thread only.
        let context = unsafe { graphics.device.GetImmediateContext() }.expect("context");
        let blitter = SoftwareBlitter::new(&graphics.device).expect("blitter");
        let raster = VideoRaster::new(&graphics.device).expect("raster shader compiles");
        Self {
            graphics,
            context,
            blitter,
            raster,
        }
    }

    fn upload(&self, size: (u32, u32), rgba: &[u8]) -> ID3D11Texture2D {
        assert_eq!(rgba.len(), (size.0 * size.1 * 4) as usize);
        let mut texture = None;
        // The descriptor is borrowed synchronously and upload copies the complete
        // tightly packed slice before return. Only the test owns this texture.
        unsafe {
            self.graphics
                .device
                .CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        Width: size.0,
                        Height: size.1,
                        MipLevels: 1,
                        ArraySize: 1,
                        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                        SampleDesc: DXGI_SAMPLE_DESC {
                            Count: 1,
                            Quality: 0,
                        },
                        Usage: D3D11_USAGE_DEFAULT,
                        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                        ..Default::default()
                    },
                    None,
                    Some(&mut texture),
                )
                .expect("source texture");
            let texture = texture.as_ref().expect("texture");
            self.context
                .UpdateSubresource(texture, 0, None, rgba.as_ptr().cast(), size.0 * 4, 0);
        }
        texture.expect("texture")
    }

    fn read(&self, texture: &ID3D11Texture2D, size: (u32, u32)) -> Vec<u8> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        let mut staging = None;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let mut rgba = Vec::with_capacity((size.0 * size.1 * 4) as usize);
        // Readback exists only in tests. Copy and Map use this test's serialized
        // context; each row slice stays inside the mapped staging allocation, is
        // copied into owned bytes, and is dropped before the matching Unmap.
        unsafe {
            texture.GetDesc(&mut desc);
            assert_eq!((desc.Width, desc.Height), size);
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            self.graphics
                .device
                .CreateTexture2D(&desc, None, Some(&mut staging))
                .expect("staging");
            let staging = staging.expect("staging texture");
            self.context.CopyResource(&staging, texture);
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .expect("map");
            for row in 0..size.1 as usize {
                rgba.extend_from_slice(std::slice::from_raw_parts(
                    mapped
                        .pData
                        .cast::<u8>()
                        .add(row * mapped.RowPitch as usize),
                    size.0 as usize * 4,
                ));
            }
            self.context.Unmap(&staging, 0);
        }
        rgba
    }

    fn draw(&mut self, source: &ID3D11Texture2D, plan: &Plan) -> Vec<u8> {
        let texture = self
            .raster
            .draw(
                &self.graphics.device,
                &self.context,
                &self.blitter,
                source,
                plan,
            )
            .expect("raster draw");
        self.read(&texture, plan.size)
    }
}

fn reference(size: (u32, u32), rgba: &[u8], filter: &str) -> Vec<u8> {
    let mut child =
        Command::new(crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg"))
            .creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW.0)
            .args([
                "-v",
                "error",
                "-f",
                "rawvideo",
                "-pixel_format",
                "rgba",
                "-video_size",
                &format!("{}x{}", size.0, size.1),
                "-i",
                "pipe:0",
                "-vf",
                filter,
                "-frames:v",
                "1",
                "-pix_fmt",
                "rgba",
                "-f",
                "rawvideo",
                "pipe:1",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("reference process");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(rgba)
        .expect("reference input");
    let result = child.wait_with_output().expect("reference output");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    result.stdout
}

#[test]
fn raster_plan_validates_context_and_budgets_exact_size_reusable_slots() {
    let flips = vec![Edit::FlipHorizontal; 1000];
    let p = plan((16384, 16), 1.0, &flips);
    assert_eq!(
        p.slots.iter().map(|slot| slot.size).collect::<Vec<_>>(),
        [(16384, 16), (16384, 16)]
    );
    assert!(p.stages.windows(2).all(|pair| pair[0].slot != pair[1].slot));
    let p = plan(
        (16384, 16),
        2.0,
        &[
            Edit::RotateClockwise,
            Edit::RotateCounterclockwise,
            Edit::RotateClockwise,
        ],
    );
    assert_eq!(
        p.slots.iter().map(|slot| slot.size).collect::<Vec<_>>(),
        [(16, 16384), (16384, 16)]
    );
    assert_eq!((p.size, p.pixel_aspect), ((16, 16384), 0.5));
    let p = plan((8192, 8192), 1.0, &flips);
    assert_eq!(p.slots.len(), 2); // Exactly 512 MiB; no allocation in this test.
    let excessive = [
        Edit::FlipHorizontal,
        Edit::FlipVertical,
        Edit::Crop(PixelCrop {
            x: 0,
            y: 0,
            width: 8190,
            height: 8190,
        }),
    ];
    assert!(matches!(
        Plan::new(
            (8192, 8192),
            1.0,
            crate::VideoOrientation::default(),
            &excessive,
            16384
        ),
        Err(RenderError::VideoEditBudget)
    ));
    for edits in [
        vec![Edit::RotateVideo(
            VideoRotation::new(317, (8, 6), 2.0).expect("rotation"),
        )],
        vec![Edit::RotateVideo(
            VideoRotation::new(317, (6, 8), 1.0).expect("rotation"),
        )],
        vec![Edit::Crop(PixelCrop {
            x: u32::MAX,
            y: 0,
            width: 2,
            height: 2,
        })],
        vec![Edit::RotateImage(
            towavue_core::ImageRotation::new(317, (8, 6)).expect("image"),
        )],
    ] {
        assert!(matches!(
            Plan::new(
                (8, 6),
                1.0,
                crate::VideoOrientation::default(),
                &edits,
                16384
            ),
            Err(RenderError::InvalidVideoEdit)
        ));
    }
    for aspect in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(
            Plan::new(
                (8, 6),
                aspect,
                crate::VideoOrientation::default(),
                &[],
                16384
            )
            .is_err()
        );
    }
    assert!(matches!(
        Plan::new((8, 6), 1.0, crate::VideoOrientation::default(), &[], 4),
        Err(RenderError::VideoEditBudget)
    ));
    let p = plan(
        (7, 5),
        2.0,
        &[Edit::RotateVideo(
            VideoRotation::new(0, (7, 5), 2.0).expect("zero"),
        )],
    );
    assert!(p.stages.is_empty());
    assert_eq!((p.size, p.pixel_aspect), ((7, 5), 2.0));
}

#[test]
fn warp_ordered_integer_edits_match_ffmpeg_and_reuse_without_stale_pixels() {
    let mut gpu = Gpu::new();
    let size = (7, 5);
    let rgba = pixels(size);
    let source = gpu.upload(size, &rgba);
    let edits = [
        Edit::RotateClockwise,
        Edit::Crop(PixelCrop {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        }),
        Edit::FlipHorizontal,
        Edit::RotateCounterclockwise,
        Edit::FlipVertical,
    ];
    let p = plan(size, 2.0, &edits);
    // A preceding UI pass may leave an empty scissor enabled on this same context.
    let mut scissor = None;
    unsafe {
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_CULL_NONE, D3D11_FILL_SOLID, D3D11_RASTERIZER_DESC,
        };
        gpu.graphics
            .device
            .CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    ScissorEnable: true.into(),
                    DepthClipEnable: true.into(),
                    ..Default::default()
                },
                Some(&mut scissor),
            )
            .expect("UI scissor state");
        gpu.context.RSSetState(scissor.as_ref());
        gpu.context.RSSetScissorRects(Some(&[RECT::default()]));
    }
    let actual = gpu.draw(&source, &p);
    assert_eq!(
        actual,
        reference(
            size,
            &rgba,
            "transpose=clock,crop=3:4:1:2:exact=1,hflip,transpose=cclock,vflip"
        )
    );
    assert_eq!(gpu.read(&source, size), rgba);
    let ids: Vec<_> = gpu
        .raster
        .surfaces
        .iter()
        .map(|surface| surface.texture.as_raw())
        .collect();
    let black = [0, 0, 0, 255].repeat((size.0 * size.1) as usize);
    let source = gpu.upload(size, &black);
    assert_eq!(
        gpu.draw(&source, &p),
        [0, 0, 0, 255].repeat((p.size.0 * p.size.1) as usize)
    );
    assert_eq!(
        ids,
        gpu.raster
            .surfaces
            .iter()
            .map(|surface| surface.texture.as_raw())
            .collect::<Vec<_>>()
    );
    let empty = plan(size, 1.0, &[]);
    assert_eq!(gpu.draw(&source, &empty), black);
    assert!(gpu.raster.surfaces.is_empty());
}

#[test]
fn warp_source_orientation_precedes_crop_aspect_normalization_and_rotation() {
    let mut gpu = Gpu::new();
    let size = (8, 6);
    let rgba = pixels(size);
    let source = gpu.upload(size, &rgba);
    for (linear, filter) in [
        ([1, 0, 0, 1], "null"),
        ([0, -1, 1, 0], "transpose=cclock"),
        ([-1, 0, 0, -1], "hflip,vflip"),
        ([0, 1, -1, 0], "transpose=clock"),
        ([-1, 0, 0, 1], "hflip"),
        ([1, 0, 0, -1], "vflip"),
        ([0, 1, 1, 0], "transpose=clock,hflip"),
        ([0, -1, -1, 0], "transpose=clock,vflip"),
    ] {
        let [a, b, c, d] = linear.map(|value| value * 65536);
        let matrix = [a, b, 0, c, d, 0, 0, 0, 1 << 30];
        let bytes: Vec<_> = matrix.into_iter().flat_map(i32::to_ne_bytes).collect();
        let orientation = crate::VideoOrientation::from_bytes(Some(&bytes)).expect("orientation");
        let (oriented, aspect) = if orientation.swaps_axes() {
            ((6, 8), 0.5)
        } else {
            (size, 2.0)
        };
        let crop = PixelCrop {
            x: 1,
            y: 1,
            width: oriented.0 - 2,
            height: oriented.1 - 2,
        };
        let rotation =
            VideoRotation::new(317, (crop.width, crop.height), aspect).expect("rotation");
        let p = Plan::new(
            size,
            2.0,
            orientation,
            &[Edit::Crop(crop), Edit::RotateVideo(rotation)],
            16384,
        )
        .expect("oriented plan");
        let actual = gpu.draw(&source, &p);
        let reference_filter = format!(
            "{filter},crop={}:{}:1:1:exact=1,format=gbrp,scale={}:{}:flags=bilinear,format=gbrp,rotate=317*PI/1800:ow={}:oh={}:c=black:bilinear=1,pad={}:{}:0:0:color=black",
            crop.width,
            crop.height,
            rotation.square_size().0,
            rotation.square_size().1,
            rotation.raster_size().0,
            rotation.raster_size().1,
            rotation.size().0,
            rotation.size().1
        );
        let expected = reference(size, &rgba, &reference_filter);
        assert_eq!(actual.len(), expected.len());
        let max_error = actual
            .iter()
            .zip(&expected)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .expect("pixels");
        assert!(
            max_error <= 3,
            "{filter}: maximum channel error {max_error}"
        );
        assert_eq!((p.size, p.pixel_aspect), (rotation.size(), 1.0));
    }
}

#[test]
fn warp_video_rotation_matches_export_geometry_black_canvas_and_composed_order() {
    let mut gpu = Gpu::new();
    for (size, aspect, angle) in [
        ((7, 5), 1.0, 317),
        ((7, 5), 1.0, -127),
        ((8, 6), 2.0, 317),
        ((8, 6), 0.5, -127),
        ((7, 5), 1.5, 317),
        ((7, 5), 0.75, -127),
        ((7, 5), 2.0, 317),
        ((7, 5), 0.5, -127),
        ((7, 5), 1.0, 900),
        ((7, 5), 1.0, -900),
        ((7, 5), 1.0, 1800),
        ((7, 5), 1.0, -1800),
        ((7, 5), 1.0, 1),
        ((7, 5), 1.0, -1),
        ((7, 5), 1.0, 899),
        ((7, 5), 1.0, -899),
        ((7, 5), 1.0, 901),
        ((7, 5), 1.0, -901),
        ((7, 5), 1.0, 1799),
        ((7, 5), 1.0, -1799),
    ] {
        let rgba = pixels(size);
        let source = gpu.upload(size, &rgba);
        let rotation = VideoRotation::new(angle, size, aspect).expect("rotation");
        let p = plan(size, aspect, &[Edit::RotateVideo(rotation)]);
        let actual = gpu.draw(&source, &p);
        let (sw, sh) = rotation.square_size();
        let (rw, rh) = rotation.raster_size();
        let (ow, oh) = rotation.size();
        let rotate = match angle {
            900 => "transpose=clock".to_owned(),
            -900 => "transpose=cclock".to_owned(),
            -1800 | 1800 => "hflip,vflip".to_owned(),
            _ => format!("rotate={angle}*PI/1800:ow={rw}:oh={rh}:c=black:bilinear=1"),
        };
        let expected = reference(
            size,
            &rgba,
            &format!(
                "format=gbrp,scale={sw}:{sh}:flags=bilinear,format=gbrp,{rotate},pad={ow}:{oh}:0:0:color=black"
            ),
        );
        assert_eq!(actual.len(), expected.len());
        let max_error = actual
            .iter()
            .zip(&expected)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .expect("pixels");
        assert!(
            max_error <= 3,
            "{size:?} SAR{aspect} angle{angle}: max channel error {max_error}"
        );
        for (index, (a, b)) in actual
            .as_chunks::<4>()
            .0
            .iter()
            .zip(expected.as_chunks::<4>().0)
            .enumerate()
        {
            assert_eq!(
                *a == [0, 0, 0, 255],
                *b == [0, 0, 0, 255],
                "black canvas at {index}, angle{angle}"
            );
        }
        assert_eq!(gpu.read(&source, size), rgba);
    }
    let size = (12, 8);
    let rgba = pixels(size);
    let source = gpu.upload(size, &rgba);
    let first = VideoRotation::new(317, size, 1.0).expect("first");
    let second = VideoRotation::new(-127, (8, 6), 1.0).expect("second");
    let edits = [
        Edit::RotateVideo(first),
        Edit::Crop(PixelCrop {
            x: 2,
            y: 2,
            width: 8,
            height: 6,
        }),
        Edit::FlipHorizontal,
        Edit::RotateVideo(second),
        Edit::RotateClockwise,
    ];
    let p = plan(size, 1.0, &edits);
    let filter = format!(
        "format=gbrp,rotate=317*PI/1800:ow={}:oh={}:c=black:bilinear=1,pad={}:{}:0:0:color=black,crop=8:6:2:2:exact=1,hflip,rotate=-127*PI/1800:ow={}:oh={}:c=black:bilinear=1,pad={}:{}:0:0:color=black,transpose=clock",
        first.raster_size().0,
        first.raster_size().1,
        first.size().0,
        first.size().1,
        second.raster_size().0,
        second.raster_size().1,
        second.size().0,
        second.size().1
    );
    let actual = gpu.draw(&source, &p);
    let expected = reference(size, &rgba, &filter);
    assert_eq!(actual.len(), expected.len());
    let max_error = actual
        .iter()
        .zip(&expected)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .expect("pixels");
    assert!(max_error <= 3, "composed max channel error {max_error}");
}
