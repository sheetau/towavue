use std::mem::ManuallyDrop;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use thiserror::Error;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
    D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
    D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_STREAM,
    D3D11_VIDEO_USAGE_PLAYBACK_NORMAL, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D, D3D11CreateDeviceAndSwapChain, ID3D11Device,
    ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D, ID3D11VideoContext, ID3D11VideoDevice,
    ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_MODE_DESC, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_PRESENT, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_DISCARD,
    DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice, IDXGISwapChain,
};
use windows::core::{BOOL, Interface};

use crate::VideoFrame;
use crate::decode::HardwareVideoFrame;

/// A failure while creating or using the shared D3D11 renderer.
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
    #[error("the hardware frame does not contain a D3D11 texture")]
    InvalidHardwareFrame,
}

/// Stable identifier of the DXGI adapter selected for the shared device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdapterLuid {
    pub low_part: u32,
    pub high_part: i32,
}

/// Opaque, cloneable reference to the one D3D11 device used by playback.
#[derive(Clone)]
pub struct GraphicsDevice {
    device: ID3D11Device,
    adapter_luid: AdapterLuid,
}

impl GraphicsDevice {
    pub fn adapter_luid(&self) -> AdapterLuid {
        self.adapter_luid
    }

    pub(crate) fn clone_raw(&self) -> *mut std::ffi::c_void {
        self.device.clone().into_raw()
    }
}

struct VideoProcessorState {
    dimensions: (u32, u32),
    device: ID3D11VideoDevice,
    context: ID3D11VideoContext,
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
}

/// Safe owner of the D3D11 device, immediate context, and window swap chain.
pub struct FrameRenderer {
    graphics_device: GraphicsDevice,
    context: ID3D11DeviceContext,
    swap_chain: IDXGISwapChain,
    buffer_dimensions: Option<(u32, u32)>,
    video_processor: Option<VideoProcessorState>,
}

impl FrameRenderer {
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
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&description),
                Some(&mut swap_chain),
                Some(&mut device),
                Some(&mut feature_level),
                Some(&mut context),
            )?;
        }

        let device = device.expect("D3D11 returned success without a device");
        let context = context.expect("D3D11 returned success without a context");
        let multithread: ID3D11Multithread = context.cast()?;
        // FFmpeg decode and presentation issue commands from different threads.
        unsafe {
            let _ = multithread.SetMultithreadProtected(true);
        }
        let dxgi_device: IDXGIDevice = device.cast()?;
        // GetAdapter returns an owned COM reference and GetDesc copies the LUID;
        // both temporary values are released before this constructor returns.
        let adapter_luid = unsafe { dxgi_device.GetAdapter()?.GetDesc()?.AdapterLuid };

        Ok(Self {
            graphics_device: GraphicsDevice {
                device,
                adapter_luid: AdapterLuid {
                    low_part: adapter_luid.LowPart,
                    high_part: adapter_luid.HighPart,
                },
            },
            context,
            swap_chain: swap_chain.expect("D3D11 returned success without a swap chain"),
            buffer_dimensions: None,
            video_processor: None,
        })
    }

    pub fn graphics_device(&self) -> GraphicsDevice {
        self.graphics_device.clone()
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

        self.resize_buffers(frame.width, frame.height)?;

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

    pub(crate) fn present_hardware(
        &mut self,
        frame: &HardwareVideoFrame,
    ) -> Result<(), RenderError> {
        if frame.width == 0 || frame.height == 0 {
            return Err(RenderError::InvalidFrame);
        }
        self.resize_buffers(frame.width, frame.height)?;
        self.prepare_video_processor(frame.width, frame.height)?;

        let (texture_raw, array_slice) = frame
            .texture_and_slice()
            .ok_or(RenderError::InvalidHardwareFrame)?;
        // The AVFrame owns this COM reference for the duration of the call.
        let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&texture_raw) }
            .ok_or(RenderError::InvalidHardwareFrame)?;
        let state = self
            .video_processor
            .as_ref()
            .expect("video processor was prepared");
        let input_description = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: array_slice,
                },
            },
        };
        let output_description = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };

        // Views live only through this blit; the AVFrame and swap-chain buffer
        // retain their resources for the entire operation.
        unsafe {
            let mut input_view = None;
            state.device.CreateVideoProcessorInputView(
                texture,
                &state.enumerator,
                &input_description,
                Some(&mut input_view),
            )?;
            let back_buffer: ID3D11Texture2D = self.swap_chain.GetBuffer(0)?;
            let mut output_view = None;
            state.device.CreateVideoProcessorOutputView(
                &back_buffer,
                &state.enumerator,
                &output_description,
                Some(&mut output_view),
            )?;
            let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: BOOL(1),
                pInputSurface: ManuallyDrop::new(input_view),
                ..Default::default()
            };
            let result = state.context.VideoProcessorBlt(
                &state.processor,
                output_view.as_ref().expect("D3D11 returned an output view"),
                0,
                std::slice::from_ref(&stream),
            );
            ManuallyDrop::drop(&mut stream.pInputSurface);
            result?;
            self.swap_chain.Present(1, DXGI_PRESENT(0)).ok()?;
        }
        Ok(())
    }

    fn resize_buffers(&mut self, width: u32, height: u32) -> Result<(), RenderError> {
        if self.buffer_dimensions != Some((width, height)) {
            // No back-buffer views or references are retained across ResizeBuffers.
            unsafe {
                self.swap_chain.ResizeBuffers(
                    2,
                    width,
                    height,
                    DXGI_FORMAT_R8G8B8A8_UNORM,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )?;
            }
            self.buffer_dimensions = Some((width, height));
            self.video_processor = None;
        }
        Ok(())
    }

    fn prepare_video_processor(&mut self, width: u32, height: u32) -> Result<(), RenderError> {
        if self
            .video_processor
            .as_ref()
            .is_some_and(|state| state.dimensions == (width, height))
        {
            return Ok(());
        }
        let device: ID3D11VideoDevice = self.graphics_device.device.cast()?;
        let context: ID3D11VideoContext = self.context.cast()?;
        let description = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: DXGI_RATIONAL {
                Numerator: 1,
                Denominator: 1,
            },
            InputWidth: width,
            InputHeight: height,
            OutputFrameRate: DXGI_RATIONAL {
                Numerator: 1,
                Denominator: 1,
            },
            OutputWidth: width,
            OutputHeight: height,
            Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
        };
        // These COM objects are retained by the renderer and used only on its
        // event-loop thread; the shared immediate context is multithread-protected.
        unsafe {
            let enumerator = device.CreateVideoProcessorEnumerator(&description)?;
            let processor = device.CreateVideoProcessor(&enumerator, 0)?;
            self.video_processor = Some(VideoProcessorState {
                dimensions: (width, height),
                device,
                context,
                enumerator,
                processor,
            });
        }
        Ok(())
    }
}
