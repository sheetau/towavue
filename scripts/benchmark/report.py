"""Create a local evidence report from completed visual and audio run summaries."""
import argparse
import json
from pathlib import Path

from analyze import LABELS


def table(rows, audio=False):
    output = ["| Fixture | App | Successful attempts | Median (ms) | p95 (ms) | Median bootstrap interval (ms) |",
              "|---|---|---:|---:|---:|---:|"]
    def bounds(row, prefix):
        if not row["successes"]:
            return "—"
        low, high = row[f"{prefix}_lower_ms"], row[f"{prefix}_upper_ms"]
        return f"{high:.1f}" if audio else f"{low:.1f}–{high:.1f}"
    for row in rows:
        interval = (f'{row["median_bootstrap_low_ms"]:.1f}–{row["median_bootstrap_high_ms"]:.1f}'
                    if row["successes"] else "—")
        label = LABELS.get(row["case"], row["case"])
        output.append(f'| {label} | {row["app"]} | {row["successes"]}/{row["attempts"]} | '
                      f'{bounds(row, "median")} | {bounds(row, "p95")} | {interval} |')
    return "\n".join(output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    root = args.root
    env = json.loads((root / "environment.json").read_text(encoding="utf-8"))
    visual = json.loads((root / "visual-final/summary.json").read_text(encoding="utf-8"))
    audio = json.loads((root / "audio-30/summary.json").read_text(encoding="utf-8"))
    checks = json.loads((root / "verification.json").read_text(encoding="utf-8"))
    if len(visual) != 35 or len(audio) != 15 or {row["attempts"] for row in visual + audio} != {30}:
        raise ValueError("This report template requires the complete 30-repeat pilot")
    total = sum(row["attempts"] for row in visual + audio)
    good = sum(row["successes"] for row in visual + audio)
    version_lines = "\n".join(f"- {name}: `{version}`" for name, version in env["app_versions"].items())
    gpu_lines = "\n".join(f"- GPU/display inventory: {gpu['Name']}; driver {gpu['DriverVersion']}; "
                          f"reported mode {gpu['CurrentHorizontalResolution']} x {gpu['CurrentVerticalResolution']} "
                          f"at {gpu['CurrentRefreshRate']} Hz." for gpu in env["gpu"])
    audio_warnings = checks['runs']['audio-30']['packet_warning_count']
    text = f"""# Command-launch media benchmark pilot

The CLI phase completed **{total:,} measured attempts**, of which **{good:,}** reached
the specified endpoint. Warmup attempts are stored separately and excluded.
This is a synthetic, warm-filesystem, new-process benchmark, not an overall
application ranking. RAW was excluded as requested.

## Environment

- Windows: {env['os']['Caption']} {env['os']['Version']}.
- CPU: {env['cpu']['Name'].strip()}.
- RAM: {env['physical_ram_bytes'] / 1024**3:.1f} GiB; system DPI observed after the run: {env['system_dpi_observed_after_run']}.
{gpu_lines}
- Power plan: Balanced; unchanged. Temperature was not instrumented or held
  at a controlled setpoint. No build or second measurement ran concurrently.
- towavue source commit: `{env['product_commit']}`; normal Release build.
{version_lines}

These are the installed versions, not necessarily the latest releases. Each
run records executable hashes, launch arguments and its own configuration.
Separate application profiles were used. System-wide decoder settings were
not reset. Matching decode quality and hardware backend were not established
for every app/codec. Existing sessions were not controlled by the harness;
window resizing and closing were restricted to its own fixture processes.

## Visual results

Each of 35 app/file pairs has 30 measured trials after one excluded warmup
block. Pair order was randomized within each block (seed 10404). Images and
videos use separate schedules. The original video trials revealed accumulated
towavue resume positions; **all video pairs for all apps** were rerun with
resume disabled/reset. Original records remain intact in `visual-30`; the
image batch was also rerun for all pairs, with explicit invisible-window
timestamps to tighten the observation bounds when the first capture already
contained the image. The selection manifest records both corrections. No
selection depends on speed or success. The target
client was 800 × 650; actual media viewport and scaling can differ by app.

Timing starts immediately before process creation. All 24 cells identifying
the requested fixture must be visible in a desktop capture of the owned window.
The intervals below span the last negative capture/visibility observation and
the first positive capture. Window normalization and initial window animation are included.
This proves recognizable content, not full-resolution quality or photon arrival.
In the static-image IrfanView cases, the first capture already contained the
image, leaving wider observation bounds (median width about 86–213 ms) than
most other cases (about 30–42 ms). Modest differences, such as 24 MP JPEG and
WebP, should not be treated as precisely resolved onset differences.

![Image measurements](visual-final/images.png)

![Video measurements](visual-final/videos.png)

{table(visual)}

## Audio results

Each of 15 app/file pairs has 30 measured trials after one excluded warmup block,
with randomized order (seed 10405). The default output endpoint was recorded
through stereo WASAPI loopback at 48 kHz. App volume was configured to 20%;
master output volume was unchanged. Gain controls were not loudness-calibrated.

The endpoint is completion of detection of two consecutive packets containing
the 997 Hz fixture tone. Numbers include buffering and detection confirmation.
They are **software loopback detection times**, not physical speaker onset.
No native sample timestamp was used to subtract buffering. A preexisting matching
tone invalidates the attempt. No microphone or raw sound recording was retained.
WASAPI discontinuity warnings are retained per packet; they limit conclusions
about exact onset and sample continuity, which this test does not establish.
The 450 selected audio trials had {audio_warnings} such warnings in their timed packets.

![Audio measurements](audio-30/audio.png)

{table(audio, audio=True)}

## Statistics and reproducibility

Latencies are conditional on success; failed attempts remain in the denominator.
p95 is nearest-rank and is uncertain with only 30 trials. The median interval is
a 95% percentile bootstrap interval (10,000 deterministic resamples); visual
interval endpoints also include the capture bounds. This is not a confidence
interval for the difference between apps or a proof of statistical significance.

- [Visual summary CSV](visual-final/summary.csv), [individual trials](visual-final/trials.csv),
  [full timestamp stream](visual-final/samples.jsonl), [selection metadata](visual-final/run.json).
- [Controlled image-run metadata](visual-images-30/run.json), [controlled video-run metadata](visual-video-30/run.json).
- [Audio summary CSV](audio-30/summary.csv), [individual trials](audio-30/trials.csv),
  [full timestamp stream](audio-30/samples.jsonl), [run metadata](audio-30/run.json).
- [Environment and packages](environment.json), [installed image/codec plugin versions](plugins.json).
- [Integrity and trial-count verification](verification.json).
- Corpus: `tests/generated/benchmark-v1/manifest.json` in the repository;
  SHA-256, byte size, stream metadata and generator commands for all 20 files.
- Reproduction scripts: `scripts/benchmark/README.md` in the repository.
  Calibration runs are kept separately and excluded from these results.

## Interpretation and remaining coverage

The image corpus uses procedural texture and a large identity marker. Videos
use static texture with a changing frame counter. There is only one file per
stratum. These data do not establish performance for general photographs or
cinema, and should not be promoted as a universal speed advantage. A precise
claim must name the fixture, environment, installed versions and measured endpoint.

Photos, already-running Open, sequential/random navigation, large-folder
operations, full-quality image completion, seek, playback pacing, waveform readiness, long audio,
multiple real-media files per stratum and reboot-based cold-cache measurements
remain separate work. They are not counted as passes in this report. The CLI
phase was prioritized in response to the owner's instruction.

The WASAPI loopback API and buffering caveats are described in the
[SoundCard documentation](https://soundcard.readthedocs.io/en/latest/).
The implementation and its pinned package source define the actual detector
used here; this reference is not a source of the measured performance numbers.
"""
    (root / "REPORT.md").write_text(text, encoding="utf-8")
    print(root / "REPORT.md")


if __name__ == "__main__":
    main()
