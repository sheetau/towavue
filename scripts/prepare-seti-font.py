"""Convert the pinned Seti WOFF to an equivalent TTF for the embedded icon face."""

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
    source = directory / "source/seti.woff"
    if hashlib.sha256(source.read_bytes()).hexdigest() != "b127762058f89b37b08d76185e3b0558f5c28859e39a3840a27cb28b9739d6e2":
        raise RuntimeError("Seti source hash changed")
    font = TTFont(source, recalcTimestamp=False)
    if 0xE04C not in font.getBestCmap():
        raise RuntimeError("Seti image glyph is missing")
    font.flavor = None
    output = BytesIO()
    font.save(output)
    data = output.getvalue()
    destination = directory / "seti.ttf"
    if args.check:
        if destination.read_bytes() != data:
            raise RuntimeError("Bundled Seti font differs from reproducible output")
    else:
        destination.write_bytes(data)
    print(f"Seti: bytes={len(data)}, sha256={hashlib.sha256(data).hexdigest()}")


if __name__ == "__main__":
    main()
