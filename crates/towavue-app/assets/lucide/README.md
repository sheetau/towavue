# Lucide artwork

Unmodified upstream SVGs and the complete [Lucide license](LICENSE), pinned by
[upstream.json](upstream.json) to commit `ba6751ac45d379f6359d75ef2228cff6ba96c122` of
[Lucide](https://github.com/lucide-icons/lucide/tree/ba6751ac45d379f6359d75ef2228cff6ba96c122/icons).

`play`, `pause`, `skip-back`, `skip-forward`, `repeat`, `repeat-1` and `shuffle`
replace the unspecified custom transport/audio-mode artwork. Other Codicons
remain. The explicitly requested towavue logo, reading artwork, Monapad/Tabler
drag badge and Chromium cursor assets remain separate.

Run `python scripts/generate-lucide-paths.py` to regenerate the Rust paths or
append `--check` to verify them offline. The generator verifies upstream hashes,
converts SVG arcs/rounded rectangles to cubic curves, and rejects unsupported
geometry. Arc conversion follows the [SVG endpoint-to-center rules](https://www.w3.org/TR/SVG/implnote.html#ArcImplementationNotes). The SVGs are the editable source of truth; do not hand-edit generated
coordinates. Runtime parsing and new dependencies are unnecessary.

UI glyph boxes are 16 logical points with a one-logical-point round stroke.
Closed transport subpaths also receive a white fill; open skip bars and all
repeat/shuffle strokes stay open. Taskbar transport uses the same paths in its
32-point box with a one-point stroke and its existing black contrast edge.
CPU rasterization uses pinned tiny-skia, caches by glyph/pixel size, and places
texels on physical pixel boundaries to avoid a second fractional resampling.

Keep the SVGs, this provenance and the complete license with future application
source/license materials; historical candidate manifests still describe their
original executables and are not silently repinned by this UI change.
