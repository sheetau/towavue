# External media benchmarks

These Windows-only development scripts compare command-launch latency using
generated fixtures. They are separate from the product and its dependencies.
No Computer Use connection is required. Only fixture processes started by the
harness are closed or resized; existing application sessions are left alone.

The original command-launch pilot and the interaction suite below use separate
corpora, output directories and measurement boundaries. Do not merge their
latencies into one application ranking.

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

## Interaction suite

Build the read-only Shell-order probe before timing:

```powershell
cargo build -p towavue-runtime-windows --example folder-order-dump --release --locked --offline
target/benchmark/venv/Scripts/python.exe scripts/benchmark/prepare_navigation.py --root tests/generated/benchmark-navigation
target/benchmark/venv/Scripts/python.exe scripts/benchmark/prepare_seek.py --root tests/generated/benchmark-seek --ffmpeg <path-to-ffmpeg.exe>
target/benchmark/venv/Scripts/python.exe -u scripts/benchmark/navigation.py --root target/benchmark/navigation-run --corpus tests/generated/benchmark-navigation --repeats 30
target/benchmark/venv/Scripts/python.exe -u scripts/benchmark/seek.py --root target/benchmark/seek-run --corpus tests/generated/benchmark-seek --repeats 10
target/benchmark/venv/Scripts/python.exe -u scripts/benchmark/folders.py --root target/benchmark/folder-run --corpus tests/generated/benchmark-navigation --repeats 30
target/benchmark/venv/Scripts/python.exe -u scripts/benchmark/quality_startup.py --root target/benchmark/quality-run --corpus tests/generated/benchmark-v1 --repeats 30
target/benchmark/venv/Scripts/python.exe scripts/benchmark/analyze_interaction.py target/benchmark/navigation-run
target/benchmark/venv/Scripts/python.exe scripts/benchmark/plot_interaction.py target/benchmark/navigation-run
target/benchmark/venv/Scripts/python.exe scripts/benchmark/prepare_interaction.py --root tests/generated/benchmark-interaction --corpus tests/generated/benchmark-v1 --ffmpeg <path-to-ffmpeg.exe>
target/benchmark/venv/Scripts/python.exe scripts/benchmark/navigation.py --root target/benchmark/mixed-run --corpus tests/generated/benchmark-interaction --groups mixed --repeats 30
target/benchmark/venv/Scripts/python.exe scripts/benchmark/media_navigation.py --root target/benchmark/media-run --corpus tests/generated/benchmark-interaction --repeats 30
target/benchmark/venv/Scripts/python.exe scripts/benchmark/pacing.py --root target/benchmark/pacing-run --corpus tests/generated/benchmark-seek --repeats 10
```

Run and inspect a one-repeat calibration for every runner before a full run.
Generation and compilation must finish before timing begins. The MPC-HC fixture
copy includes its bundled LAV decoders and MPC renderer directory; retain their
versions and effective settings. This corrects the original top-level-only copy,
whose results must not be assumed to represent the same decoder selection.

- **Navigation:** three 16-image groups (1080p JPEG, 24 MP JPEG, 4K PNG) and a
  1,000-image folder with unique 640x360 JPEG identities. Read the actual Shell
  order with the product's provider, and give IrfanView that exact file list.
  Do not assume the Shell sorts by filename. Fresh process per block/group,
  one-second initial idle, eight next/eight previous steps, first/last jumps and
  two random native-dialog Opens. Native keys/menu messages start the clock;
  Open starts at dialog submission, after path entry. Wait 100 ms after observing
  each target. This permits production prefetch and is not unlimited-repeat
  throughput. Steps share process/cache state, so bootstrap whole blocks.
  Native Open also brackets when the owned dialog HWND stops being visible,
  and when that HWND is destroyed, then reports the remaining time to content
  from each boundary with observation bounds. This
  separates dialog acceptance/closure from subsequent display without assuming
  that HWND visibility measures COM return or compositor fade completion.
  Keep the full confirmation-to-content result alongside this narrower metric.
- **Display settings:** towavue keeps default HQ/Fit; IrfanView uses explicit
  Fit-to-window and Resample, verified by its native menu state. These labels
  do not certify identical filters or color processing. Actual marker dimensions
  and representative viewport screenshots remain evidence.
- **Folder Open:** towavue's native folder picker is compared descriptively with
  selecting the first Shell-ordered file in another folder in IrfanView. These
  are different commands and are labeled separately, not a like-for-like win.
