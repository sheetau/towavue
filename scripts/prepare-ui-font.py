"""Freeze tnum digits for egui's shaping path without configurable features."""

import argparse
import hashlib
from io import BytesIO
from pathlib import Path

import fontTools
from fontTools.ttLib import TTFont


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if fontTools.__version__ != "4.59.2":
        raise RuntimeError("Use fonttools==4.59.2 for reproducible font output")
    directory = Path(__file__).resolve().parents[1] / "crates/towavue-app/assets/fonts"
    if hashlib.sha256((directory / "codicon.ttf").read_bytes()).hexdigest() != "9d25513c861704be650eacef8c4588aceabdc8b668857747b5e42b299e926918":
        raise RuntimeError("Unmodified Monaco Codicon hash changed")
    source = directory / "source/Figtree-Regular.ttf"
    if hashlib.sha256(source.read_bytes()).hexdigest() != "9acc05654630d37003d6368c7bb33e3cc57b5dd3d9f9b4a753891016527112cf":
        raise RuntimeError("Figtree source hash changed")
    font = TTFont(source, recalcTimestamp=False)
    substitutions = {}
    for feature in font["GSUB"].table.FeatureList.FeatureRecord:
        if feature.FeatureTag == "tnum":
            for index in feature.Feature.LookupListIndex:
                lookup = font["GSUB"].table.LookupList.Lookup[index]
                if lookup.LookupType != 1:
                    raise RuntimeError("Expected single-glyph tnum substitutions")
                for subtable in lookup.SubTable:
                    substitutions.update(subtable.mapping)
    original = font.getBestCmap()
    digits = {ord(char): substitutions[original[ord(char)]] for char in "0123456789"}
    advances = {font["hmtx"][glyph][0] for glyph in digits.values()}
    if len(advances) != 1:
        raise RuntimeError("Tabular digits must have one advance width")
    for table in font["cmap"].tables:
        if table.isUnicode():
            for codepoint, glyph in digits.items():
                if codepoint in table.cmap:
                    table.cmap[codepoint] = glyph
    license_text = (directory / "OFL-Figtree.txt").read_text(encoding="utf-8")
    names = {
        0: license_text.splitlines()[0],
        1: "Towavue Figtree Tabular",
        2: "Regular",
        3: "TowavueFigtreeTabular-Regular-1.000",
        4: "Towavue Figtree Tabular Regular",
        6: "TowavueFigtreeTabular-Regular",
        13: license_text,
        14: "https://openfontlicense.org/",
        16: "Towavue Figtree Tabular",
        17: "Regular",
    }
    for name_id, value in names.items():
        font["name"].removeNames(nameID=name_id)
        font["name"].setName(value, name_id, 3, 1, 0x409)
    output = BytesIO()
    font.save(output)
    data = output.getvalue()
    destination = directory / "Figtree-Tabular.ttf"
    if args.check:
        if destination.read_bytes() != data:
            raise RuntimeError("Bundled font differs from reproducible output")
    else:
        destination.write_bytes(data)
    print(f"Figtree tnum: advance={advances.pop()}, bytes={len(data)}, sha256={hashlib.sha256(data).hexdigest()}")


if __name__ == "__main__":
    main()
