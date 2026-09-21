"""Build web-sized copies from the owner's supplied media, leaving originals intact."""
from pathlib import Path
import subprocess
from PIL import Image, ImageOps

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "images"
DEST = ROOT / "public" / "media"
(DEST / "samples").mkdir(parents=True, exist_ok=True)

photos = [
    "ajoy-das--SdQ1Q3oUdg-unsplash.jpg",
    "leman-gujTEMomy5A-unsplash.jpg",
    "magnus-thompson-BBlerGhETwU-unsplash.jpg",
    "michael-navarro-OQLM1Yr9u6k-unsplash.jpg",
    "miom-_0326-k9cL4b3wXZA-unsplash.jpg",
    "siddharth-sarma-Mnztfkyile4-unsplash.jpg",
    "vinh-thang-PeNUDEdA3Xg-unsplash.jpg",
]

for index, name in enumerate(photos, 1):
    with Image.open(SOURCE / "sample media" / name) as original:
        image = ImageOps.exif_transpose(original).convert("RGB")
        image.thumbnail((1600, 1100))
        image.save(DEST / "samples" / f"photo-{index}.webp", quality=88, method=6)

screenshots = {
    "monapad_ss.png": "workspace.webp",
    "video.png": "video.webp",
    "video-edit.png": "video-edit.webp",
    "tab.png": "tab.webp",
    "filmstrip.png": "filmstrip.webp",
    "shortcut.png": "shortcut.webp",
}
for source, destination in screenshots.items():
    with Image.open(SOURCE / source) as original:
        image = original.convert("RGB")
        image.thumbnail((1440, 1000))
        image.save(DEST / destination, quality=92, method=6)

(DEST / "favicon.ico").write_bytes((SOURCE / "favicon.ico").read_bytes())

subprocess.run([
    "ffmpeg", "-y", "-loglevel", "error", "-i",
    str(SOURCE / "sample media" / "12934697_3840_2160_30fps.mp4"),
    "-an", "-vf", "scale=1280:-2", "-c:v", "libx264", "-preset", "medium",
    "-crf", "25", "-pix_fmt", "yuv420p", "-movflags", "+faststart",
    "-g", "15", str(DEST / "samples" / "flowers.mp4"),
], check=True)
subprocess.run([
    "ffmpeg", "-y", "-loglevel", "error", "-i", str(DEST / "samples" / "flowers.mp4"),
    "-frames:v", "1", str(DEST / "samples" / "video-poster.webp"),
], check=True)

with Image.open(SOURCE / "monapad_ss.png") as original:
    social = Image.new("RGB", (1200, 630), "black")
    screenshot = ImageOps.contain(original.convert("RGB"), (1160, 590))
    social.paste(screenshot, ((1200 - screenshot.width) // 2, (630 - screenshot.height) // 2))
    social.save(DEST / "og-image.png", optimize=True)

print("Prepared supplied photos, app screenshots, favicon, and silent video.")
