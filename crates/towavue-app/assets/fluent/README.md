# Fluent System Icons

Unmodified Microsoft Fluent System Icons SVGs from commit
[`883bef1e12d37343d0b2a64a1af43d0209de1128`](https://github.com/microsoft/fluentui-system-icons/tree/883bef1e12d37343d0b2a64a1af43d0209de1128)
are bundled with the upstream [MIT license](LICENSE) and [NOTICE](NOTICE).
`upstream.json` records each original path and SHA-256, including both notices.

The 16-unit filled Play/Pause/Previous/Next artwork serves both UI transport
and taskbar buttons. Regular Arrow Repeat All/Arrow Repeat 1/Arrow Shuffle,
Arrow Repeat All Off and Arrow Shuffle Off serve playback modes.
Image/Filmstrip Play/Music Note 2 serve Gallery filters;
Arrow Right/Window New/Prohibited serve native drag badges. Compound filled outlines
are rasterized with their original winding and without adding a stroke.
The UI uses a 16-point box, taskbar buttons 32 points and badge symbols 17 points.
The existing color tints, taskbar contrast edge and badge decoration are retained.
Play's upstream optical alignment is translated 1.5 source units left at rasterization
to center its silhouette in our compact buttons, consistently across UI and taskbar
sizes. Source SVGs and generated paths remain unchanged (upstream discussion:
[Play alignment](https://github.com/microsoft/fluentui-system-icons/issues/137)).
Custom logo and reading artwork, font-based Codicons and Chromium cursors remain.

Run `python scripts/generate-fluent-paths.py` to regenerate the Rust paths, or
add `--check` to verify all source hashes and generated output. This offline
standard-library tool adds no product dependency or build-time network access.
