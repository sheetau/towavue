use super::*;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL,
    DXGI_QUERY_VIDEO_MEMORY_INFO, IDXGIAdapter3,
};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::GetCurrentProcess;

/// Calling-thread kernel + user CPU accounting, not wall time or GPU execution time.
/// OS accounting granularity means wall-minus-CPU is not an exact wait duration.
pub fn verification_thread_cpu_time() -> Result<std::time::Duration, RenderError> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // The pseudo handle is borrowed, never closed. All four outputs remain valid
    // for this synchronous call; creation/exit timestamps are deliberately ignored.
    unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )?;
    }
    let duration = |time: FILETIME| {
        let ticks = (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
        std::time::Duration::new(ticks / 10_000_000, ((ticks % 10_000_000) * 100) as u32)
    };
    Ok(duration(kernel) + duration(user))
}

#[test]
fn thread_cpu_accounting_advances_with_calling_thread_work() {
    use std::time::{Duration, Instant};
    let before = verification_thread_cpu_time().expect("thread CPU accounting");
    let started = Instant::now();
    let mut value = 1_u64;
    while started.elapsed() < Duration::from_millis(250) {
        value = std::hint::black_box(value.wrapping_mul(6364136223846793005).wrapping_add(1));
    }
    let after = verification_thread_cpu_time().expect("thread CPU accounting");
    assert!(after > before);
    std::thread::sleep(Duration::from_millis(50));
    assert!(verification_thread_cpu_time().expect("thread CPU accounting") >= after);
}

/// Process-lifetime OS high-water marks and current node-zero GPU usage, in bytes.
pub struct VerificationMemory {
    pub working_set: u64,
    pub peak_working_set: u64,
    pub private_bytes: u64,
    pub peak_commit: u64,
    pub gpu_local_nonlocal: Result<[u64; 2], String>,
}

struct MappedSurface<'a> {
    context: &'a ID3D11DeviceContext,
    texture: ID3D11Texture2D,
}

impl Drop for MappedSurface<'_> {
    fn drop(&mut self) {
        // This guard is created only after a successful Map and owns its resource.
        unsafe { self.context.Unmap(&self.texture, 0) };
    }
}

impl FrameRenderer {
    /// Opt into pre-Present pixel checks for explicit black transition frames.
    /// Readback is disabled until requested and absent from normal builds.
    pub fn verification_track_transition_background(&mut self) {
        self.transition_background_verification = Some((0, true, std::time::Duration::ZERO));
    }

    /// Completed submissions, whether all checked pixels were opaque black, and
    /// the last Present/DwmFlush duration (excludes the opt-in GPU readback).
    /// These are not captured compositor frames or an uninstrumented benchmark.
    pub fn verification_transition_background(&self) -> Option<(u32, bool, std::time::Duration)> {
        self.transition_background_verification
    }

    /// Calling-thread totals: native texture/SRV creation, source release, pool update.
    /// Creation/release are included in pool update; these are CPU wall times.
    pub fn verification_upload_times(&self) -> [std::time::Duration; 3] {
        self.ui_renderer.verification_upload_times()
    }

    /// Inventory of renderer-owned managed textures, not driver allocations or user/video views.
    pub fn verification_managed_textures(&self) -> Vec<(egui::TextureId, [usize; 2])> {
        self.ui_renderer.verification_managed_textures()
    }

    /// Diagnostic only: retire bindings on an isolated, idle test device, optionally trim its caches.
    pub fn verification_retire_resources(&mut self, trim: bool) -> Result<(), RenderError> {
        // The test owns this multithread-protected immediate context and performs no concurrent
        // media work. No mapped memory is live; subsequent draws rebind their complete state.
        unsafe {
            self.context.ClearState();
            self.context.Flush();
        }
        if trim {
            let device: windows::Win32::Graphics::Dxgi::IDXGIDevice3 =
                self.graphics_device.device.cast()?;
            // The owned device reference outlives Trim; only its internal caches are discarded.
            unsafe {
                device.Trim();
            }
        }
        Ok(())
    }

    pub fn verification_memory(&self) -> Result<VerificationMemory, RenderError> {
        let mut counters = PROCESS_MEMORY_COUNTERS_EX {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        // The pseudo handle is borrowed, never closed; the sized output lives through the call.
        unsafe {
            GetProcessMemoryInfo(
                GetCurrentProcess(),
                (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
                counters.cb,
            )?;
        }
        let gpu = || -> windows::core::Result<[u64; 2]> {
            let dxgi: IDXGIDevice = self.graphics_device.device.cast()?;
            // The owned device/adapter references outlive both writes to local output structs.
            let adapter: IDXGIAdapter3 = unsafe { dxgi.GetAdapter()? }.cast()?;
            let mut usage = [0; 2];
            for (index, segment) in [
                DXGI_MEMORY_SEGMENT_GROUP_LOCAL,
                DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL,
            ]
            .into_iter()
            .enumerate()
            {
                let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                unsafe {
                    adapter.QueryVideoMemoryInfo(0, segment, &mut info)?;
                }
                usage[index] = info.CurrentUsage;
            }
            Ok(usage)
        };
        Ok(VerificationMemory {
            working_set: counters.WorkingSetSize as u64,
            peak_working_set: counters.PeakWorkingSetSize as u64,
            private_bytes: counters.PrivateUsage as u64,
            peak_commit: counters.PeakPagefileUsage as u64,
            gpu_local_nonlocal: gpu().map_err(|error| error.to_string()),
        })
    }

    /// Verification-only GPU readback, in tightly packed RGBA order, before Present.
    /// Call on the rendering thread after sizing and drawing this surface.
    pub fn verification_surface_rgba(&mut self) -> Result<Vec<u8>, RenderError> {
        self.ensure_surface()?;
        // The exclusive renderer borrow keeps its swap chain stable during copying.
        // Source/staging COM references stay owned here; Map synchronizes GPU work.
        // The guard unmaps on normal return or unwind; no borrowed pixels escape.
        unsafe {
            let source: ID3D11Texture2D = self.swap_chain.GetBuffer(0)?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            source.GetDesc(&mut desc);
            assert_eq!(desc.Format, DXGI_FORMAT_R8G8B8A8_UNORM);
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            desc.MiscFlags = 0;
            let mut staging = None;
            self.graphics_device
                .device
                .CreateTexture2D(&desc, None, Some(&mut staging))?;
            let staging = staging.expect("D3D11 returned success without a staging texture");
            self.context.CopyResource(&staging, &source);
            let row_bytes = desc.Width as usize * 4;
            let mut result = vec![0; row_bytes * desc.Height as usize];
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            let _mapping = MappedSurface {
                context: &self.context,
                texture: staging,
            };
            assert!(row_bytes <= mapped.RowPitch as usize);
            for (row, output) in result.chunks_exact_mut(row_bytes).enumerate() {
                output.copy_from_slice(std::slice::from_raw_parts(
                    mapped
                        .pData
                        .cast::<u8>()
                        .add(row * mapped.RowPitch as usize),
                    row_bytes,
                ));
            }
            Ok(result)
        }
    }
}
