# Bundled UI fonts

These assets implement the owner's explicit Figtree/Codicon/Seti UI requests. They are embedded in the app, not installed into Windows. Japanese UI fonts are read from Windows and are not copied here.

## Figtree

- Author: The Figtree Project Authors; project by Erik Kennedy.
- Project: https://github.com/erikdkennedy/figtree
- License: [SIL Open Font License 1.1](OFL-Figtree.txt), copyright 2022 The Figtree Project Authors.
- Original: `source/Figtree-Regular.ttf`, copied unchanged from the owner's monapad `src/font/Figtree-Regular.ttf`, version `1.000; ttfautohint (v1.8.4.7-5d5b)`.
- Original SHA-256: `9acc05654630d37003d6368c7bb33e3cc57b5dd3d9f9b4a753891016527112cf`.
- Embedded derivative: `Figtree-Tabular.ttf`, SHA-256 `8c9ea38b7b414dc3a2884343671f4e9c4a92a015541a00ad30fd60a3a2d18ed3`.

The derivative maps U+0030..U+0039 to the original font's `tnum` glyphs, all with advance 623 font units. Other character mappings, outlines and metrics are preserved. Its internal family is `Towavue Figtree Tabular`; copyright/license metadata is included. No endorsement by the original authors is implied. egui 0.35.0 does shape text, but its pinned shaping call supplies an empty feature list; it does not expose a per-font `tnum` setting. This is not a replacement monospaced font.

The upstream license was retrieved at commit `032dfa7fe219ef3a02890d6d3add84eacc9aebfe`. The local monapad font is an older file; it is not claimed to match that commit's current TTF.

Rebuild or verify from the retained original using the pinned, development-only tool:

```powershell
python -m pip install --target target/font-tools "fonttools==4.59.2"
$env:PYTHONPATH = (Resolve-Path target/font-tools).Path
python scripts/prepare-ui-font.py
python scripts/prepare-ui-font.py --check
```

Run from the repository root. Ordinary Cargo builds need no Python, font tooling, network access or external font download.

## Codicon

- Author: Microsoft Corporation and contributors.
- Project: https://github.com/microsoft/vscode-codicons
- Original: unmodified `dist/codicon.ttf` from `@vscode/codicons` **0.0.46-16**, the exact version in the owner's reference VS Code checkout's `package-lock.json`. That checkout copies the npm font during compilation and does not track the TTF itself.
- Package: https://registry.npmjs.org/@vscode/codicons/-/codicons-0.0.46-16.tgz
- Package integrity: `sha512-pjgpzU5fMIuI3atUdpbWiaUxZtpTak57vjp1hIEvmmmd1dSOTB++5DoKeQCtRLapbB9DU24EHk31njP2ayPvmA==`.
- Font SHA-256: `841d1c28fd2de5af86d42894cace161203525c0a903e6615a1fe2d354839e676` (139084 bytes).
- The font has 596 glyphs and 604 mapped codepoints, replacing the former Monaco-derived font's 516 glyphs and 525 mappings. Both include Gallery's `clear-all` U+EABF and `filter` U+EAF1; the replacement follows the owner's request to prefer the reference's larger inventory. Removed legacy mappings are unused by the app. The package's license is byte-identical to the retained Codicons notice.
- Codepoint names are verified against the pinned Codicons package's `dist/codicon.css`; the app's existing mappings retain their VS Code names. Gallery's video/audio filters use `device-camera-video` U+EAD9 and `music` U+EC1B from that same package; its image filter uses the separately attributed Seti face below.
- The tab's unsaved marker uses `close-dirty` / `circle-filled`, U+EA71, from the [matching VS Code mapping](https://github.com/microsoft/vscode/blob/86f5a62f058e3905f74a9fa65d04b2f3b533408e/src/vs/base/common/codiconsLibrary.ts). The existing U+EA76 close glyph replaces it on hover or keyboard focus.
- Retain [Monaco's MIT notice](LICENSE-Monaco.txt) and the upstream [Codicons artwork license](LICENSE-Codicons.txt), CC BY 4.0, including its Git logo attribution and CC BY 3.0 link. The Codicons license was retrieved at commit `1c47ab36a4bb845c437866405c2fa67b8ca0fe36`.

The app selects a named Codicon family only for icon widgets. It does not add Codicon as a general text fallback or modify the font. The reading button uses its separately attributed reference/Tabler artwork; see [reading icon provenance](../reading-icons/README.md). Play/pause use solid vector counterparts to the bundled Codicon transport outlines in `src/chrome.rs`, with the same attribution; no separate transport font is bundled. The logo remains the owner's custom vector. Caption controls now use native Windows drawing.

Keep these notices, provenance and Figtree modification statement with any future package containing these assets. The older qualified local Setup and its material bindings have not been rebuilt or approved for this newer executable.

Taskbar thumbnail buttons rasterize the bundled `chevron-left` (U+EAB5), `play` (U+EB2C), `debug-pause` (U+EAD1), and `chevron-right` (U+EAB6) glyphs at the window's pixel density. Their white fills receive a black contrast outline for light/dark Shell backgrounds. This is another derivative under the same Codicons attribution and CC BY 4.0 notice; no new font or installed asset is used.

## Seti image icon

- Author: Jesse Weed; [Seti UI](https://github.com/jesseweed/seti-ui).
- License: [MIT](LICENSE-Seti.txt), copyright 2014 Jesse Weed, retained from the reference VS Code extension's third-party notice.
- Source: unmodified `source/seti.woff`, from `extensions/theme-seti/icons/seti.woff` in the owner's reference VS Code checkout at `682c0f7de831c5f8616e854c28ba1bd15705441a`.
- WOFF SHA-256: `b127762058f89b37b08d76185e3b0558f5c28859e39a3840a27cb28b9739d6e2` (37284 bytes).
- Converted TTF SHA-256: `da651d2230847582a35748dc9fdfc2e0fd4caa7bd52e80eeac2e5627b32056e3` (54476 bytes).
- The extension's `vs-seti-icon-theme.json` maps both `_image` and `_image_light` to U+E04C. Its `cgmanifest.json` identifies Seti UI commit `2d6c5e68b4ded73c92dac291845ee44e1182d511`.

`seti.ttf` is a container conversion for egui. Glyph mappings, outlines, metrics and names are retained. The app uses the isolated `seti` family only for the Gallery image filter, at 24 points with its painted bounds centered in the 22-point button, matching the adjacent 16-point Codicon artwork without enlarging the button. Seti is not a general text fallback and does not change the other icon families.

Regenerate with `python scripts/prepare-seti-font.py`, or verify with `--check`, using the same pinned FontTools environment described above. Ordinary Cargo builds embed the converted TTF and need no conversion tool or additional product dependency.
