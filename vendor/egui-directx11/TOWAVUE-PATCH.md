# towavue renderer patch

Based on the crates.io package `egui-directx11 0.13.0+egui-0.35.0` by Nekomaru,
upstream https://github.com/Nekomaru-PKU/egui-directx11.
Original crate SHA-256: `e6a92d7edaf05aefc0ed9a0ecba4765035fa5eb14061ea242e406c624e5289aa`.
The original MIT and Apache-2.0 license texts and texture-source attribution are retained.

Local changes:

- Retain managed texture sampling options across full and partial updates, and
  select a cached D3D11 sampler per draw (minification, magnification, wrapping).
  User textures default to linear/clamp. Mipmaps are not generated; the original
  shader samples level zero.
- Compile the original HLSL through D3DCompile when constructing the renderer.
  This source-only copy contains no precompiled shader binaries or example media.
- Pin the existing resolved dependencies; omit upstream examples/dev dependencies.
- Add an offscreen WARP test comparing mixed linear/nearest output pixels and
  switching a texture's sampler through a partial update.
- Recognize the plain `InvertMesh` paint-callback payload in tessellation order.
  Opaque white geometry uses inverse-destination RGB blending and preserves
  destination alpha. Every subsequent normal mesh restores ordinary blending.
  Texture sampling, the existing UI pipeline and scissor clipping remain shared;
  arbitrary GPU callbacks are still unsupported. No native handles are carried
  in the payload.
- Verify atlas-white inversion, overlapping inversion, clipping, alpha and
  following ordinary meshes by exact readback on WARP and an owned offscreen
  hardware device. Hardware creation failure reports an explicit skip; shader,
  draw and pixel failures after creation fail the test.

The public marker is additive; native device ownership APIs are unchanged.
Upstream buffer/texture upload and normal blending otherwise remain unchanged.
The existing context-zoom workaround
in towavue's runtime wrapper is still required.

Run `cargo test -p egui-directx11 --lib --locked --offline` from the repository root.
Offscreen validation does not qualify interactive window or mixed-DPI behavior.