- **Seek:** moving 30 fps clips encode a unique 24-cell identity in every frame.
  Every 10% of the 12-second audio has a different tone. Video completion requires
  a captured frame at/after the requested timestamp and within one second; audio
  requires two matching loopback packets. Retain the actual frame number, packet
  timing/warnings and dispatch duration. towavue uses its compact seekbar,
  MPC-HC its documented slave API, and VLC its RC interface on an ephemeral IPv4
  loopback port (Windows quiet RC does not consume stdin). This is native command
  response, not physical input latency. The common 1080p master is upscaled for
  4K encodes; it is a controlled seek fixture, not representative cinema.
  The aligned runner requests each percentage plus 1/60 second, inside its tone
  interval and halfway between 30 fps frame boundaries. Map towavue's click
  through the 5-DIP endpoint inset using the owned window's DPI and the manifest
  container duration (subtract Matroska start time, matching the product).
  Record the estimated clicked timestamp and half-pixel bound; reject geometry
  that can select a different next frame. Numeric APIs receive the common target.
  Native clicks remain quantized, so timestamps are not bit-identical. Results
  use `seek_aligned` and must not be merged with historical `seek`: the old runner
  used the full client width and sometimes sought before the detector's tone or
  frame target, adding subsequent playback time to the reported latency.
- **Settled viewport:** the startup runner separately reports recognizable
  identity and convergence to the same app's eventual displayed pixels. It checks
  stability and keeps pixel error scores. This is not proof that every original
  source pixel is decoded, nor a cross-app quality-equivalence test. Animation is
  excluded from this convergence endpoint.
- **Mixed media and long audio:** the additional fixture generator copies the
  original ten image/animation and five video fixtures into separate folders,
  then generates identifiable 12-second and 180-second audio in five formats.
  Each audio file has a distinct frequency. The media runner supplies the Shell
  order to explicit MPC/VLC playlists and measures four forward/four backward
  moves plus five Opens per session, visiting every format. towavue uses Alt+arrows and native Open;
  MPC uses native file-skip commands and its Open API; VLC uses RC. These are
  application-command comparisons with different transports and playlist models.
  The audio startup runner also accepts these frequencies and an explicit
  `--cases` list. Use `--kinds video` with the original corpus to measure its
  embedded 997 Hz audio onset separately from recognizable video onset. These
  are separate process trials, not a simultaneous A/V synchronization measure.
  The ordinary video sequence allows towavue's intra-session position restoration,
  while MPC/VLC resume is disabled; revisits are therefore not position-matched
  decode comparisons. Use `--groups video --open-fresh` for a separate already-running
  Open comparison that launches an isolated seed copy and visits each real target
  exactly once. The first target differs from the seed identity. Keep this result
  separate from natural next/previous/revisit behavior.
  Retain actual profile values; an invalid preference file is
  not evidence that a requested volume was applied.
- **Playback pacing:** capture five seconds of progressing frame identities per
  session. Report observed frame-change intervals and long observation gaps.
  Capture can miss a displayed frame, so source-index jumps are not decoder
  drop counts. These observations do not establish physical display cadence.
- **Waveforms:** the runtime `waveform-cost` example measures native preparation
  and cache reuse on short/long generated audio. It is an internal boundary,
  separate from playback onset or screen display. Competitors without an
  equivalent waveform view have no comparable endpoint; report N/A.
  For the complete shared short/long audio corpus, build the Release
  `waveform-fixture-cost` example and run `waveform.py --root <new-output>
  --corpus tests/generated/benchmark-interaction --repeats 30`. It times a new
  preview cache, a memory hit, a reopened disk hit and timeline detail separately.
  Run `--repeats 0` as calibration first. The source bytes are warmed before the
  native process starts; process launch is outside these internal API timers.
  Inputs and cache roots are explicit, input stamps stay unchanged, cache pixels
  must match the initial result, and repeated per-file pixel/detail hashes must
  remain stable. Cache files and logs are retained. These checks do not establish
  identical waveform rendering to another application.

All runners use unique output roots and preserve unsuccessful attempts. The
desktop fixture windows are temporarily topmost and use native commands, so
foreground-activation denial cannot leave them behind the invoking application.
Only retained benchmark processes receive this window setting and are closed
after their trials; no owner window or persistent preference is changed. Capture
ownership checks still reject occlusion. Window placement is part of startup
timing, not product behavior. The application preparation helper snapshots the
Python sources before each run.
Record executable hashes separately when diagnosing a build, and retain the
matching executable/PDB before replacing an instrumented build. The
interaction summary includes per-scenario median, nearest-rank p95, success
counts, and a deterministic whole-block bootstrap interval. Setup errors remain
separate from operation outcomes; a setup failure is never a successful trial.
Desktop sampling bounds and statistical uncertainty are different quantities.
Photos activation, genuine OS-cold starts, physical scanout, and full-source
quality certification need their own evidence and must not be inferred here.
