use std::error::Error;
use std::time::Instant;

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
    D3D11CreateDevice, ID3D11Multithread, ID3D11VideoContext, ID3D11VideoDevice,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::core::Interface;

fn main() -> Result<(), Box<dyn Error>> {
    if cfg!(debug_assertions) {
        return Err("use Release; run each sample in a fresh process".into());
    }
    let video_flag = match std::env::args().nth(1).as_deref() {
        Some("video") => true,
        Some("graphics") => false,
        _ => return Err("usage: device-start-cost video|graphics".into()),
    };
    let flags = if video_flag {
        D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT
    } else {
        D3D11_CREATE_DEVICE_BGRA_SUPPORT
    };
    let mut device = None;
    let mut context = None;
    let mut level = D3D_FEATURE_LEVEL::default();
    let started = Instant::now();
    // This diagnostic owns one windowless device and its context on the main thread.
    // No native handle leaves the process and no production creation policy is changed.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            flags,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut level),
            Some(&mut context),
        )?;
    }
    let creation = started.elapsed();
    let device = device.ok_or("missing device")?;
    let context = context.ok_or("missing context")?;
    let multithread: ID3D11Multithread = context.cast()?;
    // Keep the ordinary app's immediate-context serialization enabled.
    unsafe {
        let _ = multithread.SetMultithreadProtected(true);
    }
    let protected = started.elapsed();
    let video: ID3D11VideoDevice = device.cast()?;
    let _video_context: ID3D11VideoContext = context.cast()?;
    let dxgi: IDXGIDevice = device.cast()?;
    // All queried COM interfaces remain owned until after capability inspection.
    let (count, adapter, protected_context, actual_flags) = unsafe {
        (
            video.GetVideoDecoderProfileCount(),
            dxgi.GetAdapter()?.GetDesc()?,
            multithread.GetMultithreadProtected().as_bool(),
            device.GetCreationFlags(),
        )
    };
    let mut profile_hash = 0xcbf29ce484222325_u64;
    for index in 0..count {
        // The output GUID is caller-owned; profile indexes are bounded by the device count.
        let profile = unsafe { video.GetVideoDecoderProfile(index)? };
        for byte in format!("{profile:?}").bytes() {
            profile_hash = (profile_hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    if !protected_context || actual_flags != flags.0 {
        return Err("device creation/serialization flags differ".into());
    }
    println!(
        "DEVICE_START video_flag={video_flag} create_ms={:.4} protected_ms={:.4} checked_ms={:.4} feature={} luid={}:{} profiles={count} profile_hash={profile_hash:016x}",
        creation.as_secs_f64() * 1000.0,
        protected.as_secs_f64() * 1000.0,
        started.elapsed().as_secs_f64() * 1000.0,
        level.0,
        adapter.AdapterLuid.HighPart,
        adapter.AdapterLuid.LowPart,
    );
    Ok(())
}
