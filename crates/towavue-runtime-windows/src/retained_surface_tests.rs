use super::*;
use std::time::{Duration, Instant};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_FORMAT_P010};
use windows::core::Interface;

fn descriptor(frame: &HardwareVideoFrame) -> D3D11_TEXTURE2D_DESC {
    let (raw, _) = frame.texture_and_slice().expect("hardware texture");
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // The borrowed frame owns the COM texture throughout this descriptor-only query.
    unsafe {
        ID3D11Texture2D::from_raw_borrowed(&raw)
            .expect("texture")
            .GetDesc(&mut desc);
    }
    desc
}

fn pixels(frame: &HardwareVideoFrame) -> Vec<u8> {
    let (raw, slice) = frame.texture_and_slice().expect("texture");
    let mut desc = descriptor(frame);
    let bytes_per_sample = match desc.Format {
        DXGI_FORMAT_NV12 => 1,
        DXGI_FORMAT_P010 => 2,
        _ => panic!("expected planar 4:2:0 texture"),
    };
    let mut output = Vec::new();
    // Test-only readback: the frame owns the borrowed texture, the staging allocation
    // belongs to this call, and row slices exclude pitch padding and die before Unmap.
    // The immediate context uses the production multithread serialization contract.
    unsafe {
        let source = ID3D11Texture2D::from_raw_borrowed(&raw).expect("texture");
        let device = source.GetDevice().expect("device");
        let context = device.GetImmediateContext().expect("context");
        desc.ArraySize = 1;
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        desc.MiscFlags = 0;
        let mut staging = None;
        device
            .CreateTexture2D(&desc, None, Some(&mut staging))
            .expect("staging");
        let staging = staging.expect("staging texture");
        context.CopySubresourceRegion(&staging, 0, 0, 0, 0, source, slice, None);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .expect("readback");
        let row_bytes = desc.Width as usize * bytes_per_sample;
        assert!(row_bytes <= mapped.RowPitch as usize);
        for row in 0..(desc.Height * 3 / 2) as usize {
            output.extend_from_slice(std::slice::from_raw_parts(
                mapped
                    .pData
                    .cast::<u8>()
                    .add(row * mapped.RowPitch as usize),
                row_bytes,
            ));
        }
        context.Unmap(&staging, 0);
    }
    output
}

#[test]
#[ignore = "requires a hardware D3D11VA device; only generated video without audio is decoded"]
fn retained_surface_hardware_pool_size_and_return_are_measured() {
    for name in ["h264-aac.mp4", "generated-1080p"] {
        measure_retained_surface(name);
    }
}

#[test]
#[ignore = "requires hardware VP9 Profile 2 D3D11VA; generated video has no audio"]
fn retained_surface_hardware_p010_copy_preserves_pq_and_aspect() {
    measure_retained_surface("generated-p010");
}

