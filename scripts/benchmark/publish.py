"""Generate public benchmark documentation from reviewed, portable data."""
from __future__ import annotations

import argparse
from collections import defaultdict
import json
import math
from pathlib import Path
from statistics import median
import tempfile

REPO = Path(__file__).resolve().parents[2]
APPS = {
    "towavue": "towavue", "irfanview": "IrfanView", "mpc-hc": "MPC-HC",
    "vlc": "VLC", "photos": "Photos",
}
COLORS = {"towavue": "#087f8c", "irfanview": "#dd7a28", "mpc-hc": "#8055ad",
          "vlc": "#cc486c", "photos": "#5384b1"}
WORKLOADS = (
    "Image startup", "Image navigation", "Image file open", "Video startup",
    "Video seek", "Video navigation", "Video file open", "Audio startup",
    "Audio seek", "Audio navigation", "Audio file open",
)
CATEGORIES = {
    "image-startup": ("Image and animation startup", "New process with a file path. Recognition is the overview endpoint; settled viewport is a separate observation. Photos uses URI activation with a potentially retained spare process, so its boundary differs."),
    "navigation": ("Image folder navigation", "Next, previous, first and last file in homogeneous image folders. The large folder contains 1,000 small JPEGs. towavue/IrfanView use three process blocks, except the qualified JPEG 24 MP study with ten. Photos uses ten viewer sessions with its filmstrip visible; its large-folder results cover next/previous only. See the Photos protocol below."),
    "mixed": ("Mixed image and animation navigation", "The same four navigation commands in a mixed-format folder; three towavue/IrfanView process blocks and ten Photos viewer sessions. Photos uses copies preserving the same encoded images and visited content sequence."),
    "video-startup": ("Video startup", "New process to recognition of the expected video frame; three measured blocks."),
    "audio-startup": ("Audio startup", "New process to loopback detection of the expected audio; 12-second and 180-second fixtures, three measured blocks."),
    "seek": ("Video and audio seek", "A running player seeks to aligned target positions. Video-frame recognition and audio detection remain separate endpoints. The overview uses video recognition for video seek and audio-only fixtures for audio seek. Qualified towavue/MPC-HC video observations use twelve blocks and PCM WAV ten; other observations use three."),
    "media": ("Video and audio navigation", "Next/previous in towavue folders and comparator playlists, in a warm running process. Three process blocks. Within-session towavue resume can apply. Only matching measured case/action pairs enter the overview."),
    "direct-open": ("Open file", "Another file is sent to the same running, owned window: towavue production WM_COPYDATA, IrfanView WM_DROPFILES, MPC-HC API and VLC RC. Startup and Windows dialogs are outside this measurement. Audio targets are unvisited; pausing the prior towavue tab is outside timing. Homogeneous folders and mixed sessions are distinct workloads. Six image conditions use thirty new paired blocks on 7b2422; the other cells retain ten blocks on dce8e3. The mixed refresh visits three selected targets, a narrower context than the older ten-format sequence."),
    "video-audio-startup": ("Video soundtrack startup", "Supplemental loopback-audio endpoint for video process startup; three blocks. This is not included in the audio-only startup overview."),
    "pacing": ("Playback cadence", "Desktop capture of a 30 fps video source. These intervals should match the source cadence, so lower is not necessarily better. No winner is assigned; this supplemental endpoint is excluded from the speed overview."),
}
METRICS = {
    "recognizable": "Recognizable content", "converged": "Settled viewport",
    "latency": "Visual completion", "audio": "Audio detection", "video": "Video frame recognition",
    "per_trial_median_frame_interval": "Median frame interval",
    "per_trial_p95_frame_interval": "P95 frame interval",
}
SCENARIOS = {
    "startup": "Startup", "next": "Next file", "previous": "Previous file",
    "distant_first": "Jump to first file", "distant_last": "Jump to last file",
    "seek_aligned": "Seek", "desktop_pacing": "Playback cadence",
    "direct_open_video": "Video file open", "direct_open_audio12": "Audio file open: 12 s",
    "direct_open_audio180": "Audio file open: 180 s", "direct_open_mixed": "Image file open: mixed images",
    "direct_open_jpeg1080": "Image file open: JPEG 1080p folder",
    "direct_open_jpeg24mp": "Image file open: JPEG 24 MP folder",
    "direct_open_png4k": "Image file open: PNG 4K folder",
    "direct_open_large1000": "Image file open: 1,000 small JPEGs",
}


