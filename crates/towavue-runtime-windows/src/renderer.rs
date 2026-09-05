use std::mem::ManuallyDrop;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use thiserror::Error;
use towavue_core::UnitPoint;
use windows::Win32::Foundation::{HMODULE, HWND, RECT};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_9_3, D3D_FEATURE_LEVEL_10_0,
    D3D_FEATURE_LEVEL_11_0, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, ID3DBlob, ID3DInclude,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
    D3D11_BUFFER_DESC, D3D11_COMPARISON_ALWAYS, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_SAMPLER_DESC,
    D3D11_SDK_VERSION, D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_TEXTURE_ADDRESS_CLAMP,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
    D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_STREAM,
    D3D11_VIDEO_USAGE_PLAYBACK_NORMAL, D3D11_VIEWPORT, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D, D3D11CreateDeviceAndSwapChain, ID3D11Buffer,
    ID3D11DepthStencilView, ID3D11Device, ID3D11DeviceContext, ID3D11InputLayout,
    ID3D11Multithread, ID3D11PixelShader, ID3D11RenderTargetView, ID3D11SamplerState,
    ID3D11Texture2D, ID3D11VertexShader, ID3D11VideoContext, ID3D11VideoContext1,
    ID3D11VideoDevice, ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
    ID3D11VideoProcessorEnumerator1,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
    DXGI_COLOR_SPACE_YCBCR_STUDIO_G2084_LEFT_P2020,
    DXGI_COLOR_SPACE_YCBCR_STUDIO_GHLG_TOPLEFT_P2020, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_MODE_DESC,
    DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_PRESENT, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_DISCARD,
    DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice, IDXGISwapChain,
};
use windows::core::{BOOL, Interface, PCSTR, s};

use crate::VideoFrame;
use crate::decode::{HardwareVideoFrame, VideoTransfer};

/// A failure while creating or using the shared D3D11 renderer.
#[derive(Debug, Error)]
pub enum RenderError {
    #[error("the window is not backed by a Win32 HWND")]
    UnsupportedWindow,
    #[error("the window handle is unavailable")]
    WindowHandle,
    #[error("D3D11 failed: {0}")]
    D3d11(#[from] windows::core::Error),
    #[error("the D3D11 device was removed: {0}")]
    DeviceRemoved(String),
    #[error("decoded video dimensions are invalid")]
    InvalidFrame,
    #[error("the D3D11 presentation surface has not been sized")]
    SurfaceNotSized,
    #[error("the hardware frame does not contain a D3D11 texture")]
    InvalidHardwareFrame,
    #[error("the D3D11 Video Processor cannot tone-map this HDR frame to SDR")]
    HdrConversionUnsupported,
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

    pub(crate) fn device_removed_reason(&self) -> Option<String> {
        // The device is retained by this safe handle; the call only reads its
        // terminal removal status and does not retain native state.
        unsafe { self.device.GetDeviceRemovedReason().err() }.map(|error| error.to_string())
    }
}

struct VideoProcessorState {
    dimensions: (u32, u32, u32, u32),
    device: ID3D11VideoDevice,
    context: ID3D11VideoContext,
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
}

struct SoftwareBlitter {
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    uv_constants: ID3D11Buffer,
}

impl SoftwareBlitter {
    fn new(device: &ID3D11Device) -> Result<Self, RenderError> {
        const VERTEX_SHADER: &[u8] = br#"
struct VertexOutput {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
};

VertexOutput main(uint vertex_id : SV_VertexID) {
    VertexOutput output;
    output.uv = float2((vertex_id << 1) & 2, vertex_id & 2);
    output.position = float4(output.uv.x * 2.0 - 1.0, 1.0 - output.uv.y * 2.0, 0.0, 1.0);
    return output;
}
"#;
        const PIXEL_SHADER: &[u8] = br#"
Texture2D source_texture : register(t0);
SamplerState source_sampler : register(s0);
cbuffer Transform : register(b0) {
    float4 origin;
    float4 x_axis;
    float4 y_axis;
};

float4 main(float4 position : SV_POSITION, float2 uv : TEXCOORD0) : SV_TARGET {
    return source_texture.Sample(source_sampler, origin.xy + uv.x * x_axis.xy + uv.y * y_axis.xy);
}
"#;

        let vertex_bytecode = compile_shader(VERTEX_SHADER, s!("vs_4_0"))?;
        let pixel_bytecode = compile_shader(PIXEL_SHADER, s!("ps_4_0"))?;
        let mut vertex_shader = None;
        let mut pixel_shader = None;
        let mut sampler = None;
        let mut uv_constants = None;
        let sampler_description = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
            ComparisonFunc: D3D11_COMPARISON_ALWAYS,
            MaxLOD: f32::MAX,
            ..Default::default()
        };
        // Creation consumes the borrowed bytecode/descriptors synchronously. The renderer owns
        // the shaders, sampler and constant buffer; buffer updates stay on its event-loop thread.
        unsafe {
            device.CreateVertexShader(
                blob_bytes(&vertex_bytecode),
                None,
                Some(&mut vertex_shader),
            )?;
            device.CreatePixelShader(blob_bytes(&pixel_bytecode), None, Some(&mut pixel_shader))?;
            device.CreateSamplerState(&sampler_description, Some(&mut sampler))?;
            device.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: 48,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut uv_constants),
            )?;
        }
        Ok(Self {
            vertex_shader: vertex_shader.expect("D3D11 returned success without a vertex shader"),
            pixel_shader: pixel_shader.expect("D3D11 returned success without a pixel shader"),
            sampler: sampler.expect("D3D11 returned success without a sampler"),
            uv_constants: uv_constants.expect("D3D11 returned success without a constant buffer"),
        })
    }
}

