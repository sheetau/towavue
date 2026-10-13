"""Build a full-coverage, horizontally condensed Noto Serif JP Regular.

Requires fonttools[woff], requests. All original character mappings, variation
sequences, and substitution glyphs are retained; copy changes need no rebuild.
Atelier Femme Serif remains the first CSS font, preserving English typography.
"""
from pathlib import Path
import requests
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib.ttVisitor import TTVisitor
from fontTools.ttLib.tables import otTables
from fontTools.misc.fixedTools import otRound

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / "public/media/fonts"
CACHE = ROOT / "output/font-sources"
CACHE.mkdir(parents=True, exist_ok=True)
SOURCE = "https://raw.githubusercontent.com/google/fonts/main/ofl/notoserifjp/"
original = CACHE / "NotoSerifJP.ttf"
if not original.exists():
    response = requests.get(SOURCE + "NotoSerifJP%5Bwght%5D.ttf", timeout=90)
    response.raise_for_status()
    original.write_bytes(response.content)
response = requests.get(SOURCE + "OFL.txt", timeout=30)
response.raise_for_status()
(DEST / "towavue-jp-narrow-OFL.txt").write_bytes(response.content)

font = instantiateVariableFont(TTFont(original), {"wght": 400}, inplace=True)
# Keep every cmap, including format 14 (ideographic variation sequences).
original_cmap = font.getBestCmap().copy()
original_uvs = {t.platformID: t.uvsDict.copy() for t in font["cmap"].tables if t.format == 14}
original_order = font.getGlyphOrder()[:]
original_metrics = font["hmtx"].metrics.copy()
glyph_set = font.getGlyphSet()
glyphs = {}
for name in font.getGlyphOrder():
    pen = TTGlyphPen(glyph_set)
    glyph_set[name].draw(TransformPen(pen, (0.7, 0, 0, 1, 0, 0)))
    glyphs[name] = pen.glyph()
for name, glyph in glyphs.items():
    font["glyf"][name] = glyph
    advance, bearing = font["hmtx"][name]
    font["hmtx"][name] = (otRound(advance * 0.7), otRound(bearing * 0.7))
# Retain kerning and mark positioning, scaling only their horizontal values.
class HorizontalPositionVisitor(TTVisitor):
    pass

@HorizontalPositionVisitor.register_attrs(((otTables.ValueRecord, ("XAdvance", "XPlacement")),
                                          (otTables.Anchor, ("XCoordinate",))))
def visit(visitor, obj, attr, value):
    setattr(obj, attr, otRound(value * 0.7))

HorizontalPositionVisitor().visit(font["GPOS"])
font["hhea"].advanceWidthMax = max(value[0] for value in font["hmtx"].metrics.values())
font["OS/2"].xAvgCharWidth = round(font["OS/2"].xAvgCharWidth * 0.7)
font["OS/2"].usWidthClass = 3
names = {1: "Towavue JP Narrow", 2: "Regular", 3: "TowavueJPNarrow-Regular-2.0",
         4: "Towavue JP Narrow Regular", 6: "TowavueJPNarrow-Regular",
         16: "Towavue JP Narrow", 17: "Regular"}
for name_id, value in names.items():
    font["name"].removeNames(nameID=name_id)
    font["name"].setName(value, name_id, 3, 1, 0x409)
font.flavor = "woff2"
destination = DEST / "towavue-jp-narrow-full.woff2"
font.save(destination)
# Verify the encoded file, not just the in-memory font.
encoded = TTFont(destination)
assert encoded.getBestCmap() == original_cmap, "A supported character was lost"
assert encoded.getGlyphOrder() == original_order, "A glyph or alternate was lost"
assert {t.platformID: t.uvsDict for t in encoded["cmap"].tables if t.format == 14} == original_uvs
assert "GSUB" in encoded and "GPOS" in encoded
assert all(encoded["hmtx"][name][0] == otRound(metrics[0] * 0.7)
           for name, metrics in original_metrics.items())
print(f"Prepared {len(original_cmap)} characters / {len(original_order)} glyphs; "
      f"all mappings and variation sequences preserved. {destination.stat().st_size:,} bytes.")