def condition(row):
    return tuple(row[k] for k in ("category", "case", "scenario", "metric"))


def app_label(app, startup=False):
    return APPS[app] + ("*" if app == "photos" and startup else "")


def selected(row, workload):
    cat, metric = row["category"], row["metric"]
    predicates = (
        cat == "image-startup" and metric == "recognizable",
        cat in ("navigation", "mixed") and metric == "latency",
        cat == "direct-open" and metric == "latency" and row["scenario"] != "direct_open_video",
        cat == "video-startup" and metric == "recognizable",
        cat == "seek" and metric == "video",
        cat == "media" and metric == "latency",
        cat == "direct-open" and row["scenario"] == "direct_open_video",
        cat == "audio-startup" and metric == "audio",
        cat == "seek" and metric == "audio" and row["case"].startswith("seek-"),
        cat == "media" and metric == "audio",
        cat == "direct-open" and metric == "audio",
    )
    return predicates[WORKLOADS.index(workload)]


def aggregate(rows, apps):
    """Match conditions first, then give each fixture equal weight."""
    by_condition = defaultdict(dict)
    for row in rows:
        if row["app"] in apps:
            key = condition(row)
            if row["app"] in by_condition[key]:
                raise ValueError(f"Duplicate cell: {key}, {row['app']}")
            by_condition[key][row["app"]] = row
    matched = {key: group for key, group in by_condition.items() if set(group) == set(apps)}
    if not matched:
        raise ValueError("No common conditions")
    values, fixture_values = {}, {}
    for app in apps:
        fixtures = defaultdict(list)
        for key, group in matched.items():
            fixtures[key[:2]].append(group[app]["median_ms"])
        fixture_values[app] = [{"category": key[0], "case": key[1], "median_ms": median(v)}
                               for key, v in sorted(fixtures.items())]
        values[app] = median(item["median_ms"] for item in fixture_values[app])
    return {"values_ms": values, "fixtures": len(fixtures), "conditions": len(matched),
            "matched_conditions": [list(key) for key in sorted(matched)],
            "fixture_values": fixture_values}


def overview(data):
    result = []
    for workload in WORKLOADS:
        apps = (["towavue", "irfanview", "photos"] if workload == "Image startup" else
                ["towavue", "irfanview"] if workload.startswith("Image") else
                ["towavue", "mpc-hc", "vlc"])
        if workload == "Image navigation" and any(
                row["app"] == "photos" and selected(row, workload) for row in data["records"]):
            apps.append("photos")
        result.append({"workload": workload, **aggregate(
            [row for row in data["records"] if selected(row, workload)], apps)})
    return result


def validate(data):
    studies = {s["id"] for s in data["studies"]}
    keys, groups = set(), defaultdict(set)
    for row in data["records"]:
        key = (*condition(row), row["app"])
        if key in keys:
            raise ValueError(f"Duplicate cell: {key}")
        keys.add(key)
        groups[condition(row)].add(row["app"])
        assert row["category"] in CATEGORIES and row["metric"] in METRICS
        assert row["scenario"] in SCENARIOS and row["study"] in studies
        assert row["app"] in APPS and row["case"] in data["labels"]
        assert all(math.isfinite(row[k]) and row[k] >= 0 for k in ("median_ms", "p95_ms"))
        assert 0 < row["successful"] <= row["attempts"] and row["blocks"] > 0
    assert all("towavue" in apps and len(apps) >= 2 for apps in groups.values())
    serialized = json.dumps(data)
    assert not any(s in serialized for s in ("C:\\", "C:/", ".dashboard", "APPDATA"))
    overview(data)


def plotting():
    import matplotlib
    matplotlib.use("Agg")
    matplotlib.rcParams.update({"svg.hashsalt": "towavue-public-benchmark-v1", "font.family": "DejaVu Sans"})
    import matplotlib.pyplot as plt
    return plt


