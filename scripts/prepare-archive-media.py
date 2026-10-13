"""Download the selected licensed works and create faithful web thumbnails.

No generated images, colour changes, or cropping. Original downloads are cached
under output/archive-sources. Credits are shipped alongside the public images.
"""
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
import json
import requests
from PIL import Image, ImageOps

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / "public/media/gallery"
CACHE = ROOT / "output/archive-sources"
CACHE.mkdir(parents=True, exist_ok=True)
DEST.mkdir(parents=True, exist_ok=True)
chladni_files = [
    "35835910874_1b7b4201e7_o.png", "36501469562_d783356518_o.png",
    "35835909884_635cb51847_o.png", "36501467992_8b8f1d8500_o.jpg",
    "35861584123_ea3acc281f_o.jpg", "36501467482_a8e3bd781b_o.jpg",
    "35861583583_e06ab8bd83_o.jpg", "35835908874_5251f8d8f8_o.jpg",
    "35861583333_d4bf180a28_o.jpg",
]
works = []
for index, filename in enumerate(chladni_files, 1):
    if index not in {1, 4, 9}:
        continue
    works.append(dict(id=f"chladni-{index:02}", title=f"Chladni Figures - plate {index}",
        creator="Ernst Chladni", date="1787", source="Max Planck Institute / The Public Domain Review",
        sourceUrl="https://publicdomainreview.org/collection/chladni-figures-1787/",
        imageUrl=f"https://pdr-assets.b-cdn.net/collections/chladni-figures-1787/{filename}?height=1200&width=600",
        license="CC BY-SA 3.0 DE (digital copy); underlying work public domain",
        licenseUrl="https://creativecommons.org/licenses/by-sa/3.0/de/"))
works += [
    dict(id="marey-bird-in-flight", title="Bird in Flight", creator="Etienne-Jules Marey", date="1886",
        source="The Metropolitan Museum of Art, Gilman Collection, Purchase, Denise and Andrew Saul Gift, 2005 (2005.100.820)",
        sourceUrl="https://www.metmuseum.org/art/collection/search/286544",
        imageUrl="https://collectionapi.metmuseum.org/api/collection/v1/iiif/286544/611465/main-image",
        license="Public domain / Met Open Access", licenseUrl="https://www.metmuseum.org/policies/image-resources"),
    dict(id="colour-diatom", title="Colour diatom", creator="Kevin Mackenzie, University of Aberdeen", date="",
        source="Wellcome Collection", sourceUrl="https://wellcomecollection.org/works/a48qpsa9",
        imageUrl="https://iiif.wellcomecollection.org/image/B0007598/full/760,/0/default.jpg",
        license="CC BY 4.0", licenseUrl="https://creativecommons.org/licenses/by/4.0/"),
]
# Selected Met works: only records explicitly marked public domain are accepted.
for object_id, slug in [
    (501691, "ceramic-horn"), (261941, "le-gray-great-wave"),
    (333990, "toulouse-la-goulue"), (283222, "watkins-cape-horn"),
    (334197, "toulouse-divan-japonais"), (266441, "muybridge-horse-and-rider"),
    (261937, "le-gray-brig-on-water"), (291521, "atkins-sphacelaria"),
    (459400, "redon-pegasus"), (13269, "whistler-nocturne-green-gold"),
    (359761, "redon-day"),
]:
    record_path = ROOT / "output/met-records" / f"{object_id}.json"
    if record_path.exists():
        record = json.loads(record_path.read_text(encoding="utf-8"))
    else:
        response = requests.get(f"https://collectionapi.metmuseum.org/public/collection/v1/objects/{object_id}", timeout=30)
        response.raise_for_status()
        record = response.json()
        record_path.parent.mkdir(parents=True, exist_ok=True)
        record_path.write_text(json.dumps(record, ensure_ascii=False, indent=2), encoding="utf-8")
    if not record["isPublicDomain"] or not record["primaryImage"]:
        raise ValueError(f"Met object {object_id} has no reusable public-domain image")
    works.append(dict(id=slug, title=record["title"].strip("[]"),
        creator=record["artistDisplayName"] or record["culture"] or "Unknown maker",
        date=record["objectDate"], source=f"The Metropolitan Museum of Art, {record['creditLine']} ({record['accessionNumber']})",
        sourceUrl=record["objectURL"], imageUrl=record["primaryImage"],
        license="Public domain / Met Open Access", licenseUrl="https://www.metmuseum.org/policies/image-resources"))

