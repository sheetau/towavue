use windows::{
    Win32::Graphics::{Direct3D::*, Direct3D11::*, Dxgi::Common::*},
    core::Result,
};

pub(super) struct MipGenerator {
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    rasterizer: ID3D11RasterizerState,
}

impl MipGenerator {
    pub(super) fn new(device: &ID3D11Device) -> Result<Self> {
        let mut vertex = None;
        let mut pixel = None;
        let mut rasterizer = None;
        // Fixed build-time shaders and stack descriptors outlive each creation call.
        unsafe {
            device.CreateVertexShader(
                include_bytes!(concat!(env!("OUT_DIR"), "/mipmap-vertex.cso")),
                None,
                Some(&mut vertex),
            )?;
            device.CreatePixelShader(
                include_bytes!(concat!(env!("OUT_DIR"), "/mipmap-pixel.cso")),
                None,
                Some(&mut pixel),
            )?;
            device.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    DepthClipEnable: true.into(),
                    ..Default::default()
                },
                Some(&mut rasterizer),
            )?;
        }
        Ok(Self {
            vertex: vertex.unwrap(),
            pixel: pixel.unwrap(),
            rasterizer: rasterizer.unwrap(),
        })
    }

    pub(super) fn generate(
        &self,
        device: &ID3D11Device,
        ctx: &ID3D11DeviceContext,
        texture: &ID3D11Texture2D,
    ) -> Result<()> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // The caller serializes this same-device immediate context. Views own
        // references to distinct input/output subresources of the retained texture.
        // Build all views before changing pipeline bindings, so creation failure
        // leaves no half-bound mip pass. The renderer installs its UI pipeline
        // after all requested mip generation and before drawing any UI mesh.
        unsafe {
            texture.GetDesc(&mut desc);
            let mut views = Vec::new();
            for level in 1..desc.MipLevels {
                let mut source = None;
                let mut target = None;
                device.CreateShaderResourceView(
                    texture,
                    Some(&D3D11_SHADER_RESOURCE_VIEW_DESC {
                        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                        ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
                        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                            Texture2D: D3D11_TEX2D_SRV {
                                MostDetailedMip: level - 1,
                                MipLevels: 1,
                            },
                        },
                    }),
                    Some(&mut source),
                )?;
                device.CreateRenderTargetView(
                    texture,
                    Some(&D3D11_RENDER_TARGET_VIEW_DESC {
                        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                        ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
                        Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0 {
                            Texture2D: D3D11_TEX2D_RTV { MipSlice: level },
                        },
                    }),
                    Some(&mut target),
                )?;
                views.push((source.unwrap(), target.unwrap()));
            }
            ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx.IASetInputLayout(None);
            ctx.VSSetShader(&self.vertex, None);
            ctx.PSSetShader(&self.pixel, None);
            ctx.RSSetState(&self.rasterizer);
            ctx.OMSetBlendState(None, None, u32::MAX);
            for (index, (source, target)) in views.into_iter().enumerate() {
                let level = index + 1;
                ctx.PSSetShaderResources(0, Some(&[None]));
                ctx.OMSetRenderTargets(Some(&[Some(target)]), None);
                ctx.PSSetShaderResources(0, Some(&[Some(source)]));
                ctx.RSSetViewports(Some(&[D3D11_VIEWPORT {
                    Width: (desc.Width >> level).max(1) as f32,
                    Height: (desc.Height >> level).max(1) as f32,
                    MaxDepth: 1.0,
                    ..Default::default()
                }]));
                ctx.Draw(3, 0);
            }
            ctx.PSSetShaderResources(0, Some(&[None]));
            ctx.OMSetRenderTargets(None, None);
        }
        Ok(())
    }
}