def save_figure(fig, stem):
    import matplotlib
    fig.canvas.draw()
    renderer = fig.canvas.get_renderer()
    hidden_ticks = {label for ax in fig.axes for tick in ax.xaxis.get_major_ticks()
                    if not ax.get_xlim()[0] <= tick.get_loc() <= ax.get_xlim()[1]
                    for label in (tick.label1, tick.label2)}
    for text in fig.findobj(match=matplotlib.text.Text):
        if text in hidden_ticks or not text.get_visible() or not text.get_text():
            continue
        box = text.get_window_extent(renderer)
        if box.x0 < -1 or box.y0 < -1 or box.x1 > fig.bbox.width + 1 or box.y1 > fig.bbox.height + 1:
            raise ValueError(f"Clipped chart text in {stem.name}: {text.get_text()}")
    stem.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(stem.with_suffix(".png"), dpi=150, facecolor="white")
    svg = stem.with_suffix(".svg")
    fig.savefig(svg, metadata={"Date": None}, facecolor="white")
    svg.write_bytes(b"\n".join(line.rstrip() for line in svg.read_bytes().splitlines()) + b"\n")


def style_axis(ax):
    ax.grid(axis="x", alpha=.15)
    ax.set_axisbelow(True)
    for spine in ("top", "right", "left"):
        ax.spines[spine].set_visible(False)
    ax.tick_params(axis="y", length=0)


def overview_chart(items, destination, updated):
    plt = plotting()
    fig, axes = plt.subplots(len(items), 1, figsize=(14, 21.5))
    for ax, item in zip(axes, items):
        values = item["values_ms"]
        best, maximum = min(values.values()), max(values.values())
        for i, (app, value) in enumerate(values.items()):
            leader = math.isclose(value, best, abs_tol=1e-9)
            ax.barh(i, value, height=.52, color=COLORS[app], alpha=.95 if leader else .23)
            ax.text(value + maximum * .015, i, f"{value:.1f} ms", va="center", fontsize=11.5,
                    weight="bold" if leader else "normal", color="#17212f")
        ax.set_yticks(range(len(values)), [app_label(a, item["workload"] == "Image startup") for a in values], fontsize=11.5)
        ax.invert_yaxis()
        ax.set_xlim(0, maximum * 1.16)
        ax.set_title(item["workload"], loc="left", fontsize=15, weight="bold", pad=8)
        ax.set_xlabel(f"ms · {item['fixtures']} fixtures", fontsize=11, color="#526071")
        style_axis(ax)
    fig.suptitle("towavue · media benchmark", fontsize=22, weight="bold", x=.12, ha="left", y=.989)
    fig.text(.12, .966, "Median across fixtures, after taking each fixture's median across actions. Lower is faster.", fontsize=11)
    fig.text(.12, .034, "Shorter bars are faster. Solid: fastest observed value; faded: other apps.", fontsize=11)
    fig.text(.12, .024, "Each panel has its own scale; compare apps within a panel. Descriptive values, not a significance test.", fontsize=11)
    fig.text(.12, .014, f"{updated} · Windows 11 · Ryzen 9 9900X · RTX 5070 Ti · Development builds.", fontsize=11)
    fig.text(.12, .004, "* Photos startup uses URI activation with a different process boundary. See the detailed benchmark for navigation conditions.", fontsize=9)
    fig.subplots_adjust(left=.12, right=.98, top=.938, bottom=.068, hspace=.90)
    save_figure(fig, destination / "overview")
    plt.close(fig)


