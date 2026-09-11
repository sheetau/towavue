"""Generate owned, untracked fixtures for the optional JPEG preview color matrix."""

from pathlib import Path

import PIL
from PIL import Image, features

if PIL.__version__ != "12.3.0":
    raise SystemExit("This optional generator requires Pillow 12.3.0.")

root = Path(__file__).resolve().parent.parent / "tests/generated/jpeg-preview"
root.mkdir(parents=True, exist_ok=True)
size = (2571, 1933)
boxes = [(0, 0, 1285, 966), (1285, 0, 2571, 966),
         (0, 966, 1285, 1933), (1285, 966, 2571, 1933)]
rgb = Image.new("RGB", size)
for box, color in zip(boxes, [(240, 10, 20), (10, 230, 30),
                             (20, 30, 220), (230, 220, 20)]):
    rgb.paste(color, box)
cmyk = Image.new("CMYK", size)
for box, color in zip(boxes, [(0, 230, 210, 35), (220, 0, 190, 70),
                             (190, 170, 0, 120), (0, 0, 0, 255)]):
    cmyk.paste(color, box)


def save(name, source, **options):
    path = root / f"{name}.jpg"
    source.save(path, quality=95, **options)
    with Image.open(path) as encoded:
        reference = encoded.convert("RGBA")
        samples = bytes(channel for x, y in [(1, 1), (3, 1), (1, 3), (3, 3)]
                        for channel in reference.getpixel((size[0] * x // 4,
                                                           size[1] * y // 4)))
    path.with_suffix(".samples").write_bytes(samples)


for mode in ["RGB", "L", "CMYK"]:
    for progressive in [False, True]:
        save(f"{mode}-{progressive}", rgb.convert(mode), progressive=progressive)
for progressive in [False, True]:
    save(f"CMYK-black-{progressive}", cmyk, progressive=progressive)
save("RGB-direct", rgb, keep_rgb=True)
print(f"Generated 9 JPEGs and independent RGBA samples in {root}")
print(f"Pillow {PIL.__version__}, JPEG {features.version_codec('jpg')}, "
      f"libjpeg-turbo {features.check_feature('libjpeg_turbo')}")