/// Safe owner of the D3D11 device, immediate context, and window swap chain.
pub struct FrameRenderer {
    graphics_device: GraphicsDevice,
    max_texture_side: usize,
    context: ID3D11DeviceContext,
    swap_chain: IDXGISwapChain,
    buffer_dimensions: Option<(u32, u32)>,
    video_processor: Option<VideoProcessorState>,
    software_texture: Option<(u32, u32, ID3D11Texture2D)>,
    edit_texture: Option<(u32, u32, ID3D11Texture2D)>,
    software_blitter: SoftwareBlitter,
    ui_renderer: egui_directx11::Renderer,
    hdr_tone_mapping_active: bool,
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

        let software_blitter = SoftwareBlitter::new(&device)?;
        let ui_renderer = egui_directx11::Renderer::new(&device)?;
        Ok(Self {
            max_texture_side: texture_side_limit(feature_level),
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
            software_texture: None,
            edit_texture: None,
            software_blitter,
            ui_renderer,
            hdr_tone_mapping_active: false,
        })
    }

    pub fn graphics_device(&self) -> GraphicsDevice {
        self.graphics_device.clone()
    }

    pub fn max_texture_side(&self) -> usize {
        self.max_texture_side
    }

    pub(crate) fn device_removed_reason(&self) -> Option<String> {
        self.graphics_device.device_removed_reason()
    }

    /// Uploads one tightly packed RGBA frame and draws it to the shared surface.
    pub(crate) fn draw_software(
        &mut self,
        frame: &VideoFrame,
        destination: egui::Rect,
        uv: [UnitPoint; 4],
    ) -> Result<(), RenderError> {
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

        self.ensure_surface()?;
        if self
            .software_texture
            .as_ref()
            .is_none_or(|(width, height, _)| (*width, *height) != (frame.width, frame.height))
        {
            let description = D3D11_TEXTURE2D_DESC {
                Width: frame.width,
                Height: frame.height,
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
            };
            let mut texture = None;
            // The texture stays owned by the renderer and is replaced only when dimensions change.
            unsafe {
                self.graphics_device.device.CreateTexture2D(
                    &description,
                    None,
                    Some(&mut texture),
                )?;
            }
            self.software_texture = Some((
                frame.width,
                frame.height,
                texture.expect("D3D11 returned success without a software texture"),
            ));
        }
        let texture = self
            .software_texture
            .as_ref()
            .expect("software texture was prepared")
            .2
            .clone();
        // D3D11 copies the CPU bytes before UpdateSubresource returns.
        unsafe {
            self.context.UpdateSubresource(
                &texture,
                0,
                None,
                frame.rgba.as_ptr().cast(),
                row_pitch,
                0,
            );
        }
        self.draw_software_texture(&texture, destination, uv)
    }

    pub(crate) fn draw_hardware(
        &mut self,
        frame: &HardwareVideoFrame,
        destination: egui::Rect,
        uv: [UnitPoint; 4],
    ) -> Result<(), RenderError> {
        if frame.width == 0 || frame.height == 0 {
            return Err(RenderError::InvalidFrame);
        }
        self.ensure_surface()?;

        let (texture_raw, array_slice) = frame
            .texture_and_slice()
            .ok_or(RenderError::InvalidHardwareFrame)?;
        // The AVFrame owns this COM reference for the duration of the call.
        let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&texture_raw) }
            .ok_or(RenderError::InvalidHardwareFrame)?;
        let edited = uv
            != [
                UnitPoint { x: 0.0, y: 0.0 },
                UnitPoint { x: 1.0, y: 0.0 },
                UnitPoint { x: 1.0, y: 1.0 },
                UnitPoint { x: 0.0, y: 1.0 },
            ];
        let output = if edited {
            Some(self.prepare_edit_texture(frame.width, frame.height)?)
        } else {
            None
        };
        let processor_destination = if edited {
            egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(frame.width as f32, frame.height as f32),
            )
        } else {
            destination
        };
        self.blit_texture(
            texture,
            array_slice,
            frame,
            processor_destination,
            output.as_ref(),
        )?;
        if let Some(output) = output {
            self.draw_software_texture(&output, destination, uv)?;
        }
        if frame.transfer != VideoTransfer::Sdr && !self.hdr_tone_mapping_active {
            self.hdr_tone_mapping_active = true;
            eprintln!(
                "towavue: HDR source tone-mapped to the SDR swap chain by D3D11 Video Processor"
            );
        }
        Ok(())
    }

    fn prepare_edit_texture(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<ID3D11Texture2D, RenderError> {
        if self
            .edit_texture
            .as_ref()
            .is_none_or(|(w, h, _)| (*w, *h) != (width, height))
        {
            let description = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                ..Default::default()
            };
            let mut texture = None;
            // The renderer owns one same-device GPU-only output texture. It is reused on the
            // event-loop thread, whose immediate context is shared with FFmpeg under protection.
            unsafe {
                self.graphics_device.device.CreateTexture2D(
                    &description,
                    None,
                    Some(&mut texture),
                )?;
            }
            self.edit_texture = Some((
                width,
                height,
                texture.expect("D3D11 returned success without an edit texture"),
            ));
        }
        Ok(self
            .edit_texture
            .as_ref()
            .expect("edit texture was prepared")
            .2
            .clone())
    }

    fn blit_texture(
        &mut self,
        texture: &ID3D11Texture2D,
        array_slice: u32,
        frame: &HardwareVideoFrame,
        destination: egui::Rect,
        output: Option<&ID3D11Texture2D>,
    ) -> Result<(), RenderError> {
        let (output_width, output_height) = if output.is_some() {
            (frame.width, frame.height)
        } else {
            self.ensure_surface()?
        };
        self.prepare_video_processor(frame.width, frame.height, output_width, output_height)?;
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

        // Views live only through this event-loop blit; the AVFrame and output owners
        // retain their resources for the entire operation.
        unsafe {
            let mut input_view = None;
            state.device.CreateVideoProcessorInputView(
                texture,
                &state.enumerator,
                &input_description,
                Some(&mut input_view),
            )?;
            let back_buffer: ID3D11Texture2D = match output {
                Some(texture) => texture.clone(),
                None => self.swap_chain.GetBuffer(0)?,
            };
            let mut output_view = None;
            state.device.CreateVideoProcessorOutputView(
                &back_buffer,
                &state.enumerator,
                &output_description,
                Some(&mut output_view),
            )?;
            let destination = RECT {
                left: destination.left().round() as i32,
                top: destination.top().round() as i32,
                right: destination.right().round() as i32,
                bottom: destination.bottom().round() as i32,
            };
            state.context.VideoProcessorSetStreamDestRect(
                &state.processor,
                0,
                true,
                Some(&destination),
            );
            state.context.VideoProcessorSetOutputTargetRect(
                &state.processor,
                true,
                Some(&destination),
            );
            configure_hdr_to_sdr(state, texture, frame.transfer)?;
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
        }
        Ok(())
    }

    fn draw_software_texture(
        &self,
        texture: &ID3D11Texture2D,
        destination: egui::Rect,
        uv: [UnitPoint; 4],
    ) -> Result<(), RenderError> {
        self.ensure_surface()?;
        let render_target = self.render_target()?;
        let mut source_view = None;
        let constants = [
            [uv[0].x, uv[0].y, 0.0, 0.0],
            [uv[1].x - uv[0].x, uv[1].y - uv[0].y, 0.0, 0.0],
            [uv[3].x - uv[0].x, uv[3].y - uv[0].y, 0.0, 0.0],
        ];
        // The resource view borrows the renderer-owned texture. All pipeline
        // objects and the immediate context remain on the event-loop thread.
        unsafe {
            // UpdateSubresource copies all 48 stack-owned bytes before returning.
            self.context.UpdateSubresource(
                &self.software_blitter.uv_constants,
                0,
                None,
                constants.as_ptr().cast(),
                0,
                0,
            );
            self.context
                .PSSetConstantBuffers(0, Some(&[Some(self.software_blitter.uv_constants.clone())]));
            self.graphics_device.device.CreateShaderResourceView(
                texture,
                None,
                Some(&mut source_view),
            )?;
            self.context.OMSetRenderTargets(
                Some(&[Some(render_target)]),
                None::<&ID3D11DepthStencilView>,
            );
            // UI rendering leaves scissor and blending enabled; video must not
            // inherit the last UI mesh's clipping rectangle or alpha state.
            self.context.RSSetState(None);
            self.context.OMSetBlendState(None, None, u32::MAX);
            self.context.OMSetDepthStencilState(None, 0);
            self.context.IASetInputLayout(None::<&ID3D11InputLayout>);
            self.context
                .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.context
                .VSSetShader(&self.software_blitter.vertex_shader, None);
            self.context
                .PSSetShader(&self.software_blitter.pixel_shader, None);
            self.context
                .PSSetSamplers(0, Some(&[Some(self.software_blitter.sampler.clone())]));
            self.context.PSSetShaderResources(0, Some(&[source_view]));
            self.context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: destination.left().round(),
                TopLeftY: destination.top().round(),
                Width: destination.right().round() - destination.left().round(),
                Height: destination.bottom().round() - destination.top().round(),
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            self.context.Draw(3, 0);
            self.context.PSSetShaderResources(0, Some(&[None]));
        }
        Ok(())
    }

    pub fn resize_surface(&mut self, width: u32, height: u32) -> Result<(), RenderError> {
        if width == 0 || height == 0 {
            return Ok(());
        }
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

    pub fn clear(&mut self, color: [f32; 4]) -> Result<(), RenderError> {
        self.ensure_surface()?;
        let render_target = self.render_target()?;
        // The target is valid until the next ResizeBuffers call.
        unsafe { self.context.ClearRenderTargetView(&render_target, &color) };
        Ok(())
    }

    pub fn render_ui(
        &mut self,
        context: &egui::Context,
        output: egui::FullOutput,
    ) -> Result<egui::PlatformOutput, RenderError> {
        self.ensure_surface()?;
        let render_target = self.render_target()?;
        let (renderer_output, platform_output, _) = egui_directx11::split_output(output);
        self.ui_renderer
            .render(&self.context, &render_target, context, renderer_output)?;
        Ok(platform_output)
    }

    pub fn present_surface(&self) -> Result<(), RenderError> {
        // The swap chain and device stay owned for the duration of presentation.
        unsafe { self.swap_chain.Present(1, DXGI_PRESENT(0)).ok()? };
        Ok(())
    }

    fn ensure_surface(&self) -> Result<(u32, u32), RenderError> {
        self.buffer_dimensions.ok_or(RenderError::SurfaceNotSized)
    }

    fn render_target(&self) -> Result<ID3D11RenderTargetView, RenderError> {
        let back_buffer: ID3D11Texture2D = unsafe { self.swap_chain.GetBuffer(0)? };
        let mut render_target = None;
        // The returned view owns its reference to the current back buffer.
        unsafe {
            self.graphics_device.device.CreateRenderTargetView(
                &back_buffer,
                None,
                Some(&mut render_target),
            )?;
        }
        Ok(render_target.expect("D3D11 returned success without a render target view"))
    }

    fn prepare_video_processor(
        &mut self,
        width: u32,
        height: u32,
        output_width: u32,
        output_height: u32,
    ) -> Result<(), RenderError> {
        if self
            .video_processor
            .as_ref()
            .is_some_and(|state| state.dimensions == (width, height, output_width, output_height))
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
            OutputWidth: output_width,
            OutputHeight: output_height,
            Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
        };
        // These COM objects are retained by the renderer and used only on its
        // event-loop thread; the shared immediate context is multithread-protected.
        unsafe {
            let enumerator = device.CreateVideoProcessorEnumerator(&description)?;
            let processor = device.CreateVideoProcessor(&enumerator, 0)?;
            let source = RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            };
            let destination = RECT {
                left: 0,
                top: 0,
                right: output_width as i32,
                bottom: output_height as i32,
            };
            context.VideoProcessorSetStreamSourceRect(&processor, 0, true, Some(&source));
            context.VideoProcessorSetStreamDestRect(&processor, 0, true, Some(&destination));
            context.VideoProcessorSetOutputTargetRect(&processor, true, Some(&destination));
            self.video_processor = Some(VideoProcessorState {
                dimensions: (width, height, output_width, output_height),
                device,
                context,
                enumerator,
                processor,
            });
        }
        Ok(())
    }
}

