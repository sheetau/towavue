# Chromium media cursors

These two 32×32 cursor images are derived from Chromium revision
`4c84a0e082c7d97af376c636329ad71ca16acee3` and retain its [BSD-style license](LICENSE-Chromium.txt).
Ship this license with any binary containing these images.

| Image | Original resource | Source SHA-256 | Hotspot | DIB entry |
|---|---|---|---|---|
| zoom_in.rgba | [zoom_in.cur](https://chromium.googlesource.com/chromium/src/+/4c84a0e082c7d97af376c636329ad71ca16acee3/ui/resources/cursors/zoom_in.cur) | `eb69f540be1e416b7346017da48deaf5ba2f2ee0af366c04f1e374351b651872` | 6, 6 | 1-bit |
| hand_grabbing.rgba | [hand_grabbing.cur](https://chromium.googlesource.com/chromium/src/+/4c84a0e082c7d97af376c636329ad71ca16acee3/ui/resources/cursors/hand_grabbing.cur) | `22e823e71c106f338d42932c13c16e05a8310b3bdec18a89cc5ca197408cf11a` | 13, 13 | 24-bit |

Each whitespace-separated word is one straight-alpha RRGGBBAA pixel, in
top-to-bottom row order. Conversion reverses the source DIB's bottom-up rows,
expands the black/white palette or reorders 24-bit BGR, and maps the AND mask
to alpha (1 → 0, 0 → 255). The monochrome source has no XOR-inversion pixels.
No artwork is redrawn; RGB under transparent pixels is retained. The app
uses nearest-neighbor scaling and proportionally scaled hotspots for window
DPI, and keeps the native cursor objects until that DPI changes or the window
closes. Text encoding avoids adding a runtime image decoder for two assets.

Only ZoomIn and Grabbing use these images. Tab/filmstrip Move/NoDrop and
native caption cursors remain unchanged. Source/provenance verification is
separate from physical-input or native cursor appearance verification.
