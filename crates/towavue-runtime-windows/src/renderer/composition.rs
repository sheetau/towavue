//! An opaque, pixel-sized compositor visual on the existing shared D3D11 device.
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGISwapChain1};

/// Owned by FrameRenderer before its swap chain and CaptionSurface fields.
/// All calls and destruction stay on the HWND's event-loop thread. The visual
/// retains the swap chain; it applies no transform or clip, so the buffer keeps
/// physical-pixel coordinates during resize and includes the caption controls.
pub(super) struct Surface {
    target: IDCompositionTarget,
    visual: IDCompositionVisual,
    device: IDCompositionDevice,
}

impl Surface {
    pub(super) fn new(
        device: &IDXGIDevice,
        hwnd: HWND,
        chain: &IDXGISwapChain1,
    ) -> windows::core::Result<Self> {
        // SAFETY: the caller retains this same-thread child HWND and the shared
        // D3D11 device. COM references own every graph object; no borrowed pointer
        // survives a call. This child has no GDI presentation or other target.
        unsafe {
            let device: IDCompositionDevice = DCompositionCreateDevice(device)?;
            let surface = Self {
                target: device.CreateTargetForHwnd(hwnd, false)?,
                visual: device.CreateVisual()?,
                device,
            };
            surface.visual.SetContent(chain)?;
            surface.target.SetRoot(&surface.visual)?;
            surface.device.Commit()?;
            // Establish the tree before a staged GPU child can replace the CPU
            // startup surface. This one-time setup wait is never in Present or
            // the move/resize frame path; it does not acknowledge displayed pixels.
            surface.device.WaitForCommitCompletion()?;
            Ok(surface)
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: FrameRenderer still owns the child and swap chain. Detach their
        // compositor references before releasing either, including partial setup
        // and device-loss paths. Teardown is best effort on a removed device;
        // releasing the owned graph/target and destroying the child still retire it.
        unsafe {
            let _ = self.target.SetRoot(None::<&IDCompositionVisual>);
            let _ = self.visual.SetContent(None::<&windows::core::IUnknown>);
            let _ = self.device.Commit();
        }
    }
}
