# Reading button artwork

The owner supplied these three replacement SVGs in the follow-up plan on 2026-09-18: an outline book, an outline book with an inset right chevron for entry, and a filled book with a cutout right chevron for active reading. These replace the previous pfp_cropper/Tabler assets. Left reading mirrors the corresponding right-facing artwork, including the active filled state.

The SVGs are source references, not runtime files. `src/chrome/reading_icon.rs` transcribes their cubic paths into the already pinned tiny-skia rasterizer. Normalize all sources into a 24-unit square with centered, matching optical widths; the outline uses a 1.5-unit round stroke, equivalent to one logical pixel at the existing 16-point glyph size. The filled source scales by 21.5/22 to match the outline bounds. Retain the center gutter and chevron cutout.

Rasterize at the current physical size and align texels to physical pixels. Cache one texture per control, replacing it on state/direction/DPI changes; enabled/disabled tint reuses those pixels. The 24-point hit target remains unchanged. No SVG parser, additional dependency or runtime asset lookup is required.
