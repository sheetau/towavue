"""Build web-sized copies from the owner's supplied media, leaving originals intact."""
from pathlib import Path
import json
import math
import subprocess
from PIL import Image, ImageOps

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "images"
DEST = ROOT / "public" / "media"
(DEST / "samples").mkdir(parents=True, exist_ok=True)

photos = [
    "quino-al-BlMj6RYy3c0-unsplash.jpg",
    "leman-gujTEMomy5A-unsplash.jpg",
    "ajoy-das--SdQ1Q3oUdg-unsplash.jpg",
    "aarn-giri-3tYZjGSBwbk-unsplash.jpg",
    "andrew-small-EfhCUc_fjrU-unsplash.jpg",
    "rikonavt-oEWdQsbRVZk-unsplash.jpg",
    "magnus-thompson-BBlerGhETwU-unsplash.jpg",
    "michael-navarro-OQLM1Yr9u6k-unsplash.jpg",
    "miom-_0326-k9cL4b3wXZA-unsplash.jpg",
    "siddharth-sarma-Mnztfkyile4-unsplash.jpg",
    "vinh-thang-PeNUDEdA3Xg-unsplash.jpg",
]

photo_metadata = []
for index, name in enumerate(photos, 1):
    with Image.open(SOURCE / "sample media" / name) as original:
        image = ImageOps.exif_transpose(original).convert("RGB")
        photo_metadata.append({"name": name, "bytes": (SOURCE / "sample media" / name).stat().st_size, "width": image.width, "height": image.height})
        image.thumbnail((1600, 1100))
        image.save(DEST / "samples" / f"photo-{index}.webp", quality=88, method=6)
        image.thumbnail((320, 200))
        image.save(DEST / "samples" / f"photo-{index}-thumb.webp", quality=65, method=6)

screenshots = {
    "image.png": "workspace.webp",
    "video.png": "video.webp",
    "audio.png": "audio.webp",
    "gallery.png": "gallery.webp",
    "image2.png": "image2.webp",
    "video-edit.png": "video-edit.webp",
    "tab.png": "tab.webp",
    "filmstrip.png": "filmstrip.webp",
    "shortcut.png": "shortcut.webp",
}
for source, destination in screenshots.items():
    with Image.open(SOURCE / source) as original:
        image = original.convert("RGBA")
        image.thumbnail((1440, 1000))
        image.save(DEST / destination, lossless=True, method=6)

(DEST / "favicon.ico").write_bytes((SOURCE / "favicon.ico").read_bytes())

subprocess.run([
    "ffmpeg", "-y", "-loglevel", "error", "-i",
    str(SOURCE / "sample media" / "13560406_3840_2160_30fps.mp4"),
    "-an", "-vf", "scale=1280:-2", "-c:v", "libx264", "-preset", "medium",
    "-crf", "25", "-pix_fmt", "yuv420p", "-movflags", "+faststart",
    "-g", "15", str(DEST / "samples" / "flowers.mp4"),
], check=True)
subprocess.run([
    "ffmpeg", "-y", "-loglevel", "error", "-i", str(DEST / "samples" / "flowers.mp4"),
    "-frames:v", "1", str(DEST / "samples" / "video-poster.webp"),
], check=True)

with Image.open(SOURCE / "image.png") as original:
    social = Image.new("RGB", (1200, 630), "black")
    screenshot = ImageOps.contain(original.convert("RGBA"), (1160, 590))
    social.paste(screenshot, ((1200 - screenshot.width) // 2, (630 - screenshot.height) // 2), screenshot)
    social.save(DEST / "og-image.png", optimize=True)

# One small sprite provides instant hover previews without seeking the playing video.
video_path = DEST / "samples" / "flowers.mp4"
probe = json.loads(subprocess.check_output([
    "ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "json", str(video_path),
], text=True))
duration = float(probe["format"]["duration"])
interval = 2
count = math.ceil(duration / interval)
subprocess.run([
    "ffmpeg", "-y", "-loglevel", "error", "-i", str(video_path),
    "-vf", f"fps=1/{interval},scale=160:90,tile={count}x1", "-frames:v", "1",
    "-quality", "55", str(DEST / "samples" / "video-thumbnails.webp"),
], check=True)
manifest = {
    "photos": photo_metadata,
    "sampleVideo": {"name": "13560406_3840_2160_30fps.mp4", "bytes": video_path.stat().st_size,
                    "width": 1280, "height": 720, "fps": 30, "duration": duration,
                    "previewInterval": interval, "previewCount": count},
}
(ROOT / "src" / "site" / "media-manifest.json").write_text(
    json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n",
)
print(f"Prepared {len(photos)} photos, app screenshots, silent video, and {count} preview frames.")
