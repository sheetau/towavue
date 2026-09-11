// This file contains implementations inspired by or derived from the following
// sources:
// - https://github.com/ohchase/egui-directx/blob/master/egui-directx11/src/texture.rs
//
// Here I would express my gratitude for their contributions to the Rust
// community. Their work served as a valuable reference and inspiration for this
// project.
//
// Nekomaru, March 2024

use std::{collections::HashMap, mem, sync::Arc};

use egui::{Color32, ImageData, TextureId, TexturesDelta};

use windows::{
    Win32::{
        Foundation::E_INVALIDARG,
        Graphics::{Direct3D11::*, Dxgi::Common::*},
    },
    core::Result,
};

struct ManagedTexture {
    tex: ID3D11Texture2D,
    srv: ID3D11ShaderResourceView,
    image: Arc<egui::ColorImage>,
    options: egui::TextureOptions,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::slice;
    use windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
    };

    fn device(driver: D3D_DRIVER_TYPE) -> Result<Option<(ID3D11Device, ID3D11DeviceContext)>> {
        let mut device = None;
        let mut context = None;
        // Test-owned offscreen resources; no application or desktop surface is touched.
        let result = unsafe {
            D3D11CreateDevice(
                None,
                driver,
                windows::Win32::Foundation::HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        };
        if let Err(error) = result {
            if driver == D3D_DRIVER_TYPE_HARDWARE {
                eprintln!("SKIP hardware texture test: no device: {error}");
                return Ok(None);
            }
            return Err(error);
        }
        Ok(Some((device.unwrap(), context.unwrap())))
    }

    fn readback(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        texture: &ID3D11Texture2D,
    ) -> Result<Vec<Color32>> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // This synchronous test exclusively owns the texture, staging and context.
        unsafe {
            texture.GetDesc(&mut desc);
        }
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let mut staging = None;
        unsafe {
            device.CreateTexture2D(&desc, None, Some(&mut staging))?;
        }
        let staging = staging.unwrap();
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let mut pixels = Vec::with_capacity((desc.Width * desc.Height) as usize);
        // Copy/Map waits for GPU writes. Borrow each padded row only until Unmap.
        unsafe {
            context.CopyResource(&staging, texture);
            context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            for y in 0..desc.Height as usize {
                pixels.extend_from_slice(slice::from_raw_parts(
                    mapped
                        .pData
                        .cast::<u8>()
                        .add(y * mapped.RowPitch as usize)
                        .cast::<Color32>(),
                    desc.Width as usize,
                ));
            }
            context.Unmap(&staging, 0);
        }
        Ok(pixels)
    }

    #[test]
    fn partial_texture_updates_preserve_padded_rows() -> Result<()> {
        for driver in [D3D_DRIVER_TYPE_WARP, D3D_DRIVER_TYPE_HARDWARE] {
            let Some((device, context)) = device(driver)? else {
                continue;
            };
            for width in [1, 3, 17, 63, 64, 65, 257] {
                let original = Arc::new(egui::ColorImage::new(
                    [width, 3],
                    (0..width * 3)
                        .map(|n| {
                            Color32::from_rgba_premultiplied(
                                (n * 20) as u8,
                                80,
                                150,
                                (n * 37) as u8,
                            )
                        })
                        .collect(),
                ));
                let mut texture = TexturePool::create_managed_texture(
                    &device,
                    ImageData::Color(original.clone()),
                    egui::TextureOptions::LINEAR,
                )?;
                let Texture::Managed(managed) = &texture else {
                    unreachable!()
                };
                assert!(
                    Arc::ptr_eq(&managed.image, &original),
                    "whole upload must not copy pixels"
                );
                assert_eq!(readback(&device, &context, &managed.tex)?, original.pixels);
                let x = width / 2;
                let patch = Arc::new(egui::ColorImage::new(
                    [1, 2],
                    vec![Color32::RED, Color32::GREEN],
                ));
                TexturePool::update_partial(
                    &context,
                    &mut texture,
                    ImageData::Color(patch),
                    [x, 1],
                )?;
                let Texture::Managed(managed) = &texture else {
                    unreachable!()
                };
                assert!(
                    !Arc::ptr_eq(&managed.image, &original),
                    "shared pixels detach before mutation"
                );
                let mut expected = original.pixels.clone();
                expected[width + x] = Color32::RED;
                expected[2 * width + x] = Color32::GREEN;
                assert_eq!(
                    readback(&device, &context, &managed.tex)?,
                    expected,
                    "padded rows on {driver:?}, width={width}"
                );
                assert_eq!(
                    original.pixels[width + x],
                    Color32::from_rgba_premultiplied(
                        ((width + x) * 20) as u8,
                        80,
                        150,
                        ((width + x) * 37) as u8
                    ),
                    "external snapshot unchanged"
                );
                let unique = Arc::as_ptr(&managed.image);
                let patch = Arc::new(egui::ColorImage::filled([width, 1], Color32::BLUE));
                TexturePool::update_partial(
                    &context,
                    &mut texture,
                    ImageData::Color(patch),
                    [0, 0],
                )?;
                let Texture::Managed(managed) = &texture else {
                    unreachable!()
                };
                assert_eq!(
                    unique,
                    Arc::as_ptr(&managed.image),
                    "unique backing edits in place"
                );
                expected[..width].fill(Color32::BLUE);
                assert_eq!(readback(&device, &context, &managed.tex)?, expected);
            }
        }
        Ok(())
    }

    #[test]
    fn managed_texture_bounds_and_backing_lifetime_are_explicit() -> Result<()> {
        let Some((device, context)) = device(D3D_DRIVER_TYPE_WARP)? else {
            unreachable!()
        };
        let make = || Arc::new(egui::ColorImage::filled([3, 3], Color32::WHITE));
        let original = make();
        let weak = Arc::downgrade(&original);
        let id = TextureId::Managed(9);
        let mut pool = TexturePool::new(&device);
        pool.update(
            &context,
            TexturesDelta {
                set: vec![(
                    id,
                    egui::epaint::ImageDelta::full(original.clone(), egui::TextureOptions::LINEAR),
                )],
                ..Default::default()
            },
        )?;
        assert_eq!(
            Arc::strong_count(&original),
            2,
            "renderer retains one shared backing"
        );
        drop(original);
        let texture = pool.pool.get_mut(&id).unwrap();
        let mut malformed = (*make()).clone();
        malformed.pixels.pop();
        assert!(
            TexturePool::update_partial(
                &context,
                texture,
                ImageData::Color(Arc::new(malformed.clone())),
                [0, 0]
            )
            .is_err()
        );
        assert!(
            TexturePool::create_managed_texture(
                &device,
                ImageData::Color(Arc::new(malformed)),
                egui::TextureOptions::LINEAR
            )
            .is_err()
        );
        for pos in [[1, 0], [0, 1], [usize::MAX, 0], [0, usize::MAX]] {
            assert!(
                TexturePool::update_partial(&context, texture, ImageData::Color(make()), pos)
                    .is_err()
            );
        }
        let Texture::Managed(managed) = texture else {
            unreachable!()
        };
        assert_eq!(
            readback(&device, &context, &managed.tex)?,
            vec![Color32::WHITE; 9]
        );
        for size in [[0, 0], [usize::MAX, 2], [1, usize::MAX]] {
            let mut invalid = egui::ColorImage::filled([1, 1], Color32::BLACK);
            invalid.size = size;
            assert!(
                TexturePool::create_managed_texture(
                    &device,
                    ImageData::Color(Arc::new(invalid)),
                    egui::TextureOptions::LINEAR
                )
                .is_err()
            );
        }
        pool.update(
            &context,
            TexturesDelta {
                free: vec![id],
                ..Default::default()
            },
        )?;
        assert!(
            weak.upgrade().is_none(),
            "free releases the shared CPU backing"
        );
        assert!(pool.get_srv(id).is_none());
        Ok(())
    }
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

        let ImageData::Color(patch) = image;
        let width = old.image.width();
        if patch.width().checked_mul(patch.height()) != Some(patch.pixels.len())
            || nx.checked_add(patch.width()).is_none_or(|end| end > width)
            || ny
                .checked_add(patch.height())
                .is_none_or(|end| end > old.image.height())
        {
            return Err(E_INVALIDARG.into());
        }
        if patch.width() == 0 || patch.height() == 0 {
            return Ok(());
        }
        // Whole uploads retain egui's shared immutable pixels. Only partial edits
        // detach the backing image if another owner still references it.
        let pixels = &mut Arc::make_mut(&mut old.image).pixels;
        for y in 0..patch.height() {
            let start = (ny + y) * width + nx;
            let source = y * patch.width();
            pixels[start..start + patch.width()]
                .copy_from_slice(&patch.pixels[source..source + patch.width()]);
        }

        // The caller exclusively owns this managed texture and immediate context.
        // WRITE_DISCARD requires restoring every row; the mapped row pitch can
        // exceed the packed CPU width. No mapped pointer survives Unmap.
        let subr = unsafe {
            let mut output = D3D11_MAPPED_SUBRESOURCE::default();
            ctx.Map(&old.tex, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut output))?;
            output
        };
        for (y, row) in pixels.chunks_exact(width).enumerate() {
            unsafe {
                subr.pData
                    .cast::<u8>()
                    .add(y * subr.RowPitch as usize)
                    .copy_from_nonoverlapping(row.as_ptr().cast(), mem::size_of_val(row));
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
        let ImageData::Color(image) = data;
        let width = image.width();
        let height = image.height();
        let max_side = D3D11_REQ_TEXTURE2D_U_OR_V_DIMENSION as usize;
        if width == 0
            || height == 0
            || width > max_side
            || height > max_side
            || width.checked_mul(height) != Some(image.pixels.len())
        {
            return Err(E_INVALIDARG.into());
        }

        let desc = D3D11_TEXTURE2D_DESC {
            Width: width as _,
            Height: height as _,
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
            pSysMem: image.pixels.as_ptr() as _,
            SysMemPitch: (width * mem::size_of::<Color32>()) as u32,
            SysMemSlicePitch: 0,
        };

        let mut tex = None;
        // The immutable, validated CPU image remains alive for this synchronous
        // device upload and is retained for later partial updates. D3D keeps no
        // pointer to its storage after CreateTexture2D returns.
        unsafe { device.CreateTexture2D(&desc, Some(&subresource_data), Some(&mut tex)) }?;
        let tex = tex.unwrap();

        let mut srv = None;
        unsafe { device.CreateShaderResourceView(&tex, None, Some(&mut srv)) }?;
        let srv = srv.unwrap();

        Ok(Texture::Managed(ManagedTexture {
            tex,
            srv,
            image,
            options,
        }))
    }
}