fn measure_retained_surface(name: &str) {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("towavue-retained-surface-{unique}.mp4"));
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/generated/m1")
        .join(name);
    let mut generate =
        std::process::Command::new(crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg"));
    generate.args(["-v", "error"]);
    if name == "generated-p010" {
        generate.args([
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=160x96:rate=30:duration=1",
            "-vf",
            "setsar=4/3,format=yuv420p10le",
            "-c:v",
            "libvpx-vp9",
            "-deadline",
            "realtime",
            "-cpu-used",
            "8",
            "-b:v",
            "250k",
            "-color_trc",
            "smpte2084",
            "-color_primaries",
            "bt2020",
            "-colorspace",
            "bt2020nc",
            "-an",
        ]);
    } else if name == "generated-1080p" {
        generate.args([
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=1920x1080:rate=30:duration=1",
            "-c:v",
            "libopenh264",
            "-b:v",
            "4M",
            "-an",
        ]);
    } else {
        generate
            .arg("-i")
            .arg(fixture)
            .args(["-t", "3", "-map", "0:v:0", "-c", "copy", "-an"]);
    }
    assert!(generate.arg(&path).status().expect("fixture").success());
    let device = match GraphicsDevice::hardware_for_test() {
        Ok(device) => device,
        Err(error) => {
            eprintln!("SKIP retained surface {name}: hardware D3D11 device unavailable: {error}");
            std::fs::remove_file(path).expect("remove generated video");
            return;
        }
    };
    let mut session =
        PlaybackSession::open(&path, device, 0.0, 1.0, PlaybackRange::default(), |_| {})
            .expect("session");
    let deadline = Instant::now() + Duration::from_secs(5);
    while session.pending_video_time().is_none() {
        assert!(Instant::now() < deadline, "hardware frame deadline");
        thread::sleep(Duration::from_millis(2));
    }
    assert!(session.advance_pending());
    let Some(PresentationFrame::Hardware(frame)) = &session.current_video else {
        eprintln!("SKIP retained surface {name}: D3D11VA unavailable; decoder selected software");
        drop(session);
        std::fs::remove_file(path).expect("remove generated video");
        return;
    };
    let before = descriptor(frame);
    if name == "generated-p010" {
        assert_eq!(before.Format, DXGI_FORMAT_P010);
        assert_eq!(frame.transfer, crate::decode::VideoTransfer::Pq);
        assert_eq!(frame.pixel_aspect, 4.0 / 3.0);
    }
    let time = frame.presentation_time;
    let metadata = (
        frame.width,
        frame.height,
        frame.pixel_aspect,
        frame.orientation,
        frame.transfer,
    );
    let original = frame.texture_and_slice();
    let expected = pixels(frame);
    // A rejected copy must leave the original frame and its pool reference intact.
    let other = GraphicsDevice::hardware_for_test().expect("different device");
    let Some(PresentationFrame::Hardware(frame)) = &mut session.current_video else {
        unreachable!()
    };
    assert!(matches!(
        frame.retain_surface(&other),
        Err(RenderError::InvalidHardwareFrame)
    ));
    assert_eq!(frame.texture_and_slice(), original);
    drop(other);
    session.set_paused(true).expect("pause");
    let start = Instant::now();
    session.set_video_visible(false, time).expect("hide");
    let hide = start.elapsed();
    let Some(PresentationFrame::Hardware(frame)) = &session.current_video else {
        panic!("retained hardware frame");
    };
    let after = descriptor(frame);
    assert_eq!(after.ArraySize, 1);
    assert_eq!(
        (after.Width, after.Height, after.Format),
        (before.Width, before.Height, before.Format)
    );
    assert_eq!(frame.presentation_time, time);
    assert_eq!(
        (
            frame.width,
            frame.height,
            frame.pixel_aspect,
            frame.orientation,
            frame.transfer
        ),
        metadata
    );
    assert_eq!(pixels(frame), expected);
    let retained = frame.texture_and_slice();
    assert_ne!(retained, original);
    eprintln!(
        "SURFACE measurement: {}x{}, format {}, array {} -> {}, hide {:?}",
        before.Width, before.Height, before.Format.0, before.ArraySize, after.ArraySize, hide
    );
    assert!(
        session.video_thread.is_none()
            && session.video_rx.is_none()
            && session.video_input.is_some()
    );
    assert_eq!(session.metrics().cpu_transfer_count, 0);
    let Some(PresentationFrame::Hardware(frame)) = &mut session.current_video else {
        unreachable!()
    };
    frame
        .retain_surface(&session.graphics_device)
        .expect("already retained");
    assert_eq!(frame.texture_and_slice(), retained);
    let start = Instant::now();
    session.set_video_visible(true, time).expect("show");
    let immediate = start.elapsed();
    let deadline = Instant::now() + Duration::from_secs(5);
    while session.pending_video_time().is_none() {
        assert!(Instant::now() < deadline, "return frame deadline");
        thread::sleep(Duration::from_millis(2));
    }
    eprintln!(
        "SURFACE return: immediate {:?}, fresh {:?}",
        immediate,
        start.elapsed()
    );
    assert_eq!(session.pending_video_time(), Some(time));
    assert_eq!(session.current_video_time(), Some(time));
    assert!(session.video_refresh_pending());
    assert!(session.advance_pending());
    assert!(!session.video_refresh_pending());
    for _ in 0..5 {
        session.set_video_visible(false, time).expect("repeat hide");
        let Some(PresentationFrame::Hardware(frame)) = &session.current_video else {
            unreachable!()
        };
        assert_eq!(descriptor(frame).ArraySize, 1);
        assert_eq!(pixels(frame), expected);
        session.set_video_visible(true, time).expect("repeat show");
        let deadline = Instant::now() + Duration::from_secs(5);
        while session.pending_video_time().is_none() {
            assert!(Instant::now() < deadline, "repeat deadline");
            thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(session.pending_video_time(), Some(time));
        assert!(session.advance_pending());
    }
    assert_eq!(session.metrics().cpu_transfer_count, 0);
    drop(session);
    std::fs::remove_file(path).expect("remove generated video");
}
