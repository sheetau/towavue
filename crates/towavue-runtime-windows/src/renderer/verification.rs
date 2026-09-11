use super::*;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_USAGE_STAGING,
};

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
