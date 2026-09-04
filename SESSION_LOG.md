# SESSION_LOG.md

This log preserves compact, factual continuity across sessions. New entries are added first.

## 2026-09-04 22:19 JST - implementation / complete M3 seek, synchronization, and resilience

- Trigger: M2 was complete and the owner requested milestone-by-milestone implementation under `AGENTS.md`.
- Intent: complete M3 without beginning the M4 application shell.
- Result: playback now uses generation-tagged transactional seek, audio-master and video-only pacing, continuous keyboard seek, pause/resume, late-video drop accounting, default-render-endpoint recovery, and typed D3D11 device-removal recovery. Runtime demux, video decode, audio decode, and WASAPI output run as separate workers behind bounded queues; independent audio drain and a running-clock monotonic floor prevent end-of-stream and underrun stalls.
- Architecture evidence: a shared decoded queue larger than the D3D11VA surface pool exhausted hardware surfaces, so it was reduced to two items. A first 30-minute run exposed demux head-of-line pressure and an EOF clock stall; bounded per-stream pending packets, independent audio completion, post-drain video clock selection, and the running-clock floor resolved both cases.
- Changed areas: core time/generation contracts, runtime queue and worker lifecycle, seek/discard decode, WASAPI clock and endpoint notifications, typed graphics recovery, app pacing/controls/metrics, architecture, roadmap, and README.
- Verification: `cargo fmt --all --check`, workspace all-target Clippy with warnings denied, and workspace all-target tests pass. Release hardware checks on adapter `00000000:0001311b`: H.264 D3D11VA and FFV1 software fallback both reached EOF; pause/resume returned from `Paused` to `Playing`; 100 local 1080p H.264 seeks completed with p95 37.750 ms and maximum 56.991 ms; 30-minute 4K60 H.264/AAC playback presented 107,768 frames, dropped 3 (0.0028%), and measured A/V drift p95 4.772 ms and maximum 35.759 ms.
- Hardware limitation: changing the owner's default audio endpoint and deliberately removing or resetting the active D3D11 device were not forced because those operations alter external machine state. Notification filtering and generation retention have unit checks; the pipeline replacement and graphics construction primitives used by recovery were exercised separately by repeated seek and hardware playback.
- Commit: `09b3761`.
- Status: `m3_complete`.
- Next action: begin M4 with the application shell and command model, then implement Shell-backed folder snapshots without parsing Explorer Bags.

## 2026-09-04 19:49 JST - implementation / begin M3 timing and seek substrate

- Intent: establish the M3 clock and generation primitives before changing worker lifecycle or user controls.
- Result: added saturating `MediaTime` arithmetic, `PlaybackGeneration`, source timestamps on decoded audio chunks, an `IAudioClock`-backed media position, audio-master video pacing, and decode entry points that seek in AV_TIME_BASE units and discard pre-target output.
- Scope note: generation-tagged worker replacement, continuous seek controls, endpoint/device recovery, frame-drop metrics, and M3 performance gates remain incomplete.
- Verification: workspace all-target `cargo check` passes; full M3 checks are pending.
- Status: `m3_in_progress`.
- Next action: make `PlaybackSession` restart its bounded decode/audio pipeline per generation, reject stale events, and expose transactional seek to the app.

## 2026-09-04 19:38 JST - implementation / complete M2 D3D11VA path

- Intent: add the M2 zero-copy hardware path without changing M1 controls, timing, or scope.
- Result: the renderer-owned D3D11 device is passed to FFmpeg through an owned `AVD3D11VADeviceContext` reference; runtime-only hardware frames are presented through `ID3D11VideoProcessor` on the same device, while the app sees only timestamps and typed events.
- Fallback: codecs or adapters that fail D3D11VA before the first hardware frame reopen through the verified M1 software path. Failures after hardware output begins remain session errors instead of being hidden by fallback.
- Evidence: adapter `00000000:0001311b` decoded and presented all 60 H.264 frames with `hardware_frames=60` and `cpu_transfers=0`. HEVC, VP9, and FFV1 were not hardware-capable on that adapter and each completed through software fallback with 60 CPU transfers.
- Changed areas: FFmpeg hardware-context ownership, opaque graphics-device sharing, runtime-only presentation frames, D3D11 Video Processor output, fallback selection, adapter/counter diagnostics, app presentation boundary, architecture, roadmap, and README.
- Checkpoint: pushed commit `aff9e34` to `origin/main`.
- Verification: local format, workspace Clippy with warnings denied, all-target tests, software codec fixtures, hardware H.264 EOF smoke, and software fallback smokes passed. GitHub Actions run `33864384769` independently passed formatting, Clippy, linking, and all portable tests on `windows-2022`.
- Status: `m2_complete_m3_not_started`.
- Next action: begin M3 with generation-aware seek and clock contracts before adding resilience cases or performance validation.

