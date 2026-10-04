# External command-launch pilot

These Windows-only development scripts compare command-launch latency using
generated fixtures. They are separate from the product and its dependencies.
No Computer Use connection is required. Only fixture processes started by the
harness are closed or resized; existing application sessions are left alone.

## Setup and execution

Use Python 3.14 and the pinned packages in `requirements.txt` in a local virtual
environment. Keep the environment, media and results under ignored directories.
IrfanView, MPC-HC (K-Lite installation) and VLC must already be installed. Build
the normal towavue Release executable using [DEVELOPMENT](../../docs/DEVELOPMENT.md).
The scripts use the repository's pinned development FFmpeg location. They do
not download applications or distribute their binaries.

```powershell
python -m venv target/benchmark/venv
target/benchmark/venv/Scripts/python.exe -m pip install -r scripts/benchmark/requirements.txt
target/benchmark/venv/Scripts/python.exe scripts/benchmark/prepare.py --root tests/generated/benchmark-v1 --ffmpeg <path-to-ffmpeg.exe>
target/benchmark/venv/Scripts/python.exe -m unittest discover -s scripts/benchmark -p 'test_*.py' -v
target/benchmark/venv/Scripts/python.exe -u scripts/benchmark/visual.py --root target/benchmark/visual-run --corpus tests/generated/benchmark-v1 --repeats 30
target/benchmark/venv/Scripts/python.exe scripts/benchmark/analyze.py target/benchmark/visual-run
target/benchmark/venv/Scripts/python.exe -u scripts/benchmark/audio.py --root target/benchmark/audio-run --corpus tests/generated/benchmark-v1 --repeats 30
target/benchmark/venv/Scripts/python.exe scripts/benchmark/analyze.py target/benchmark/audio-run
```

Run a one-repeat calibration before a full run. Run measurement phases
sequentially, without concurrent builds, downloads or other benchmarks. Avoid
covering fixture windows during visual measurements. Audio runs briefly play a
quiet test tone; master output volume is not changed. Use a new output directory
for each run. Existing measurement logs are never overwritten by the runners.

Record OS/build, CPU, GPU/driver, display mode/DPI, power plan, exact application
and plugin/decoder versions, product commit, configuration and Python packages
alongside `run.json`. Keep profiles with the results. A separate profile is not
proof that all system-wide codec settings are at factory defaults.

## What is measured

- Each randomized block attempts every eligible application/file pair once.
  One complete warmup block is retained but excluded from summaries. Each
  measured trial launches a fresh process after warming the file's bytes.
  The fixture towavue resume history is cleared between trials; MPC file/audio
  resume is disabled in the portable profile; VLC uses `--qt-continue=0`.
- Visual timing starts immediately before process creation and ends at desktop
  capture of the correct 24-cell fixture identity in the owned window. The
  client is normalized to 800 by 650 pixels. Bounds span the last absent capture/visibility observation
  and the first successful capture. Sampling, normalization and window animation
  are included. Capture is not a measurement of photons reaching the display.
- Audio timing ends when two consecutive stereo WASAPI loopback packets match
  the fixture's 997 Hz tone. It includes buffering and detector confirmation.
  Equal lower/upper values identify this software observation timestamp; they
  do **not** establish precise acoustic onset. A preexisting matching tone
  invalidates that attempt. The recorder is explicitly restricted to loopback;
  no microphone or raw sound recording is retained.
  Packet discontinuity warnings remain in the timestamp stream; do not interpret
  this detector as sample-accurate output timing or a continuity/dropout test.
- Input corpus and application executable hashes are retained. Output includes
  append-only `samples.jsonl`, flattened `trials.csv`, per-pair `summary.csv`,
  JSON summaries and PNG/SVG charts. Failures remain in success denominators.
  Latency summaries are conditional on successful trials.
- Median bounds, nearest-rank p95 bounds and a deterministic 10,000-resample
  percentile bootstrap interval for the median are reported. For visual results,
  the bootstrap endpoints also incorporate capture bounds. A p95 from 30 trials
  is a noisy tail estimate; do not imply narrow precision.

## Scope and limits

The 20-file corpus contains JPEG, PNG/alpha, WebP, AVIF, TIFF, EXIF rotation,
sRGB ICC, GIF/WebP animation, H.264, HEVC 10-bit, VP9, AV1 10-bit, long-GOP H.264,
WAV, FLAC, MP3, AAC/M4A and Opus. RAW is excluded. Visual markers and fine texture
are procedural; video is a static texture with a changing frame counter. These
are format/measurement fixtures, not representative photographs or cinema.
One file per stratum is insufficient for general claims about a format.

Equal client bounds do not establish equal media viewport, scaling filter or
full-resolution quality. This pilot measures recognizable content only.
Application defaults may use different decoders and hardware acceleration.
Do not infer a decoder backend from a correct picture. The audio gain knobs
are not an acoustic loudness calibration.

Photos, already-running application Open, folder navigation, sequential/random
navigation, image quality completion, seek, playback pacing, waveform readiness and genuinely cold
OS-cache trials are separate measurements. Do not label this pilot as covering
them or describe it as an overall application speed ranking. Public comparisons
need workload-specific wording and the method, exact versions, success counts
and relevant limitations alongside the chart.