fn texture_side_limit(level: D3D_FEATURE_LEVEL) -> usize {
    if level.0 >= D3D_FEATURE_LEVEL_11_0.0 {
        16_384
    } else if level.0 >= D3D_FEATURE_LEVEL_10_0.0 {
        8_192
    } else if level.0 >= D3D_FEATURE_LEVEL_9_3.0 {
        4_096
    } else {
        2_048
    }
}

fn configure_hdr_to_sdr(
    state: &VideoProcessorState,
    texture: &ID3D11Texture2D,
    transfer: VideoTransfer,
) -> Result<(), RenderError> {
    let input_color_space = match transfer {
        VideoTransfer::Sdr => DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
        VideoTransfer::Pq => DXGI_COLOR_SPACE_YCBCR_STUDIO_G2084_LEFT_P2020,
        VideoTransfer::Hlg => DXGI_COLOR_SPACE_YCBCR_STUDIO_GHLG_TOPLEFT_P2020,
    };
    let context: ID3D11VideoContext1 = match state.context.cast() {
        Ok(context) => context,
        Err(_) if transfer == VideoTransfer::Sdr => return Ok(()),
        Err(_) => return Err(RenderError::HdrConversionUnsupported),
    };
    let enumerator: ID3D11VideoProcessorEnumerator1 = state
        .enumerator
        .cast()
        .map_err(|_| RenderError::HdrConversionUnsupported)?;
    let mut description = D3D11_TEXTURE2D_DESC::default();
    // The texture and processor interfaces remain owned by the current frame and
    // renderer for every call; the conversion capability is checked before use.
    unsafe {
        texture.GetDesc(&mut description);
        let supported = enumerator.CheckVideoProcessorFormatConversion(
            description.Format,
            input_color_space,
            DXGI_FORMAT_R8G8B8A8_UNORM,
            DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709,
        )?;
        if !supported.as_bool() {
            return Err(RenderError::HdrConversionUnsupported);
        }
        context.VideoProcessorSetStreamColorSpace1(&state.processor, 0, input_color_space);
        context.VideoProcessorSetOutputColorSpace1(
            &state.processor,
            DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709,
        );
    }
    Ok(())
}

fn compile_shader(source: &[u8], target: PCSTR) -> Result<ID3DBlob, RenderError> {
    let mut bytecode = None;
    // D3DCompile reads the static source for the duration of this call and
    // returns an owned blob. No include handler or caller-owned pointers escape.
    unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR::null(),
            None,
            None::<&ID3DInclude>,
            s!("main"),
            target,
            0,
            0,
            &mut bytecode,
            None,
        )?;
    }
    Ok(bytecode.expect("D3DCompile returned success without bytecode"))
}

unsafe fn blob_bytes(blob: &ID3DBlob) -> &[u8] {
    // The returned slice cannot outlive the borrowed COM blob and is consumed
    // synchronously by D3D11 shader creation.
    unsafe { std::slice::from_raw_parts(blob.GetBufferPointer().cast(), blob.GetBufferSize()) }
}
