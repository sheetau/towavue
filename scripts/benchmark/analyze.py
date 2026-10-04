"""Summarize every attempted measurement, including failures and interval widths."""
import argparse
from collections import defaultdict
import csv
import json
from pathlib import Path

import numpy as np

LABELS = {
    "image01-jpeg-1080": "JPEG 1920 x 1080",
    "image02-jpeg-24mp": "JPEG 24 MP",
    "image03-png-alpha-4k": "PNG with alpha 4K",
    "image04-webp-4k": "WebP 4K",
    "image05-avif-4k": "AVIF 4K",
    "image06-tiff-4k": "TIFF 4K",
    "image07-exif-portrait": "JPEG EXIF rotation",
    "image08-icc-srgb": "JPEG sRGB profile 4K",
    "image09-animation": "Animated GIF 960 x 540",
    "image10-animation": "Animated WebP 960 x 540",
    "video11-h264-1080": "H.264 1080p",
    "video12-hevc-4k10": "HEVC 4K 10-bit",
    "video13-vp9-1080": "VP9 1080p",
    "video14-av1-4k10": "AV1 4K 10-bit",
    "video15-h264-long-gop": "H.264 1080p long GOP",
    "audio16-pcm": "PCM WAV",
    "audio17": "FLAC",
    "audio18": "MP3",
    "audio19": "AAC (M4A)",
    "audio20": "Opus",
}


def summarize(rows):
    groups = defaultdict(list)
    for row in rows:
        if row.get("phase") == "measurement":
            groups[(row["scenario"], row["endpoint"], row["case"], row["app"])].append(row)
    result = []
    for (scenario, endpoint, case, app), attempts in sorted(groups.items()):
        good = [row for row in attempts if row["status"] == "ok"]
        summary = dict(scenario=scenario, endpoint=endpoint, case=case, app=app,
                       attempts=len(attempts), successes=len(good),
                       success_rate=len(good) / len(attempts),
                       failures=json.dumps({status: sum(row["status"] == status for row in attempts)
                                            for status in sorted({row["status"] for row in attempts})
                                            if status != "ok"}))
        if good:
            lower = np.array([row["latency_lower_ms"] for row in good], dtype=float)
            upper = np.array([row["latency_upper_ms"] for row in good], dtype=float)
            indexes = np.random.default_rng(10404).integers(0, len(good), (10000, len(good)))
            summary.update(
                median_lower_ms=float(np.median(lower)), median_upper_ms=float(np.median(upper)),
                median_bootstrap_low_ms=float(np.percentile(np.median(lower[indexes], axis=1), 2.5)),
                median_bootstrap_high_ms=float(np.percentile(np.median(upper[indexes], axis=1), 97.5)),
                p95_lower_ms=float(np.quantile(lower, .95, method="inverted_cdf")),
                p95_upper_ms=float(np.quantile(upper, .95, method="inverted_cdf")),
                median_interval_width_ms=float(np.median(upper - lower)),
            )
        result.append(summary)
    return result


def write_csv(path, rows):
    fields = list(dict.fromkeys(key for row in rows for key in row))
    with path.open("w", newline="", encoding="utf-8") as file:
        writer = csv.DictWriter(file, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)


def plot(path, summaries, prefix, title):
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    selected = [row for row in summaries if row["case"].startswith(prefix)]
    if not selected:
        return
    cases = sorted({row["case"] for row in selected})
    apps = sorted({row["app"] for row in selected})
    colors = {"towavue": "#087e8b", "irfanview": "#9b4eaf", "mpc-hc": "#cc7134", "vlc": "#5073be"}
    fig, ax = plt.subplots(figsize=(12, max(4, len(cases) * .72)))
    height = .78 / len(apps)
    for app_index, app in enumerate(apps):
        for case_index, case in enumerate(cases):
            row = next((row for row in selected if row["case"] == case and row["app"] == app), None)
            if not row or not row["successes"]:
                continue
            y = case_index + (app_index - (len(apps) - 1) / 2) * height
            low, high = row["median_lower_ms"], row["median_upper_ms"]
            ax.barh(y, high, height=height * .83, color=colors.get(app, "gray"),
                    label=app if case_index == 0 else None, alpha=.85)
            ax.plot([low, high], [y, y], color="black", linewidth=2)
            ax.plot(row["p95_upper_ms"], y, "|", color="black", markersize=9)
    ax.set_yticks(range(len(cases)), [LABELS.get(case, case) for case in cases])
    ax.invert_yaxis()
    ax.set_xlabel("Milliseconds from command launch (smaller is faster)")
    loopback = all(row['endpoint'] == 'loopback_detector_completion' for row in selected)
    detail = ("Bar: median loopback detector completion; tick: p95" if loopback else
              "Bar: median capture upper bound; black segment: median bounds; tick: p95 upper bound")
    ax.set_title(title + "\n" + detail, fontsize=11)
    ax.grid(axis="x", alpha=.2)
    ax.set_axisbelow(True)
    handles, labels = ax.get_legend_handles_labels()
    fig.legend(handles, labels, loc="lower center", ncol=len(apps), frameon=False)
    fig.tight_layout(rect=(0, .06, 1, 1))
    fig.savefig(path.with_suffix(".png"), dpi=160)
    fig.savefig(path.with_suffix(".svg"))
    plt.close(fig)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    args = parser.parse_args()
    rows = [json.loads(line) for line in (args.root / "samples.jsonl").read_text(encoding="utf-8").splitlines()]
    summaries = summarize(rows)
    write_csv(args.root / "summary.csv", summaries)
    flat = [{key: value for key, value in row.items() if key not in ("samples", "visibility_samples", "command", "marker")}
            for row in rows]
    write_csv(args.root / "trials.csv", flat)
    (args.root / "summary.json").write_text(json.dumps(summaries, indent=2) + "\n", encoding="utf-8")
    plot(args.root / "images", summaries, "image", "Synthetic images: warm-file new-process recognizable content")
    video_audio = all(row['endpoint'] == 'loopback_detector_completion' for row in summaries)
    plot(args.root / "videos", summaries, "video", "Synthetic video: warm-file new-process " +
         ("embedded audio loopback detection" if video_audio else "first recognizable frame"))
    plot(args.root / "audio", summaries, "audio", "Synthetic audio: warm-file new-process loopback detection")
    print(json.dumps({"groups": len(summaries), "attempts": sum(row["attempts"] for row in summaries),
                      "successes": sum(row["successes"] for row in summaries)}))


if __name__ == "__main__":
    main()
