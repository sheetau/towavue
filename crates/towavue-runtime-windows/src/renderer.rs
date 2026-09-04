use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use thiserror::Error;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDeviceAndSwapChain,
    ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_MODE_DESC, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_PRESENT, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_DISCARD,
    DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGISwapChain,
};
use windows::core::BOOL;

use crate::VideoFrame;

/// A failure while creating or using the M1 D3D11 upload renderer.
#[derive(Debug, Error)]
pub enum RenderError {
    #[error("the window is not backed by a Win32 HWND")]
    UnsupportedWindow,
    #[error("the window handle is unavailable")]
    WindowHandle,
    #[error("D3D11 failed: {0}")]
    D3d11(#[from] windows::core::Error),
    #[error("decoded video dimensions are invalid")]
    InvalidFrame,
}

/// Safe owner of the D3D11 device, immediate context, and window swap chain.
pub struct SoftwareFrameRenderer {
    _device: ID3D11Device,
    context: ID3D11DeviceContext,
    swap_chain: IDXGISwapChain,
    buffer_dimensions: Option<(u32, u32)>,
}

impl SoftwareFrameRenderer {
    pub fn new(window: &impl HasWindowHandle) -> Result<Self, RenderError> {
        let raw_handle = window
            .window_handle()
            .map_err(|_| RenderError::WindowHandle)?
            .as_raw();
        let RawWindowHandle::Win32(handle) = raw_handle else {
            return Err(RenderError::UnsupportedWindow);
        };
        let hwnd = HWND(handle.hwnd.get() as *mut _);
        let description = DXGI_SWAP_CHAIN_DESC {
            BufferDesc: DXGI_MODE_DESC {
                Width: 1,
                Height: 1,
                RefreshRate: DXGI_RATIONAL::default(),
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                ..Default::default()
            },
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            OutputWindow: hwnd,
            Windowed: BOOL(1),
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            Flags: 0,
        };

        let mut device = None;
        let mut context = None;
        let mut swap_chain = None;
        let mut feature_level = D3D_FEATURE_LEVEL::default();
        // The returned COM interfaces are owned by this renderer and are used only
        // on the winit event-loop thread where the renderer is constructed.
        unsafe {
            D3D11CreateDeviceAndSwapChain(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&description),
                Some(&mut swap_chain),
                Some(&mut device),
                Some(&mut feature_level),
                Some(&mut context),
            )?;
        }

        Ok(Self {
            _device: device.expect("D3D11 returned success without a device"),
            context: context.expect("D3D11 returned success without a context"),
            swap_chain: swap_chain.expect("D3D11 returned success without a swap chain"),
            buffer_dimensions: None,
        })
    }

    /// Uploads one tightly packed RGBA frame and presents it to the window.
    pub fn present(&mut self, frame: &VideoFrame) -> Result<(), RenderError> {
        if frame.width == 0 || frame.height == 0 {
            return Err(RenderError::InvalidFrame);
        }
        let row_pitch = frame
            .width
            .checked_mul(4)
            .ok_or(RenderError::InvalidFrame)?;
        let expected_len = (row_pitch as usize)
            .checked_mul(frame.height as usize)
            .ok_or(RenderError::InvalidFrame)?;
        if frame.rgba.len() != expected_len {
            return Err(RenderError::InvalidFrame);
        }

        if self.buffer_dimensions != Some((frame.width, frame.height)) {
            // No back-buffer references are retained across ResizeBuffers.
            unsafe {
                self.swap_chain.ResizeBuffers(
                    2,
                    frame.width,
                    frame.height,
                    DXGI_FORMAT_R8G8B8A8_UNORM,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )?;
            }
            self.buffer_dimensions = Some((frame.width, frame.height));
        }

        // The back buffer is borrowed only for this upload. D3D11 copies the CPU
        // bytes before UpdateSubresource returns, so `frame` need not outlive it.
        unsafe {
            let back_buffer: ID3D11Texture2D = self.swap_chain.GetBuffer(0)?;
            self.context.UpdateSubresource(
                &back_buffer,
                0,
                None,
                frame.rgba.as_ptr().cast(),
                row_pitch,
                0,
            );
            self.swap_chain.Present(1, DXGI_PRESENT(0)).ok()?;
        }
        Ok(())
    }
}