for slug, image, title, date in [
    ("van-gogh-space", "van-gogh-from", "Van Gogh from Space", "2005"),
    ("ice-stars", "ice-stars", "Ice Stars", "2002"),
    ("robinson-glacier", "robinson-glac", "Robinson Glacier", "2000"),
    ("yukon-delta-0", "yukon-delta3", "Yukon Delta", "2002"),
]:
    works.append(dict(id=slug, title=title, creator="USGS / Landsat 7", date=date,
        source="USGS EROS, Earth As Art", sourceUrl=f"https://www.usgs.gov/media/images/{slug}",
        imageUrl=f"https://d9-wret.s3.us-west-2.amazonaws.com/assets/palladium/production/s3fs-public/thumbnails/image/{image}.jpg",
        license="Public domain", licenseUrl=f"https://www.usgs.gov/media/images/{slug}"))

def prepare(work):
    original = CACHE / (work["id"] + ".original")
    if not original.exists():
        response = requests.get(work["imageUrl"], timeout=120)
        response.raise_for_status()
        original.write_bytes(response.content)
    with Image.open(original) as source:
        image = ImageOps.exif_transpose(source).convert("RGB")
        width, height = image.size
        image.thumbnail((800, 800))
        image.save(DEST / (work["id"] + ".webp"), quality=88, method=6)
    return {**work, "src": f"gallery/{work['id']}.webp", "name": work["id"] + ".webp",
            "width": width, "height": height,
            "changes": "Proportionally resized and converted to WebP; no crop or colour changes."}

with ThreadPoolExecutor(max_workers=4) as pool:
    prepared = {work["id"]: work for work in pool.map(prepare, works)}
# Mix analog texture, motion, microscopic geometry, and colour across the grid.
order = [
    "toulouse-la-goulue",
    "le-gray-great-wave",
    "colour-diatom",
    "chladni-01",
    "van-gogh-space",
    "atkins-sphacelaria",
    "ceramic-horn",
    "toulouse-divan-japonais",
    "marey-bird-in-flight",
    "whistler-nocturne-green-gold",
    "ice-stars",
    "muybridge-horse-and-rider",
    "yukon-delta-0",
    "chladni-09",
    "robinson-glacier",
    "le-gray-brig-on-water",
    "watkins-cape-horn",
    "redon-pegasus",
    "chladni-04",
    "redon-day"
]
filmstrip_order = [
    "toulouse-la-goulue",
    "le-gray-great-wave",
    "colour-diatom",
    "chladni-01",
    "van-gogh-space",
    "atkins-sphacelaria",
    "ceramic-horn",
    "toulouse-divan-japonais",
    "marey-bird-in-flight",
    "whistler-nocturne-green-gold",
    "ice-stars",
    "muybridge-horse-and-rider",
    "yukon-delta-0",
    "chladni-09",
    "robinson-glacier"
]
manifest = ROOT / "src/site/gallery.json"
raw = manifest.read_bytes()
data = json.loads(raw)
data["filmstripIds"] = filmstrip_order
data["photos"] = [prepared[key] for key in order]
newline = "\r\n" if b"\r\n" in raw else "\n"
manifest.write_bytes((json.dumps(data, ensure_ascii=False, indent=2) + "\n").replace("\n", newline).encode("utf-8"))
(DEST / "credits.json").write_text(json.dumps(data["photos"], ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(f"Prepared {len(prepared)} unique archive images with source and licence records.")
