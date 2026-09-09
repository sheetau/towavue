# Bundled UI fonts

These assets implement the owner's explicit Figtree/Codicon UI request. They are embedded in the app, not installed into Windows. Japanese UI fonts are read from Windows and are not copied here.

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
- Original: `codicon.ttf`, copied unchanged from monapad's Monaco Editor 0.55.1, `esm/vs/base/browser/ui/codicons/codicon/codicon.ttf`.
- Monaco's recorded VS Code source revision: `86f5a62f058e3905f74a9fa65d04b2f3b533408e`.
- SHA-256: `9d25513c861704be650eacef8c4588aceabdc8b668857747b5e42b299e926918` (121972 bytes).
- Codepoint names come from that same Monaco package's `codiconsLibrary.js`.
- Retain [Monaco's MIT notice](LICENSE-Monaco.txt) and the upstream [Codicons artwork license](LICENSE-Codicons.txt), CC BY 4.0, including its Git logo attribution and CC BY 3.0 link. The Codicons license was retrieved at commit `1c47ab36a4bb845c437866405c2fa67b8ca0fe36`.

The app selects a named Codicon family only for icon widgets. It does not add Codicon as a general text fallback or modify the font. The reading button uses book-derived vector page contours in `chrome.rs`, adapted from the Codicon book outline into matching outlined/filled states; preserve the same attribution and CC BY 4.0 notice for this derivative. The reference is `src/icons/book.svg` at the Codicons commit above. The logo remains the owner's custom vector. Caption controls now use native Windows drawing.

Keep these notices, provenance and Figtree modification statement with any future package containing these assets. The older qualified local Setup and its material bindings have not been rebuilt or approved for this newer executable.