def detail_chart(data, rows, stem):
    plt = plotting()
    cases = sorted({r["case"] for r in rows})
    apps = [app for app in APPS if any(r["app"] == app for r in rows)]
    pacing = rows[0]["category"] == "pacing"
    maximum = max(r["median_ms"] for r in rows)
    fig, ax = plt.subplots(figsize=(14, max(4.8, 2.5 + len(cases) * (.28 * len(apps) + .35))))
    for i, case in enumerate(cases):
        group = [r for r in rows if r["case"] == case]
        best = min(r["median_ms"] for r in group)
        for j, app in enumerate(apps):
            y = i * (len(apps) + 1) + j - (len(apps) - 1) / 2
            row = next((r for r in group if r["app"] == app), None)
            if row is None:
                ax.text(maximum * .013, y, f"{APPS[app]} — unavailable", va="center",
                        fontsize=9, style="italic", color="#64748b")
                continue
            value = row["median_ms"]
            leader = pacing or math.isclose(value, best, abs_tol=1e-9)
            ax.barh(y, value, .72, color=COLORS[app], alpha=.95 if leader else .23)
            lower = row.get("median_lower_ms", value)
            if lower < value:
                ax.plot([lower, value], [y, y], color="#64748b", linewidth=2, alpha=.55)
            ax.text(value + maximum * .013, y, f"{app_label(app, rows[0]['category'] == 'image-startup')}  {value:.2f} ms", va="center", fontsize=9,
                    weight="bold" if leader else "normal", color="#17212f" if leader else "#64748b")
    ax.set_yticks([i * (len(apps) + 1) for i in range(len(cases))],
                  [data["labels"][case] for case in cases], fontsize=10)
    ax.invert_yaxis()
    ax.set_xlim(0, maximum * 1.40)
    ax.set_xlabel("Observed interval (ms)" if pacing else "Median completion (ms)")
    ax.set_title(f"{SCENARIOS[rows[0]['scenario']]} — {METRICS[rows[0]['metric']].lower()}",
                 loc="left", fontsize=17, weight="bold", pad=24)
    style_axis(ax)
    if pacing:
        ax.axvline(1000 / 30, color="#64748b", linestyle="--", linewidth=1)
    footer = ("30 fps source (dashed line): cadence should match the source. No speed ranking." if pacing else
              "Shorter bars are faster. Solid: fastest observed median; faded: other apps. Descriptive order, not a significance test.")
    fig.text(.02, .050, footer, fontsize=9)
    fig.text(.02, .015, "Gray segments: median capture bounds where available, not confidence intervals. Build and sampling details are in the tables.", fontsize=8.5)
    fig.subplots_adjust(left=.25, right=.98, top=.84, bottom=.19 if len(cases) <= 2 else .12)
    save_figure(fig, stem)
    plt.close(fig)


def confidence_label(data, comparison):
    prefix = "Mixed: " if comparison["group"] == "mixed" else ""
    return prefix + data["labels"][comparison["case"]]


def confidence_chart(data, destination):
    plt = plotting()
    fig, ax = plt.subplots(figsize=(12, 5))
    comparisons = data["open_confidence"]["comparisons"]
    for i, row in enumerate(comparisons):
        low, high = row["simultaneous_interval_ms"]
        value = row["median_paired_gap_ms"]
        color = COLORS["towavue"] if high < 0 else "#64748b"
        ax.plot([low, high], [i, i], color=color, linewidth=3)
        ax.plot(value, i, "o", color=color)
        ax.text(high + 1, i, f"{value:+.2f} [{low:+.2f}, {high:+.2f}]", va="center", fontsize=9)
    ax.set_yticks(range(len(comparisons)), [confidence_label(data, r) for r in comparisons])
    ax.invert_yaxis()
    ax.axvline(0, color="#64748b", linestyle="--")
    ax.set_xlim(-58, 37)
    ax.set_xlabel("Paired completion gap (ms): negative favors towavue; positive favors IrfanView")
    ax.set_title("Image file open · 30 paired blocks per condition", loc="left", fontsize=17, weight="bold", pad=20)
    style_axis(ax)
    fig.text(.025, .03, "Simultaneous intervals: at least 95% family coverage under the independent, stable-block model. Capture sensitivity is separate.", fontsize=9)
    fig.subplots_adjust(left=.25, right=.98, top=.82, bottom=.19)
    save_figure(fig, destination / "image-open-confidence")
    plt.close(fig)


def write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes((content.rstrip() + "\n").encode("utf-8"))


