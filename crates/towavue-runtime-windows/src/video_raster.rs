use super::*;
use windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView;
use windows::Win32::Graphics::Direct3D11::{D3D11_SUBRESOURCE_DATA, D3D11_USAGE_IMMUTABLE};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R32G32_FLOAT,
};

#[path = "video_resample.rs"]
mod resample;

#[path = "video_raster_plan.rs"]
mod plan;
pub(crate) use plan::Plan;

struct Surface {
    specification: plan::Slot,
    texture: ID3D11Texture2D,
    target: ID3D11RenderTargetView,
    source: ID3D11ShaderResourceView,
}

struct Coefficients {
    specification: resample::FilterSpec,
    source: ID3D11ShaderResourceView,
}

pub(super) struct VideoRaster {
    shader: ID3D11PixelShader,
    constants: ID3D11Buffer,
    surfaces: Vec<Surface>,
    filters: Vec<Coefficients>,
}

impl VideoRaster {
    pub(super) fn new(device: &ID3D11Device) -> Result<Self, RenderError> {
        let code = compile_shader(include_bytes!("video_raster.hlsl"), s!("ps_4_0"))?;
        let mut shader = None;
        let mut constants = None;
        // Descriptors/bytecode are consumed synchronously. These owned objects are
        // used only by the renderer's event-loop thread and on its shared device.
        unsafe {
            device.CreatePixelShader(blob_bytes(&code), None, Some(&mut shader))?;
            device.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: 64,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut constants),
            )?;
        }
        Ok(Self {
            shader: shader.expect("D3D11 pixel shader"),
            constants: constants.expect("D3D11 constant buffer"),
            surfaces: Vec::new(),
            filters: Vec::new(),
        })
    }

    fn prepare(&mut self, device: &ID3D11Device, plan: &Plan) -> Result<(), RenderError> {
        if self
            .surfaces
            .iter()
            .map(|surface| surface.specification)
            .eq(plan.slots.iter().copied())
            && self
                .filters
                .iter()
                .map(|filter| filter.specification)
                .eq(plan.filters.iter().copied())
        {
            return Ok(());
        }
        // No stage views stay bound after draw. Drop the old pool before allocation,
        // so changing histories does not double the validated 512 MiB payload budget.
        self.surfaces.clear();
        self.filters.clear();
        for specification in &plan.slots {
            let (width, height) = specification.size;
            let mut texture = None;
            let mut target = None;
            let mut source = None;
            // The renderer owns all views/textures. Default-usage RGBA resources stay
            // on this device; production never maps or copies them to a CPU resource.
            unsafe {
                device.CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        Width: width,
                        Height: height,
                        MipLevels: 1,
                        ArraySize: 1,
                        Format: if specification.floating_point {
                            DXGI_FORMAT_R16G16B16A16_FLOAT
                        } else {
                            DXGI_FORMAT_R8G8B8A8_UNORM
                        },
                        SampleDesc: DXGI_SAMPLE_DESC {
                            Count: 1,
                            Quality: 0,
                        },
                        Usage: D3D11_USAGE_DEFAULT,
                        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0)
                            as u32,
                        ..Default::default()
                    },
                    None,
                    Some(&mut texture),
                )?;
                let texture = texture.as_ref().expect("D3D11 raster texture");
                device.CreateRenderTargetView(texture, None, Some(&mut target))?;
                device.CreateShaderResourceView(texture, None, Some(&mut source))?;
            }
            self.surfaces.push(Surface {
                specification: *specification,
                texture: texture.expect("D3D11 raster texture"),
                target: target.expect("D3D11 raster target"),
                source: source.expect("D3D11 raster source"),
            });
        }
        for &specification in &plan.filters {
            let coefficients = specification.coefficients();
            let mut texture = None;
            let mut source = None;
            // Immutable coefficient upload copies this owned slice synchronously.
            // The SRV retains its same-device texture after the local COM owner drops.
            // It is only bound by this renderer's serialized event-loop thread.
            unsafe {
                device.CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        Width: specification.taps(),
                        Height: specification.target,
                        MipLevels: 1,
                        ArraySize: 1,
                        Format: DXGI_FORMAT_R32G32_FLOAT,
                        SampleDesc: DXGI_SAMPLE_DESC {
                            Count: 1,
                            Quality: 0,
                        },
                        Usage: D3D11_USAGE_IMMUTABLE,
                        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                        ..Default::default()
                    },
                    Some(&D3D11_SUBRESOURCE_DATA {
                        pSysMem: coefficients.as_ptr().cast(),
                        SysMemPitch: specification.taps() * 8,
                        SysMemSlicePitch: 0,
                    }),
                    Some(&mut texture),
                )?;
                device.CreateShaderResourceView(
                    texture.as_ref().expect("coefficient texture"),
                    None,
                    Some(&mut source),
                )?;
            }
            self.filters.push(Coefficients {
                specification,
                source: source.expect("coefficient source"),
            });
        }
        Ok(())
    }

    pub(super) fn draw(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        blitter: &SoftwareBlitter,
        texture: &ID3D11Texture2D,
        plan: &Plan,
    ) -> Result<ID3D11Texture2D, RenderError> {
        self.prepare(device, plan)?;
        let Some(last) = plan.stages.last() else {
            return Ok(texture.clone());
        };
        let mut source = None;
        // Input is a renderer-owned software upload or Video Processor RGBA output.
        // All resources share the protected immediate context; the event-loop thread
        // alone issues these stages. Each target differs from the immediately previous
        // source. Commands retain resource lifetimes and execute in submission order.
        unsafe {
            device.CreateShaderResourceView(texture, None, Some(&mut source))?;
            context.RSSetState(None);
            context.OMSetBlendState(None, None, u32::MAX);
            context.OMSetDepthStencilState(None, 0);
            context.IASetInputLayout(None::<&ID3D11InputLayout>);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&blitter.vertex_shader, None);
            context.PSSetShader(&self.shader, None);
            context.PSSetSamplers(0, Some(&[Some(blitter.sampler.clone())]));
            context.PSSetConstantBuffers(0, Some(&[Some(self.constants.clone())]));
            for stage in &plan.stages {
                let surface = &self.surfaces[stage.slot];
                context.UpdateSubresource(
                    &self.constants,
                    0,
                    None,
                    stage.constants.as_ptr().cast(),
                    0,
                    0,
                );
                context.OMSetRenderTargets(
                    Some(&[Some(surface.target.clone())]),
                    None::<&ID3D11DepthStencilView>,
                );
                context.PSSetShaderResources(
                    0,
                    Some(&[
                        source,
                        stage.filter.map(|index| self.filters[index].source.clone()),
                    ]),
                );
                context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                    Width: stage.size.0 as f32,
                    Height: stage.size.1 as f32,
                    MinDepth: 0.0,
                    MaxDepth: 1.0,
                    ..Default::default()
                }]));
                context.Draw(3, 0);
                context.PSSetShaderResources(0, Some(&[None, None]));
                context.OMSetRenderTargets(None, None::<&ID3D11DepthStencilView>);
                source = Some(surface.source.clone());
            }
        }
        Ok(self.surfaces[last.slot].texture.clone())
    }
}

#[cfg(test)]
#[path = "video_raster_tests.rs"]
mod tests;