## 2026-09-04 19:18 JST - implementation / complete M1 software playback

- Intent: implement only the M1 single-window, single-file software playback vertical slice after the verified M0 checkpoint.
- Result: added FFmpeg 9.0.1 software video/audio decoding, RGBA D3D11 Flip Discard presentation, event-driven WASAPI Shared output, bounded runtime queues, Space play/pause, and EOF state after the final video frame and audio sample drain.
- Fixtures: generated and probed MP4/H.264/AAC, MKV/HEVC/AAC, and WebM/VP9/Opus from a checksum-pinned LGPL shared FFmpeg build; all three pass the decode integration test.
- Runtime verification: a real Windows playback run reached `Ended` with a responsive window; a separate 30-second run remained `Paused` until a second Space event and then returned to `Playing`.
- Changed areas: core media time/state values, runtime decode/audio/playback/renderer boundaries, app event loop, pinned dependencies, FFmpeg/fixture scripts, CI preparation, README, architecture, and roadmap.
- Checkpoint: pushed commit `d88b967` to `origin/main`.
- Verification: local format, workspace Clippy with warnings denied, all-target tests, three-codec fixture decode, EOF smoke, and pause/resume smoke passed. GitHub Actions run `33862770540` independently passed FFmpeg setup, fixture generation, formatting, Clippy, linking, and all tests on `windows-2022`.
- Status: `m1_complete_m2_not_started`.
- Next action: begin M2 with the single-device D3D11VA zero-copy path and retain M1 software decode only as capability fallback.

## 2026-09-04 18:22 JST - test / complete M0 foundation

- Result: completed the M0 repository foundation and stopped before M1; no playback, window, or Shell runtime behavior was implemented.
- Checkpoint: pushed initial commit `a707d59` to `origin/main` and established upstream tracking.
- Verification: formatting, Clippy, all-target `cargo check`, UTF-8/LF audit, ignore audit, and staged diff checks passed locally. GitHub Actions run `33857971429` passed formatting, Clippy, linking, and all tests on `windows-2022`.
- Environment note: the local Visual Studio installation has the MSVC linker but no Windows SDK libraries, so local `cargo test` cannot link until the SDK is installed. CI supplied the independent link-and-test result.
- Maintenance: replaced deprecated `actions/checkout@v4` with the exact `v7.0.1` commit after the first run reported its Node.js 20 deprecation.
- Status: `m0_complete_m1_not_started`.
- Next action: begin M1 only after an explicit request, starting with the software playback vertical slice defined in `docs/ROADMAP.md`.

## 2026-09-04 18:16 JST - implementation / M0 foundation

- Intent: establish the pre-application repository foundation only; do not begin playback, UI, or Shell runtime implementation.
- Result: initialized the Rust workspace and documented the accepted Windows, media, graphics, audio, licensing, workflow, and milestone boundaries.
- Requirement update: folder ordering means the actual per-folder Explorer Sort By state. The architecture now prefers a matching live `IFolderView2`, otherwise loads persisted Shell view state through a read-only hidden `IExplorerBrowser`; natural-name ordering is failure-only fallback.
- Changed areas: repository policy, licenses, three empty workspace crates, Windows CI, architecture, roadmap, and continuity documentation.
- Verification: pending M0 format, Clippy, tests, ignore audit, initial commit, push, and clean-worktree check.
- Status: `m0_validation_pending`.
- Next action: run the complete M0 checks and push the verified initial checkpoint to `origin/main`; then stop before M1.