def report(data, items, groups):
    leaders = sum(item["values_ms"]["towavue"] == min(item["values_ms"].values()) for item in items)
    doc = ["<!-- Generated by scripts/benchmark/publish.py; edit the data or generator. -->",
           "# Media benchmarks",
           f"Updated **{data['updated']}**. towavue has the lowest representative completion time in **{leaders} of {len(items)} workloads** below, compared with IrfanView/Photos for images and MPC-HC/VLC for video and audio. This describes these fixtures on this Windows PC; it does not mean every file or every individual comparison is a statistically established win.",
           "[Overview](#overview) · [Measurement method](#measurement-method) · [Photos navigation](#photos-navigation) · [Image-open confidence](#image-open-confidence) · [Complete results](#complete-results) · [Environment and builds](#environment-and-builds) · [Data and updates](#data-and-updates)",
           "## Overview", "![Eleven workloads, with one aggregate bar per application](benchmarks/overview.png)",
           "[Scalable overview](benchmarks/overview.svg)",
           "**Shorter bars are faster.** Solid bars mark the lowest observed value; other apps are faded. Each overview panel has its own zero-based scale: compare apps within a panel. Ranking is descriptive, not an aggregate significance test.",
           "| Workload | Fixtures / conditions | towavue | IrfanView | MPC-HC | VLC | Photos |",
           "|---|---:|---:|---:|---:|---:|---:|"]
    for item in items:
        values = [f"{item['values_ms'][app]:.2f}" if app in item["values_ms"] else "—" for app in APPS]
        doc.append(f"| {item['workload']} | {item['fixtures']} / {item['conditions']} | " + " | ".join(values) + " |")
    doc += ["All values are milliseconds. A dash means that app is not part of that comparison, not that it cannot perform the operation.",
            "**How the bars are aggregated:** start with each measured condition's median. Keep only fixture/action conditions covered by every app listed for that workload. Within each fixture, take the median across actions; then take the median across fixtures. Every fixture has equal weight, regardless of trial count or number of actions. For startup, seek and file open there is one condition per fixture; navigation can have several. This is a typical fixture score, not a pooled-trial median or the duration of a combined workflow. There is no global score across the eleven workloads.",
            "**Photos (asterisk in charts)** is an additional image-startup observation: five URI activation requests per fixture, potentially retaining a windowless spare process. It has a different process boundary from command-launched startup, and is not used to establish a paired cross-app significance claim.",
            "## Measurement method",
            "Startup launches an app with the media file; navigation, seek and file open operate in an already-running app. File open uses each app's direct file-entry interface. Windows file/folder dialogs and their derived timing endpoints are excluded throughout. Image startup and image file open therefore measure different operations.",
            "The generated corpus covers JPEG (including EXIF rotation and sRGB profiles), PNG/alpha, WebP, AVIF, TIFF, GIF/WebP animation; H.264, HEVC 10-bit, VP9, AV1 10-bit and long-GOP H.264; and WAV, FLAC, MP3, AAC/M4A and Opus. Homogeneous image folders, the 1,000-file folder and mixed-format sessions are distinct fixtures. Audio startup/open/navigation include both 12-second and 180-second files. These synthetic format fixtures are not a representative sample of photographs, movies or music.",
            "Files are warm and windows are normalized to an 800 × 650 client. Prepared profiles, fixture identity checks and owned-window checks are used; warmup/calibration observations are excluded. Measurements are sequential and operation order is randomized within blocks where the protocol permits. A fresh process block can contain several related operations: those operations are not independent samples. The tables preserve successful/attempted operations and block counts.",
            "Visual completion is desktop capture recognizing the expected fixture, including sampling/capture and window normalization overhead. It is not physical display onset. Audio completion detects the expected tone in two consecutive WASAPI loopback packets; it includes buffering and detector confirmation, and is not acoustic onset or sample-accurate timing. No microphone recording is used. Fit/resampling settings do not certify identical filtering or full-source image quality.",
            "The broad refresh uses three measured blocks, the retained direct-open study ten, qualified image startup thirty, and the targeted image-open follow-up thirty new paired blocks. Other qualified navigation/seek counts are listed below. Small cohorts describe speed but give unstable tail estimates. Table p95 values retain their source method (nearest-rank in the broad refresh; linear interpolation in the retained direct-open study). No aggregate p95 or aggregate confidence interval is inferred from these summaries.",
            "## Photos navigation",
            "Photos 2026.11080.24002.0 was measured in ten viewer sessions per fixture, plus one excluded warmup session: **880/880 measured operations succeeded** (968 including warmup). No Computer Use or additional automation software was used. Windows UI Automation shows/focuses the filmstrip before timing; native keyboard input performs next, previous, Home and End. Every timed target must match both its encoded desktop identity and window title. The app keeps its installed default profile and natural spare process; these are fresh viewer sessions, not a promise of a new isolated process for every block.",
            "In this tested URI-activation route, Photos followed filename order rather than the original folders' saved Shell order. The measurement therefore uses byte-identical independent copies named by original Shell position, with their own name-ascending Shell order. This preserves the exact image identities and content sequence used by the historical comparisons. Original images and their Shell settings are unchanged. These are separate cohorts and input routes (Photos native keyboard versus the earlier posted messages/menu commands); no paired significance claim is made against the historical apps.",
            "The Photos filmstrip is visible, with Fit display in the same 800 × 650 client. UI Automation inspects small-folder membership and the realized prefix of the large folder before timing; setup is followed by one second idle, then 100 ms between observed targets. No folder pre-traversal or UI Automation query is inside the adopted navigation measurements. Next/previous have 80 observations per fixture, and each supported boundary jump has ten; these observations share ten session blocks. The p95 uses nearest rank and remains a noisy tail estimate.",
            "**1,000-file limitation:** next/previous are measured, but first/last jumps are unavailable for this activation/input route. Calibration displayed intermediate files instead of the requested complete-folder boundary, even after separate preparation experiments. Those calibration observations are excluded, and no intermediate endpoint is presented as a successful jump. Other apps' first/last results remain in the full tables. The overview matches the **18 conditions** shared by all three apps across five fixtures; it does not average Photos over fewer actions than its competitors. This observation does not establish a universal Photos folder-size limit.",
            "Photos records the shortest observed median for 24 MP JPEG next/previous and the mixed-folder last-file jump. The eleven-workload headline concerns the aggregated values, not a clean sweep of every fixture/action.",
            "## Image-open confidence",
            "The latest six-condition follow-up used thirty new paired blocks per condition: all 540 measured operations succeeded; eighteen warmup operations and calibration are excluded. Earlier exploratory samples are not pooled. Mixed-folder targets in this follow-up are JPEG 1080p, AVIF and EXIF rotation only; they do not reproduce the entire older ten-format session.",
            "The gap is **towavue minus IrfanView**, computed within paired blocks. Its median is not the difference between the marginal medians in the bar charts. Exact order-statistic intervals use a six-comparison Bonferroni correction. Under independent, stable blocks, simultaneous family coverage is at least 95% (the discrete bound is 96.87%). An interval crossing zero is unresolved, not evidence of equivalent speed.",
            "| Condition | Median paired gap (ms) | Simultaneous interval (ms) | towavue lower blocks | Recorded-time conclusion |",
            "|---|---:|---:|---:|---|"]
    for row in data["open_confidence"]["comparisons"]:
        low, high = row["simultaneous_interval_ms"]
        doc.append(f"| {confidence_label(data, row)} | {row['median_paired_gap_ms']:+.2f} | [{low:+.2f}, {high:+.2f}] | {row['towavue_lower_blocks']}/{row['blocks']} | {row['classification']} |")
    doc += ["![Paired image file-open differences and simultaneous intervals](benchmarks/image-open-confidence.png)",
            "[Scalable confidence chart](benchmarks/image-open-confidence.svg)",
            f"Four conditions favor towavue in recorded completion timestamps; JPEG 24 MP and EXIF rotation remain unresolved. Capture timing adds a separate uncertainty: the median capture call took {data['open_confidence']['capture_median_ms']:.2f} ms. Conservatively propagating last-absent/first-present bounds leaves only AVIF entirely below zero; the other five capture-sensitivity intervals cross zero. The public data includes all thirty paired gaps and their lower/upper bounds for each condition. These statistical intervals do not remove systematic timing bias.",
            "## Complete results",
            f"All **{len(data['records'])} comparison cells** are retained below, including settled viewport, video soundtrack and playback cadence observations that are not in the eleven-workload overview. No towavue-only or Windows-dialog comparisons are included.",
            "**Table cells:** median / p95 in milliseconds; successful/attempted operations; process or session blocks; study ID. A lower–upper median is a capture-observation bound, not a confidence interval. Charts plot the upper observed median, with gray capture-bound segments where available. Build attribution is in the [study inventory](#study-inventory)."]
    doc += [" · ".join(f"[{title}](#{cat})" for cat, (title, _) in CATEGORIES.items())]
    for cat, (title, note) in CATEGORIES.items():
        doc += [f'<a id="{cat}"></a>', f"### {title}", note]
        for (group_cat, scenario, metric), rows in groups.items():
            if group_cat != cat:
                continue
            apps = [a for a in APPS if any(r["app"] == a for r in rows)]
            doc += [f"#### {SCENARIOS[scenario]} — {METRICS[metric].lower()}",
                    "| Fixture | " + " | ".join(APPS[a] for a in apps) + " |", "|---|" + "---:|" * len(apps)]
            for case in sorted({r["case"] for r in rows}):
                cells = []
                for app in apps:
                    r = next((r for r in rows if r["case"] == case and r["app"] == app), None)
                    if r is None:
                        cells.append("—")
                        continue
                    value = f"{r['median_ms']:.2f}"
                    if r.get("median_lower_ms", r["median_ms"]) < r["median_ms"]:
                        value = f"{r['median_lower_ms']:.2f}–" + value
                    cells.append(f"{value} / {r['p95_ms']:.2f}<br>{r['successful']}/{r['attempts']}; {r['blocks']} blocks; {r['study']}")
                doc.append(f"| {data['labels'][case]} | " + " | ".join(cells) + " |")
            stem = f"{cat}-{scenario}-{metric}"
            doc += [f"![{title}: {SCENARIOS[scenario]}, {METRICS[metric]}](benchmarks/figures/{stem}.png)",
                    f"[Scalable chart](benchmarks/figures/{stem}.svg)"]
    doc += ["## Environment and builds", "| Component | Measured environment |", "|---|---|"]
    for key, value in data["environment"].items():
        doc.append(f"| {key.replace('_', ' ').capitalize()} | {value} |")
    doc += ["| Application | Version |", "|---|---|"]
    doc += [f"| {APPS[app]} | {version} |" for app, version in data["apps"].items()]
    doc += [f"Product integration commit: [`{data['product_commit'][:7]}`](https://github.com/sheetau/towavue/commit/{data['product_commit']}). These are development-build measurements, not a claim that the latest published installer has identical performance. Improvements from qualified candidates are integrated, but not every historical cell was remeasured on the integrated executable.",
            "Build IDs below are **executable SHA-256 prefixes, not Git commits**. In each comparative study, the build identifies towavue; comparator versions are listed above.",
            "| Build | Attribution |", "|---|---|"]
    doc += [f"| `{build}` | {note} |" for build, note in data["build_notes"].items()]
    doc += ["### Study inventory", "Study IDs connect table cells to the public data. Evidence hashes identify the archived source summaries; private machine paths, profiles and raw captures are not published.",
            "| Study | towavue build / Photos version | Blocks | Endpoints |", "|---|---|---:|---|"]
    for study in data["studies"]:
        doc.append(f"| {study['id']} | {', '.join(study['builds'])} | {', '.join(map(str, study['blocks']))} | {', '.join(study['categories'])} |")
    doc += ["## Data and updates", "[Reviewed measurement data](benchmarks/data.json) · [Overview calculation details](benchmarks/overview.json) · [Update instructions](benchmarks/README.md) · [Measurement harness](../scripts/benchmark/README.md)",
            "This page, its figures and the README overview are generated from the reviewed public dataset. Archived measurement reports are retained as historical evidence; this page is the maintained publication. Updates replace measured conditions while preserving other comparator results and their original build/sample attribution."]
    # Keep Markdown table rows contiguous while separating prose and figures.
    output = ""
    for i, part in enumerate(doc):
        separator = "\n" if i and part.startswith("|") and doc[i - 1].startswith("|") else "\n\n"
        output += (separator if i else "") + part
    return output


