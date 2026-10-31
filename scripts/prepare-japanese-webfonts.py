"""Create small page subsets and keep full-coverage fallbacks for Japanese fonts.

Run after Japanese copy changes for the smallest first load. Existing full fonts
cover new characters even before regeneration. Requires fonttools[woff], requests.
"""
import json
from pathlib import Path

import requests
from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / "public/media/fonts"
CACHE = ROOT / "output/font-sources"
CACHE.mkdir(parents=True, exist_ok=True)
SOURCE = "https://raw.githubusercontent.com/google/fonts/main/ofl/mplus1code/"
original = CACHE / "MPLUS1Code.ttf"
if not original.exists():
    response = requests.get(SOURCE + "MPLUS1Code%5Bwght%5D.ttf", timeout=90)
    response.raise_for_status()
    original.write_bytes(response.content)
license_path = DEST / "m-plus-1-code-OFL.txt"
if not license_path.exists():
    response = requests.get(SOURCE + "OFL.txt", timeout=30)
    response.raise_for_status()
    license_path.write_bytes(response.content)

mplus_full = DEST / "m-plus-1-code-full.woff2"
if not mplus_full.exists():
    font = instantiateVariableFont(TTFont(original), {"wght": 400}, inplace=True)
    font.flavor = "woff2"
    font.save(mplus_full)

sources = [ROOT / "src/site/ja.json", ROOT / "src/site/locales.js", ROOT / "src/site/gallery.json"]
sources += list((ROOT / "src/components").glob("*.jsx"))
sources += list((ROOT / "pages").rglob("*.jsx"))
characters = set("".join(path.read_text(encoding="utf-8") for path in sources))
characters.update(chr(code) for code in range(0x20, 0x7F))
characters.update(chr(code) for code in range(0x3000, 0x3100))


def ranges(codes):
    spans = []
    for code in sorted(codes):
        if spans and code == spans[-1][1] + 1:
            spans[-1][1] = code
        else:
            spans.append([code, code])
    return spans


def css_ranges(spans):
    return ",".join(f"U+{start:X}" if start == end else f"U+{start:X}-{end:X}" for start, end in spans)


manifest = {}
for key, stem in [("narrow", "towavue-jp-narrow"), ("sans", "m-plus-1-code")]:
    full = DEST / f"{stem}-full.woff2"
    font = TTFont(full, recalcTimestamp=False)
    supported = set(font.getBestCmap())
    codes = sorted({ord(char) for char in characters} & supported)
    options = subset.Options()
    options.layout_features = ["*"]
    options.name_IDs = [0, 1, 2, 3, 4, 5, 6, 16, 17]
    options.recalc_timestamp = False
    subsetter = subset.Subsetter(options=options)
    subsetter.populate(unicodes=codes)
    subsetter.subset(font)
    font.flavor = "woff2"
    destination = DEST / f"{stem}-page.woff2"
    font.save(destination)
    # Disjoint unicode ranges prevent loading the 3MB full font for page glyphs.
    spans = ranges(codes)
    # Use compact complementary ranges instead of listing thousands of rare
    # kanji separately. Control characters never need a downloadable face.
    complement = []
    start = 0xA0
    for first, last in spans:
        if first > start:
            complement.append([start, first - 1])
        start = max(start, last + 1)
    if start <= 0x10FFFF:
        complement.append([start, 0x10FFFF])
    manifest[key] = {
        "page": f"fonts/{destination.name}",
        "full": f"fonts/{full.name}",
        "pageRange": css_ranges(spans),
        "fullRange": css_ranges(complement),
        "sample": "日本語",
    }
    print(f"{key}: {len(codes)} characters; {full.stat().st_size:,} -> {destination.stat().st_size:,} bytes")

(ROOT / "src/site/japanese-fonts.json").write_text(
    json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
)
