// This file contains implementations inspired by or derived from the following
// sources:
// - https://github.com/ohchase/egui-directx/blob/master/egui-directx11/src/texture.rs
//
// Here I would express my gratitude for their contributions to the Rust
// community. Their work served as a valuable reference and inspiration for this
// project.
//
// Nekomaru, March 2024

use std::{collections::HashMap, mem, slice};

use egui::{Color32, ImageData, TextureId, TexturesDelta};

use windows::{
    Win32::Graphics::{Direct3D11::*, Dxgi::Common::*},
    core::Result,
};

struct ManagedTexture {
    tex: ID3D11Texture2D,
    srv: ID3D11ShaderResourceView,
    pixels: Vec<Color32>,
    width: usize,
    options: egui::TextureOptions,
}

enum Texture {
    /// A texture managed by egui (created from ImageData)
    Managed(ManagedTexture),
    /// A user-provided texture (registered from an existing shader resource view)
    User { srv: ID3D11ShaderResourceView },
}
impl Texture {
    pub fn is_managed(&self) -> bool {
        matches!(self, Texture::Managed(_))
    }

    pub fn is_user(&self) -> bool {
        matches!(self, Texture::User { .. })
    }
}

pub struct TexturePool {
    device: ID3D11Device,
    pool: HashMap<TextureId, Texture>,
    next_user_texture_id: u64,
}

impl TexturePool {
    pub fn options(&self, id: TextureId) -> egui::TextureOptions {
        match self.pool.get(&id) {
            Some(Texture::Managed(texture)) => texture.options,
            _ => egui::TextureOptions::LINEAR,
        }
    }
    pub fn new(device: &ID3D11Device) -> Self {
        Self {
            device: device.clone(),
            pool: HashMap::new(),
            next_user_texture_id: 0,
        }
    }

    pub fn get_srv(&self, tid: TextureId) -> Option<ID3D11ShaderResourceView> {
        self.pool.get(&tid).map(|t| match t {
            Texture::Managed(managed) => managed.srv.clone(),
            Texture::User { srv } => srv.clone(),
        })
    }

    /// Register a user-provided shader resource view and get a TextureId for it.
    /// This TextureId can be used in egui to reference this texture.
    ///
    /// The returned TextureId will be unique and won't conflict with egui's managed textures.
    pub fn register_user_texture(&mut self, srv: ID3D11ShaderResourceView) -> TextureId {
        let id = TextureId::User(self.next_user_texture_id);
        self.next_user_texture_id += 1;
        self.pool.insert(id, Texture::User { srv });
        id
    }

    /// Unregister a user texture by its TextureId.
    /// Returns true if the texture was found and removed, false otherwise.
    pub fn unregister_user_texture(&mut self, tid: TextureId) -> bool {
        if self.pool.get(&tid).is_some_and(|t| t.is_user()) {
            self.pool.remove(&tid);
            true
        } else {
            false
        }
    }

    pub fn update(&mut self, ctx: &ID3D11DeviceContext, delta: TexturesDelta) -> Result<()> {
        for (tid, delta) in delta.set {
            if delta.is_whole() && delta.image.width() > 0 && delta.image.height() > 0 {
                self.pool.insert(
                    tid,
                    Self::create_managed_texture(&self.device, delta.image, delta.options)?,
                );
                // the old texture is returned and dropped here, freeing
                // all its gpu resource.
            } else if let Some(tex) = self.pool.get_mut(&tid).filter(|t| t.is_managed()) {
                if let Texture::Managed(texture) = tex {
                    texture.options = delta.options;
                }
                Self::update_partial(ctx, tex, delta.image, delta.pos.unwrap())?;
            } else {
                log::warn!(
                    "egui wants to update a non-existing texture {tid:?}. this request will be ignored."
                );
            }
        }
        for tid in delta.free {
            if self.pool.get(&tid).is_some_and(|t| t.is_managed()) {
                self.pool.remove(&tid);
            }
        }
        Ok(())
    }

    fn update_partial(
        ctx: &ID3D11DeviceContext,
        old: &mut Texture,
        image: ImageData,
        [nx, ny]: [usize; 2],
    ) -> Result<()> {
        let Texture::Managed(old) = old else {
            log::warn!("attempted to partially update a user texture, which is not supported");
            return Ok(());
        };

        let subr = unsafe {
            let mut output = D3D11_MAPPED_SUBRESOURCE::default();
            ctx.Map(&old.tex, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut output))?;
            output
        };
        match image {
            ImageData::Color(f) => {
                let data = unsafe {
                    let slice =
                        slice::from_raw_parts_mut(subr.pData as *mut Color32, old.pixels.len());
                    slice
                        .as_mut_ptr()
                        .copy_from_nonoverlapping(old.pixels.as_ptr(), old.pixels.len());
                    slice
                };

                for y in 0..f.height() {
                    for x in 0..f.width() {
                        let whole = (ny + y) * old.width + nx + x;
                        let frac = y * f.width() + x;
                        old.pixels[whole] = f.pixels[frac];
                        data[whole] = f.pixels[frac];
                    }
                }
            }
        }
        unsafe { ctx.Unmap(&old.tex, 0) };
        Ok(())
    }

    fn create_managed_texture(
        device: &ID3D11Device,
        data: ImageData,
        options: egui::TextureOptions,
    ) -> Result<Texture> {
        let width = data.width();

        let pixels = match &data {
            ImageData::Color(c) => c.pixels.clone(),
        };

        let desc = D3D11_TEXTURE2D_DESC {
            Width: data.width() as _,
            Height: data.height() as _,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as _,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as _,
            ..Default::default()
        };

        let subresource_data = D3D11_SUBRESOURCE_DATA {
            pSysMem: pixels.as_ptr() as _,
            SysMemPitch: (width * mem::size_of::<Color32>()) as u32,
            SysMemSlicePitch: 0,
        };

        let mut tex = None;
        unsafe { device.CreateTexture2D(&desc, Some(&subresource_data), Some(&mut tex)) }?;
        let tex = tex.unwrap();

        let mut srv = None;
        unsafe { device.CreateShaderResourceView(&tex, None, Some(&mut srv)) }?;
        let srv = srv.unwrap();

        Ok(Texture::Managed(ManagedTexture {
            tex,
            srv,
            width,
            pixels,
            options,
        }))
    }
}
