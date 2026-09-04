# SESSION_LOG.md

This log preserves compact, factual continuity across sessions. New entries are added first.

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