def generate(data, root):
    validate(data)
    destination = root / "docs/benchmarks"
    items = overview(data)
    groups = defaultdict(list)
    for row in data["records"]:
        groups[row["category"], row["scenario"], row["metric"]].append(row)
    groups = dict(sorted(groups.items()))
    overview_chart(items, destination, data["updated"])
    confidence_chart(data, destination)
    for (cat, scenario, metric), rows in groups.items():
        detail_chart(data, rows, destination / "figures" / f"{cat}-{scenario}-{metric}")
    write(destination / "overview.json", json.dumps(items, indent=2, ensure_ascii=False))
    write(root / "docs/BENCHMARKS.md", report(data, items, groups))
    return [root / "docs/BENCHMARKS.md", destination / "overview.json",
            *sorted(destination.glob("*.png")), *sorted(destination.glob("*.svg")),
            *sorted((destination / "figures").glob("*"))]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Regenerate in a temporary directory and compare published artifacts")
    args = parser.parse_args()
    data = json.loads((REPO / "docs/benchmarks/data.json").read_text(encoding="utf-8"))
    if args.check:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            outputs = generate(data, root)
            changed = [str(p.relative_to(root)) for p in outputs
                       if not (REPO / p.relative_to(root)).exists()
                       or p.read_bytes() != (REPO / p.relative_to(root)).read_bytes()]
            if changed:
                raise SystemExit("Outdated artifacts: " + ", ".join(changed))
            print(f"Verified {len(outputs)} generated artifacts from {len(data['records'])} cells.")
    else:
        outputs = generate(data, REPO)
        print(f"Published {len(outputs)} artifacts from {len(data['records'])} cells.")


if __name__ == "__main__":
    main()
