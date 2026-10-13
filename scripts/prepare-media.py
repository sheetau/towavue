"""Prepare Frame 3's supplied assets without changing the source files."""
from pathlib import Path
import json
import shutil
from PIL import Image, ImageOps

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / "public/media"
(DEST / "gallery").mkdir(parents=True, exist_ok=True)
(DEST / "fonts").mkdir(exist_ok=True)

# Archive images are prepared independently; preserve the selected catalogue.
photos = json.loads((ROOT / "src/site/gallery.json").read_text(encoding="utf-8"))["photos"]

artwork = {}
for name in ["workspace1", "workspace2", "workspace3", "audio wave"]:
    source = ROOT / "images" / f"{name}.png"
    filename = f"{name.replace(' ', '-')}.png"
    shutil.copyfile(source, DEST / filename)
    with Image.open(source) as image:
        artwork[name] = {"src": filename, "width": image.width, "height": image.height}

fonts = Path("D:/Documents/sheeta's_monooki/myDownloads/.Fonts")
shutil.copyfile(fonts / "Atelier Femme Serif.ttf", DEST / "fonts/atelier-femme-serif.ttf")
shutil.copyfile(fonts / "helveticanowtext/HelveticaNowText-Regular.ttf", DEST / "fonts/helvetica-now-text.ttf")
(ROOT / "src/site/gallery.json").write_text(json.dumps({"photos": photos, "artwork": artwork}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(f"Prepared {len(photos)} gallery images, {len(artwork)} original artwork files, and 2 fonts.")
