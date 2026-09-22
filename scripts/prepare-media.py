"""Build web-sized copies from the owner's supplied media, leaving originals intact."""
from pathlib import Path
import json
import math
import re
import subprocess
from PIL import Image, ImageOps

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "images"
DEST = ROOT / "public" / "media"
(DEST / "samples").mkdir(parents=True, exist_ok=True)

photos = [
    "quino-al-BlMj6RYy3c0-unsplash.jpg",
    "logan-clark-LdIuq6djo3U-unsplash.jpg",
    "leman-gujTEMomy5A-unsplash.jpg",
    "ajoy-das--SdQ1Q3oUdg-unsplash.jpg",
    "siddharth-sarma-Mnztfkyile4-unsplash.jpg",
    "magnus-thompson-BBlerGhETwU-unsplash.jpg",
    "michael-navarro-OQLM1Yr9u6k-unsplash.jpg",
    "vinh-thang-PeNUDEdA3Xg-unsplash.jpg",
    "miom-_0326-k9cL4b3wXZA-unsplash.jpg",
]
video_source = "Blooming white orchid.mp4"

photo_metadata = []
for index, name in enumerate(photos, 1):
    with Image.open(SOURCE / "sample media" / name) as original:
        image = ImageOps.exif_transpose(original).convert("RGB")
        photo_metadata.append({"name": name, "bytes": (SOURCE / "sample media" / name).stat().st_size, "width": image.width, "height": image.height})
        image.thumbnail((1600, 1100))
        image.save(DEST / "samples" / f"photo-{index}.webp", quality=88, method=6)
        image.thumbnail((320, 200))
        image.save(DEST / "samples" / f"photo-{index}-thumb.webp", quality=65, method=6)

# Remove only numbered derivatives from this generator when the sample list shrinks.
for derivative in (DEST / "samples").glob("photo-*.webp"):
    match = re.fullmatch(r"photo-(\d+)(?:-thumb)?\.webp", derivative.name)
    if match and int(match.group(1)) > len(photos):
        derivative.unlink()

screenshots = {
    "image.png": "workspace.webp",
    "video.png": "video.webp",
    "audio.png": "audio.webp",
    "gallery.png": "gallery.webp",
    "image2.png": "image2.webp",
    "image3.png": "image3.webp",
    "code.png": "code.webp",
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
(DEST / "image2.png").write_bytes((SOURCE / "image2.png").read_bytes())

subprocess.run([
    "ffmpeg", "-y", "-loglevel", "error", "-i",
    str(SOURCE / "sample media" / video_source),
    "-an", "-vf", "scale=1280:-2,setsar=1", "-c:v", "libx264", "-preset", "medium",
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
    "ffprobe", "-v", "error", "-show_entries", "format=duration:stream=width,height,r_frame_rate", "-of", "json", str(video_path),
], text=True))
duration = float(probe["format"]["duration"])
stream = probe["streams"][0]
fps_numerator, fps_denominator = map(int, stream["r_frame_rate"].split("/"))
interval = 0.5
thumb_width = 160
thumb_height = round(thumb_width * stream["height"] / stream["width"])
count = math.ceil(duration / interval)
subprocess.run([
    "ffmpeg", "-y", "-loglevel", "error", "-i", str(video_path),
    "-vf", f"fps=1/{interval},scale={thumb_width}:{thumb_height},tile={count}x1", "-frames:v", "1",
    "-quality", "55", str(DEST / "samples" / "video-thumbnails.webp"),
], check=True)
# Feature artwork gets a separate high-resolution sprite from the original video.
frame_width = 800
frame_height = round(frame_width * stream["height"] / stream["width"])
subprocess.run([
    "ffmpeg", "-y", "-loglevel", "error", "-i", str(SOURCE / "sample media" / video_source),
    "-vf", f"fps=1/{interval},scale={frame_width}:{frame_height},tile={count}x1", "-frames:v", "1",
    "-quality", "86", str(DEST / "samples" / "video-frames.webp"),
], check=True)
# Paused scrubbing must not depend on an iOS video decoder repainting a seek.
still_rate = 12
still_count = math.ceil(duration * still_rate)
still_dir = DEST / "samples" / "video-stills"
still_dir.mkdir(exist_ok=True)
subprocess.run([
    "ffmpeg", "-y", "-loglevel", "error", "-i", str(video_path),
    "-vf", f"fps={still_rate}:start_time=0,scale=960:-1,setsar=1", "-frames:v", str(still_count),
    "-c:v", "libwebp", "-f", "image2", "-start_number", "0", "-quality", "82", str(still_dir / "frame-%03d.webp"),
], check=True)
for old in still_dir.glob("frame-*.webp"):
    if re.fullmatch(r"frame-\d+\.webp", old.name) and int(old.stem.split("-")[1]) >= still_count:
        old.unlink()
assert all((still_dir / f"frame-{index:03d}.webp").exists() for index in range(still_count))
manifest = {
    "photos": photo_metadata,
    "sampleVideo": {"name": video_source, "bytes": video_path.stat().st_size,
                    "width": stream["width"], "height": stream["height"], "fps": fps_numerator / fps_denominator, "duration": duration,
                    "previewInterval": interval, "previewCount": count,
                    "previewWidth": thumb_width, "previewHeight": thumb_height,
                    "frameWidth": frame_width, "frameHeight": frame_height,
                    "stillFrameRate": still_rate, "stillFrameCount": still_count},
}
(ROOT / "src" / "site" / "media-manifest.json").write_text(
    json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n",
)
print(f"Prepared {len(photos)} photos, app screenshots, silent video, and {count} preview frames.")
