# towavue renderer patch

Based on the crates.io package `egui-directx11 0.13.0+egui-0.35.0` by Nekomaru,
upstream https://github.com/Nekomaru-PKU/egui-directx11.
Original crate SHA-256: `e6a92d7edaf05aefc0ed9a0ecba4765035fa5eb14061ea242e406c624e5289aa`.
The original MIT and Apache-2.0 license texts and texture-source attribution are retained.

Local changes:

- Add a nondefault `render-verification` feature exposing managed texture IDs and
  dimensions for runtime tests. It returns no native references and does not change
  texture lifetime or production rendering.

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
  Premultiplied white geometry uses inverse-destination RGB blending and preserves
  destination alpha. Vertex alpha interpolates from the original to inverted RGB:
  `a * (1 - destination) + (1 - a) * destination`. Opaque image outlines remain
  identical; alpha51 adds white20% difference fills for time selection. Every
  subsequent normal mesh restores ordinary blending.
  Texture sampling, the existing UI pipeline and scissor clipping remain shared;
  arbitrary GPU callbacks are still unsupported. No native handles are carried
  in the payload.
- Verify atlas-white 0/20/100% inversion, overlapping inversion, clipping, alpha and
  following ordinary meshes by exact readback on WARP and an owned offscreen
  hardware device. Hardware creation failure reports an explicit skip; shader,
  draw and pixel failures after creation fail the test.
- Use DEFAULT managed textures without a retained CPU pixel shadow. Whole
  uploads borrow the incoming image only through synchronous creation; partial
  updates upload a validated rectangle with its packed source row pitch through
  [UpdateSubresource](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11devicecontext-updatesubresource).
  The renderer retains only GPU resources, dimensions and sampling options.
  Partial updates require the serialized same-device immediate context used by
  towavue; deferred contexts fail explicitly because offset source handling has
  a documented driver-dependent caveat. No full-image copy-on-write or discarded
  mapping is needed. Driver upload/storage allocations are not a process cap.
- Validate image lengths, dimensions and partial bounds before GPU access.
  Offscreen WARP/hardware readback covers seven row widths, repeated partial
  updates, unchanged external snapshots and released source allocations.
  WARP also covers empty updates, invalid input, deferred-context rejection,
  GPU use after source release and texture removal.

The public marker is additive; native device ownership APIs are unchanged.
Upstream vertex/index buffer upload and normal blending remain unchanged.
The existing context-zoom workaround
in towavue's runtime wrapper is still required.

Run `cargo test -p egui-directx11 --lib --locked --offline` from the repository root.
Format with `cargo fmt --manifest-path vendor/egui-directx11/Cargo.toml -- --check`
and lint with `cargo clippy -p egui-directx11 --all-targets --locked --offline -- -D warnings`.
Offscreen validation does not qualify interactive window or mixed-DPI behavior.
