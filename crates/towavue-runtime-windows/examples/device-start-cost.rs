use std::error::Error;
use std::time::Instant;

use windows::Win32::Foundation::{FILETIME, HMODULE};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
    D3D11CreateDevice, ID3D11Multithread, ID3D11VideoContext, ID3D11VideoDevice,
};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIDevice, IDXGIFactory1};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, GetProcessTimes, GetThreadTimes,
};
use windows::core::Interface;

// Kernel/user CPU accounting only for this process or the calling thread.
// Granularity and driver worker overlap prevent treating wall-minus-CPU as wait time.
fn cpu_times(process: bool) -> windows::core::Result<[u64; 2]> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // Pseudo handles are borrowed, never closed; outputs live through the call.
    unsafe {
        if process {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )?;
        } else {
            GetThreadTimes(
                GetCurrentThread(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )?;
        }
    }
    Ok([kernel, user]
        .map(|time| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)))
}

fn main() -> Result<(), Box<dyn Error>> {
    if cfg!(debug_assertions) {
        return Err("use Release; run each sample in a fresh process".into());
    }
    let (video_flag, explicit_adapter) = match std::env::args().nth(1).as_deref() {
        Some("video") => (true, false),
        Some("graphics") => (false, false),
        Some("adapter") => (true, true),
        _ => return Err("usage: device-start-cost video|graphics|adapter".into()),
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
    // Isolate default-adapter discovery without changing the selected adapter or
    // creating a second device. Include discovery in the total, not just the API call.
    let factory: Option<IDXGIFactory1> = if explicit_adapter {
        Some(unsafe { CreateDXGIFactory1()? })
    } else {
        None
    };
    let factory_elapsed = started.elapsed();
    let selected = factory
        .as_ref()
        .map(|factory| unsafe { factory.EnumAdapters(0) })
        .transpose()?;
    let discovery = started.elapsed();
    let process_before = cpu_times(true)?;
    let thread_before = cpu_times(false)?;
    let device_started = Instant::now();
    // This diagnostic owns one windowless device and its context on the main thread.
    // No native handle leaves the process and no production creation policy is changed.
    unsafe {
        D3D11CreateDevice(
            selected.as_ref(),
            if explicit_adapter {
                D3D_DRIVER_TYPE_UNKNOWN
            } else {
                D3D_DRIVER_TYPE_HARDWARE
            },
            HMODULE::default(),
            flags,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut level),
            Some(&mut context),
        )?;
    }
    let device_elapsed = device_started.elapsed();
    let thread_after = cpu_times(false)?;
    let process_after = cpu_times(true)?;
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
    if let Some(selected) = selected {
        // Compare identities while both owned adapter references remain live.
        let selected = unsafe { selected.GetDesc()? };
        if selected.AdapterLuid != adapter.AdapterLuid {
            return Err("created device changed the selected adapter".into());
        }
    }
    println!(
        "DEVICE_START video_flag={video_flag} create_ms={:.4} protected_ms={:.4} checked_ms={:.4} feature={} luid={}:{} profiles={count} profile_hash={profile_hash:016x} explicit_adapter={explicit_adapter} factory_ms={:.4} enumerate_ms={:.4} device_ms={:.4}",
        creation.as_secs_f64() * 1000.0,
        protected.as_secs_f64() * 1000.0,
        started.elapsed().as_secs_f64() * 1000.0,
        level.0,
        adapter.AdapterLuid.HighPart,
        adapter.AdapterLuid.LowPart,
        factory_elapsed.as_secs_f64() * 1000.0,
        (discovery - factory_elapsed).as_secs_f64() * 1000.0,
        device_elapsed.as_secs_f64() * 1000.0,
    );
    println!(
        "DEVICE_CPU process_kernel_ms={:.4} process_user_ms={:.4} thread_kernel_ms={:.4} thread_user_ms={:.4}",
        (process_after[0] - process_before[0]) as f64 / 10_000.0,
        (process_after[1] - process_before[1]) as f64 / 10_000.0,
        (thread_after[0] - thread_before[0]) as f64 / 10_000.0,
        (thread_after[1] - thread_before[1]) as f64 / 10_000.0,
    );
    Ok(())
}

#[test]
fn cpu_accounting_advances_for_owned_thread_work() {
    let process_before = cpu_times(true).expect("process times");
    let thread_before = cpu_times(false).expect("thread times");
    let started = Instant::now();
    let mut value = 1_u64;
    while started.elapsed() < std::time::Duration::from_millis(150) {
        value = std::hint::black_box(value.wrapping_mul(6364136223846793005).wrapping_add(1));
    }
    for (process, before) in [(false, thread_before), (true, process_before)] {
        let after = cpu_times(process).expect("updated CPU times");
        assert!(after[0] >= before[0] && after[1] >= before[1]);
        assert!(after[0] + after[1] > before[0] + before[1]);
    }
}
