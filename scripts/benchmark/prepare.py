"""Create a deterministic, synthetic, redistributable media benchmark pilot."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import sys
import wave

import numpy as np
from PIL import Image, ImageCms, ImageDraw, ImageFont
import PIL


def digest(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def pattern(width, height, identity):
    # Fine texture, gradients and edges prevent a flat-color decode workload.
    rng = np.random.default_rng(10400 + identity)
    y, x = np.indices((height, width), dtype=np.int32)
    noise = rng.integers(0, 48, (height, width), dtype=np.uint8)
    pixels = np.stack(((x * 255 // width + noise) % 256,
                       (y * 255 // height + noise) % 256,
                       ((x // 19 ^ y // 23) * 31 + noise) % 256), axis=2).astype(np.uint8)
    image = Image.fromarray(pixels)
    draw = ImageDraw.Draw(image)
    # A large unique two-color grid is identifiable after application scaling.
    # Identity detection proves recognizable content, not original-pixel quality.
    unit = min(width // 12, height // 8)
    left, top = width // 2 - 3 * unit, height // 2 - 2 * unit
    draw.rectangle((left - unit // 4, top - unit // 4, left + 6 * unit, top + 4 * unit), fill=(8, 240, 96))
    bits = np.random.default_rng(831 + identity).integers(0, 2, (4, 6))
    for row in range(4):
        for col in range(6):
            color = (238, 42, 210) if bits[row, col] else (24, 210, 240)
            draw.rectangle((left + col * unit, top + row * unit,
                            left + (col + 1) * unit - 1, top + (row + 1) * unit - 1), fill=color)
    draw.text((width // 30, height // 30), f"TOWAVUE BENCHMARK {identity:02d}", fill="white",
              font=ImageFont.load_default(size=max(14, height // 35)))
    return image


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--ffmpeg", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    marker = root / "CORPUS_OWNER.json"
    if any(root.iterdir()) and not marker.exists():
        raise SystemExit("Refusing a nonempty, unowned corpus directory")
    marker.write_text(json.dumps({"generator": "towavue-benchmark-pilot-v1"}) + "\n", encoding="utf-8")
    media = root / "media"
    media.mkdir(exist_ok=True)
    references = root / "references"
    references.mkdir(exist_ok=True)
    logs = root / "logs"
    logs.mkdir(exist_ok=True)
    ffmpeg = args.ffmpeg.resolve()
    ffprobe = ffmpeg.with_name("ffprobe.exe")
    commands = []
    records = []

    def run(name, arguments):
        command = [str(ffmpeg), "-hide_banner", "-loglevel", "error", "-y", *map(str, arguments)]
        commands.append(command)
        result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        (logs / (name + ".log")).write_bytes(result.stdout + result.stderr)
        if result.returncode:
            raise RuntimeError(f"Fixture encoding failed: {name}; see its log")

    def add(name, kind, identity, detail):
        path = media / name
        result = subprocess.run([str(ffprobe), "-v", "error", "-show_streams", "-show_format", "-of", "json", str(path)],
                                capture_output=True, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0), check=True)
        record = {"id": path.stem, "path": str(path.relative_to(root)), "kind": kind,
                  "identity": identity, "bytes": path.stat().st_size, "sha256": digest(path),
                  "detail": detail, "probe": json.loads(result.stdout)}
        records.append(record)
        print(f"Prepared {name}", flush=True)

    images = [
        ("image01-jpeg-1080.jpg", 1920, 1080, "JPEG", {"quality": 90}),
        ("image02-jpeg-24mp.jpg", 6000, 4000, "JPEG", {"quality": 95}),
        ("image03-png-alpha-4k.png", 3840, 2160, "PNG", {}),
        ("image04-webp-4k.webp", 3840, 2160, "WEBP", {"quality": 90}),
        ("image05-avif-4k.avif", 3840, 2160, "AVIF", {"quality": 85, "speed": 8}),
        ("image06-tiff-4k.tiff", 3840, 2160, "TIFF", {"compression": "tiff_lzw"}),
        ("image07-exif-portrait.jpg", 1920, 1080, "JPEG", {"quality": 90}),
        ("image08-icc-srgb.jpg", 3840, 2160, "JPEG", {"quality": 90}),
    ]
    for identity, (name, width, height, fmt, options) in enumerate(images, 1):
        image = pattern(width, height, identity)
        if identity == 3:
            image = image.convert("RGBA")
            alpha = Image.new("L", image.size, 255)
            ImageDraw.Draw(alpha).rectangle((0, height * 3 // 4, width // 6, height), fill=96)
            image.putalpha(alpha)
        if identity == 7:
            image = image.transpose(Image.Transpose.ROTATE_90)
            exif = Image.Exif()
            exif[274] = 6
            options["exif"] = exif
        if identity == 8:
            options["icc_profile"] = ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB")).tobytes()
        image.save(media / name, fmt, **options)
        # Decode the encoded file for detection, applying EXIF display orientation.
        from PIL import ImageOps
        with Image.open(media / name) as decoded:
            ImageOps.exif_transpose(decoded).convert("RGB").resize((480, 270)).save(references / f"{Path(name).stem}.png")
        add(name, "image", identity, "Synthetic texture and unique content marker; first recognizable content endpoint")

    for identity, suffix in [(9, "gif"), (10, "webp")]:
        frames = [pattern(960, 540, identity) for _ in range(8)]
        for index, frame in enumerate(frames):
            ImageDraw.Draw(frame).rectangle((index * 110, 470, index * 110 + 80, 530), fill="yellow")
        name = f"image{identity:02d}-animation.{suffix}"
        frames[0].save(media / name, save_all=True, append_images=frames[1:], duration=100, loop=0)
        frames[0].resize((480, 270)).save(references / f"{Path(name).stem}.png")
        add(name, "animation", identity, "Eight-frame 10 fps synthetic animation")

    videos = [
        ("video11-h264-1080.mp4", 1920, 1080, "libopenh264", ["-b:v", "8M", "-g", "60"], "yuv420p"),
        ("video12-hevc-4k10.mp4", 3840, 2160, "hevc_nvenc", ["-preset", "p4", "-cq", "24", "-g", "120"], "p010le"),
        ("video13-vp9-1080.webm", 1920, 1080, "libvpx-vp9", ["-deadline", "realtime", "-cpu-used", "6", "-b:v", "6M", "-g", "120"], "yuv420p"),
        ("video14-av1-4k10.mp4", 3840, 2160, "libsvtav1", ["-preset", "12", "-crf", "32", "-g", "120", "-svtav1-params", "lp=4"], "yuv420p10le"),
        ("video15-h264-long-gop.mp4", 1920, 1080, "libopenh264", ["-b:v", "8M", "-g", "300"], "yuv420p"),
    ]
    for identity, (name, width, height, codec, options, pixel) in enumerate(videos, 11):
        source = references / f"source{identity}.png"
        image = pattern(width, height, identity)
        image.save(source)
        image.resize((480, 270)).save(references / f"{Path(name).stem}.png")
        # A changing frame counter supports separate playback/seek verification.
        # The time index is burned in, independently of application UI clocks.
        vf = "drawbox=x=0:y=ih-90:w=iw:h=90:color=black:t=fill,drawtext=text='%{n}':x=20:y=h-75:fontsize=48:fontcolor=white"
        run(name, ["-loop", "1", "-framerate", "30", "-i", source, "-f", "lavfi", "-i", "sine=frequency=997:sample_rate=48000",
                   "-t", "12", "-vf", vf, "-c:v", codec, *options, "-pix_fmt", pixel,
                   "-c:a", "libopus" if name.endswith("webm") else "aac", "-b:a", "128k", "-threads", "4", media / name])
        add(name, "video", identity, "12 s, 30 fps, burned-in frame number, 997 Hz audio; corpus encoding is not a benchmark")

    sample_rate = 48000
    time_axis = np.arange(sample_rate * 30) / sample_rate
    signal = (0.1 * np.sin(2 * np.pi * 997 * time_axis) * (0.3 + 0.7 * (np.floor(time_axis * 4) % 2)))
    pcm = np.repeat((signal * 32767).astype("<i2")[:, None], 2, axis=1)
    wav = media / "audio16-pcm.wav"
    with wave.open(str(wav), "wb") as output:
        output.setparams((2, 2, sample_rate, 0, "NONE", "not compressed"))
        output.writeframes(pcm.tobytes())
    add(wav.name, "audio", 16, "30 s stereo 48 kHz, amplitude-coded 997 Hz tone; loopback endpoint required")
    for identity, extension, codec in [(17, "flac", "flac"), (18, "mp3", "libmp3lame"), (19, "m4a", "aac"), (20, "opus", "libopus")]:
        name = f"audio{identity:02d}.{extension}"
        run(name, ["-i", wav, "-c:a", codec, *([] if codec == "flac" else ["-b:a", "192k"]), media / name])
        add(name, "audio", identity, "Transcoded from audio16; codec delay is retained")
    manifest = {"schema": 1, "generator_sha256": digest(Path(__file__)),
                "python": sys.version, "numpy": np.__version__, "pillow": PIL.__version__,
                "platform": platform.platform(), "ffmpeg_sha256": digest(ffmpeg),
                "provenance": "Procedural synthetic fixtures created by this script; no owner media or third-party photographs.",
                "limitations": "Pilot for format and measurement coverage; not a representative photographic/cinema corpus.",
                "files": records, "generation_commands": commands}
    (root / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"Complete: {len(records)} fixtures", flush=True)


if __name__ == "__main__":
    main()
