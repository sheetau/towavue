"""Build WebP images and public credits from the ordered gallery catalogue."""
from concurrent.futures import ThreadPoolExecutor
from io import BytesIO
from pathlib import Path
import json

import requests
from PIL import Image, ImageOps

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / "public/media/gallery"
CACHE = ROOT / "output/archive-sources"
MANIFEST = ROOT / "src/site/gallery.json"


def prepare(work):
    original = CACHE / (work["id"] + ".original")
    if not original.exists():
        response = requests.get(work["imageUrl"], timeout=120, headers={
            "User-Agent": "TowavueGallery/1.0 (https://github.com/sheetau/towavue; open access images)"
        })
        response.raise_for_status()
        with Image.open(BytesIO(response.content)) as source:
            source.verify()
        original.write_bytes(response.content)

    with Image.open(original) as source:
        image = ImageOps.exif_transpose(source)
        changes = "Proportionally resized and converted to WebP; no crop or colour changes."
        if image.mode == "P" and "transparency" in image.info:
            image = image.convert("RGBA")
        if "A" in image.getbands() and image.getchannel("A").getextrema()[0] < 255:
            # The source diagrams use black lines on a transparent page.
            rgba = image.convert("RGBA")
            image = Image.new("RGB", rgba.size, "white")
            image.paste(rgba, mask=rgba.getchannel("A"))
            changes = "Rendered on white, proportionally resized and converted to WebP."
        else:
            image = image.convert("RGB")
        width, height = image.size
        image.thumbnail((800, 800))
        filename = work["id"] + ".webp"
        image.save(DEST / filename, quality=88, method=6)

    return {**work, "src": f"gallery/{filename}", "name": filename,
            "width": width, "height": height, "changes": changes}


def write_json(path, data):
    newline = "\r\n" if path.exists() and b"\r\n" in path.read_bytes() else "\n"
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n",
                    encoding="utf-8", newline=newline)


if __name__ == "__main__":
    CACHE.mkdir(parents=True, exist_ok=True)
    DEST.mkdir(parents=True, exist_ok=True)
    catalogue = json.loads(MANIFEST.read_text(encoding="utf-8"))
    with ThreadPoolExecutor(max_workers=4) as pool:
        catalogue["photos"] = list(pool.map(prepare, catalogue["photos"]))
    write_json(MANIFEST, catalogue)
    write_json(DEST / "credits.json", catalogue["photos"])
    print(f"Prepared {len(catalogue['photos'])} archive images with source and licence records.")
