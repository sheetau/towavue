# SESSION_LOG.md

This log preserves compact, factual continuity across sessions. New entries are added first.

## 2026-09-05 15:57 JST - implementation / compact shell and EOF replay

- Trigger: H1 visual audit found duplicate native/custom title bars, crowded status text, and a Play action that did nothing after EOF.
- Intent: establish the draft-aligned compact shell while retaining native window operations and existing edit/export guards.
- Result: replaced native decorations with a 32-logical-pixel title/tab bar and 30-pixel status bar, neutral colors, vector logo, equal-width bounded tabs, truncated names with full-path tooltips, and right-aligned metadata. Added native move/edge-resize/minimize/maximize controls and guarded close. Timeline, reading, and pause symbols are painted to avoid missing font glyphs. Shell fallback remains explicit; full order source is in the metadata tooltip. Play from Ended seeks to zero before resuming; Loading/Faulted remain unchanged.
- Changed areas: app main/chrome, core playback transition, architecture, README, roadmap, gap ledger, and trial guide. No new dependency, native handle exposure, or app/core unsafe code.
- Verification: focused transition/geometry tests, format, all-target Clippy with warnings denied, and all-target tests pass (app 13, core 26, runtime 33, integrations 3; the existing opt-in WASAPI-clock and live-Explorer tests explicitly ignored). Previous rate checkpoint c960065 independently passed CI 33950412111.
- Real-window evidence: image/video maximize and restore, title-area drag, edge resize to 480x300, minimize/recovery, two tabs with a long filename, tab activation, dirty close/Cancel/undo, and playback pause/resume passed. Two-second H.264/AAC replay reached EOF again with 60 hardware frames, 0 transfers, 0 drops, and 32.203 ms seek latency. Latest trial windows closed; media/captures/helper stay ignored under target/tmp.
- Boundary: an earlier intermediate layout trial lost visible chrome and later logged an audio-worker-stopped error; latest rebuilt trials did not reproduce either, so no independently established root-cause claim is made. Repeat transition stress and pause after audio drain in the launch audit. Multiple DPI/monitor and large-tab-count coverage remain open; menu gestures and tab reorder are deferred.
- Status: h1_active, not launch-complete. The audit also confirmed video still draws against the whole window and is covered at the edges by bars.
- Next action: correct video aspect-fit inside the actual media viewport on both hardware/software paths, then thin seek-bar interaction and remaining daily flows. Verify this checkpoint CI after push.

## 2026-09-05 15:38 JST - implementation / pitch-preserving live rate

- Trigger: H1 rate commands still changed only export, unlike the newly live volume controls.
- Intent: apply 0.25-4x rate to playback while preserving pitch, source-time Seek/timeline semantics, pause, and the single-device path.
- Result: enabled the pinned FFmpeg filter feature and added a streaming in-process stereo-f32 tempo filter. Two 0.5-2x atempo stages implement the range; unity is byte-preserving bypass and EOF drains the filter. Rate changes rebuild the existing generation-scoped pipeline at the current source position with retained pause/volume. Audio and video-only clocks scale by rate; deadlines divide by rate. Audio drain now hands position to the video clock, paused video checks frozen media time, and EOF freezes the display position.
- Changed areas: runtime tempo/audio/playback, app edit synchronization and clocks, dependency feature, README, architecture, roadmap, trial guide, and gap ledger. No native handles or new unsafe code were added to app/core.
- Verification: format, workspace all-target Clippy with warnings denied, and workspace all-target tests pass (app 12, core 25, runtime 33 plus 2 explicitly ignored device/Explorer tests, integrations 3). Tone tests check duration, 440 Hz pitch, stereo phase, unity, and drain. The opt-in WASAPI clock test was separately executed without skipping: 0.6 seconds advanced source time by 0.15/0.30/1.20/2.40 seconds at 0.25/0.5/2/4x; pause held position and resume advanced it.
- Real-window evidence: 30-second H.264/AAC at 2x reached EOF with D3D11VA, 0 CPU transfers, 850 frames and no drops after rate change; source-time drift p95/max was 9.019/35.380 ms. A 30-second video with only 5 seconds of audio continued through the silent tail at 4x to EOF (852 frames, 2 dropped, drift 28.900/68.240 ms). Silent-video pause held the same frame across a rate change and a two-second wait; paused Seek and resume worked. Trial windows closed; generated media/captures remain ignored under `target/tmp/`.
- Boundary: rate changes re-prime playback rather than provide gapless continuous tempo automation. Short trials do not establish a rate-by-codec/resolution performance matrix. Trim and video transform previews remain incomplete.
- Checkpoint evidence: volume `fcf499d` passed CI `33949631615`; PCM fix `86db48a` passed CI `33949749766`.
- Status: `h1_active`; the overall launch objective remains open.
- Next action: verify this checkpoint in CI, then implement the draft-aligned compact shell and daily interaction improvements, including the ineffective Play action at EOF, before the full launch audit.

## 2026-09-05 15:25 JST - fix / PCM WAV channel-layout compatibility

- Trigger: the live-volume trial's generated mono PCM WAV faulted with FFmpeg `Input changed` before playback.
- Intent: resolve the reproduced decode failure without hiding actual format changes or changing explicit speaker layouts.
- Result: decoded frames with unspecified layouts now receive the same channel-count-based default used to configure the resampler. The production change is confined to audio frame conversion.
- Verification: a new generated mono/stereo PCM regression failed before the fix and passes afterward for both serial and parallel decoding, checking 480 output frames, equal mono downmix channels, and exact stereo sample values. The original 48 kHz mono WAV now plays in the real app with a session peak of 0.088367; M reduces it to zero and the status identifies live volume. Format, all-target Clippy with warnings denied, and all-target tests pass (app 11, core 25, runtime 32 plus 1 explicitly ignored live-Explorer test, integrations 3).
- Changed areas: runtime audio decode, regression test, roadmap, trial guide, and removal of the resolved gap. Generated fixtures/captures remain ignored; trial windows closed.
- Status: `h1_active`. Volume checkpoint `fcf499d` is pushed; its CI run `33949631615` is still running.
- Next action: check checkpoint CI, implement pitch-preserving live rate with clock/seek verification, then continue the compact visual shell and launch audit.

## 2026-09-05 15:22 JST - implementation / live playback volume

- Trigger: H1's volume/mute commands changed edit history and export but not playback.
- Intent: make current volume audible without waiting for the decoded-audio queue, while preserving clocks and source data.
- Result: runtime applies stereo f32 gain immediately before each WASAPI write, with a 5 ms ramp and latest-only atomic target. Playback sessions retain volume through Seek/recovery; app supplies tab history before pipeline startup and synchronizes undo/redo. Volume feedback now distinguishes playback/export from the still-export-only rate command.
- Verification: sample regressions cover unity, stereo balance, split-buffer ramps, exact mute, 200% gain, and initially muted output. The real AAC trial's own WASAPI session meter measured approximately 0.0885 at 100%, 0.0442 at 50%, and zero after mute, including undo/redo, muted Seek, and pause/resume. No recording or endpoint/master-volume changes were used. Format, all-target Clippy with warnings denied, and all-target tests pass (app 11, core 25, runtime 31 plus 1 explicitly ignored live-Explorer test, integrations 3).
- Changed areas: audio output gain, playback lifecycle, app edit synchronization/feedback, architecture, README, roadmap, trial guide, and gap ledger. Trial windows closed; generated media and meter helper remain under ignored `target/tmp/`.
- Evidence: image checkpoint `f634c1d` independently passed CI run `33949287014`. A generated mono PCM WAV failed with FFmpeg `Input changed` before volume editing; stereo AAC played normally. This is a newly reproduced compatibility issue, not a successful WAV trial.
- Status: `h1_active`; rate, visual polish, and the launch audit remain incomplete.
- Next action: fix and regress the mono PCM channel-layout failure, then continue live rate and the compact visual shell.

## 2026-09-05 15:12 JST - implementation / responsive and bounded image loading

- Trigger: launch-quality H1 work continued with synchronous image and reading-page stalls.
- Intent: move decode off the event loop, bound retained frames, and reject stale results without changing Shell ordering or the single-device boundary.
- Result: one runtime image worker now keeps latest-only request/result slots with generation cancellation. A batch shares a 512 MiB retained-RGBA budget; over-budget animation fails instead of returning partial playback. Reading reuses the primary presentation, preserves failed-page positions, and reverses without decoding again. Loading and image errors remain visible. Renderer feature-level limits now initialize egui before the first texture; oversized textures return an error. External paths use the existing Shell-compatible canonicalization so relative CLI paths match folder snapshots.
- Evidence: the old build timed out a 1-second window probe and panicked on a generated 6000x6000 PNG because egui still had its initial 2048px limit. The new build displayed that image and answered decode-time probes in 7-8 ms. Real-window trials showed four Shell-ordered pages with two valid colors, a 17000x2 texture-limit error, and a corrupt PNG error; reversal preserved all positions. An invalid primary image also remained a visible reading slot. Captures and generated fixtures remain ignored under `target/tmp/`.
- Changed areas: runtime image decoder/worker, renderer limit, Shell path normalization, app image lifecycle and reading errors, README, architecture, roadmap, trial guide, and gap ledger.
- Verification: focused budget/cancellation, latest-request, path, and texture-limit regressions pass. Workspace formatting, all-target Clippy with warnings denied, and all-target tests pass (app 11, core 25, runtime 29 plus 1 explicitly ignored live-Explorer test, integrations 3). The preceding log checkpoint `ccc1ce6` independently passed CI run `33948359898`.
- Boundary: the budget does not cap total process memory, decoder scratch, old display buffers, or GPU textures. Codec work within a frame cannot always be interrupted; texture conversion/upload and Shell snapshot acquisition still run on the UI thread.
- Status: `h1_active`; this completes the image-loading slice, not the overall launch objective.
- Next action: verify the pushed checkpoint in CI, then address live volume/rate and compact visual interaction in separate H1 slices. Packaging still requires a distribution decision.

## 2026-09-05 14:43 JST - implementation / responsive and transactional export

- Trigger: the owner requested launch-quality stability, speed, interaction, and fidelity to the visual draft, while deferring optional features.
- Intent: continue H1 with the reproducible UI stall during Save and protect existing output during cancellation or failure.
- Result: runtime now owns one cancellable background export job with encoded-time events and bounded diagnostic capture. FFmpeg writes to a sibling staging directory; only a successful nonempty output replaces the target. App keeps playback and editing responsive, prevents leaving an exporting tab, persists export errors, and resumes a dirty guard only after successful export. History marks the exported operation prefix saved, preserving subsequent edits and replaced branches. Exporting audio playlists are protected from external-open reuse.
- Changed areas: runtime export lifecycle, app export progress/guards, core saved-history tracking, architecture, roadmap, README, trial guide, and known-gap ledger.
- Verification: format, workspace all-target Clippy with warnings denied, and workspace all-target tests pass (app 10, core 25, runtime 25 plus 1 explicitly ignored live-Explorer test, integrations 3). Real-window 1080p H.264/AAC 120-second trials reproduced the old 1-second response timeout; the new job responded in approximately 10 ms during encoding. Cancel and locked-target failure preserved target SHA-256 and removed encoder children/staging. Guard cancellation retained the dirty tab; successful guarded export closed it. Editing during export retained dirty state until undo returned to the exported revision. Local captures are retained under ignored `target/tmp/`.
- Checkpoint: pushed `0346eff` to `origin/main`; GitHub Actions run `33948060486` independently passed fixture generation, formatting, Clippy, linking, and all enabled tests on `windows-2022`. Trial-generated Downloads exports were moved into ignored `target/tmp/export-trials/`; trial windows were closed.
- Status: `h1_active`; this is one completed stabilization slice, not a launch-complete app.
- Next action: address image-load stalls, live playback controls, and the compact visual shell in separate verified H1 slices. Packaging remains a separate distribution decision.

## 2026-09-05 13:43 JST - planning / begin human evaluation and UX stabilization

- Trigger: the owner requested concrete instructions for trying and evolving the development build, a path-to-ownership map, and a complete account of current limitations and draft features not yet implemented.
- Intent: make M7 usable as a human-evaluation baseline without treating the untracked concept draft as an implementation contract.
- Result: added a trial and development guide covering setup, launch modes, supported extensions, Explorer Sort By verification, local data, a manual test matrix, the change loop, repository ownership, and UI/UX decision criteria; added a categorized gap ledger that distinguishes implemented, partial, unimplemented, unverified, and intentionally excluded behavior; and made H1 human evaluation and UX stabilization the active roadmap phase ahead of a separately authorized packaging decision.
- Changed areas: README, roadmap, developer guide, known-gap ledger, and continuity log only.
- Verification: UTF-8/BOM and whitespace checks passed; local format, workspace all-target Clippy with warnings denied, and workspace all-target tests pass (app 10, core 23, runtime 22 plus 1 ignored live-Explorer integration test, integrations 3).
- Commit: `a8f4a26`.
- Status: `h1_active`.
- Next action: run the baseline trial matrix, record the first reproducible high-impact UX problem, and implement one verified slice at a time.

## 2026-09-05 01:18 JST - implementation / complete M7 advanced presentation and interaction

- Trigger: the M6 checkpoint completed and the owner requested implementation of the remaining plan.
- Intent: complete M7 without weakening the single-D3D11-device boundary or claiming unsupported HDR behavior.
- Result: added a path/size/modified-keyed 64 MiB disk cache for FFmpeg-generated waveform and hover thumbnails; asynchronous duration, waveform, and thumbnail workers with stale-path rejection; a waveform timeline with hover preview and click/drag seek; per-media configurable 4×4 grid dispatch matching `1234/qwer/asdf/zxcv`; a 120 ms grid opacity transition; dirty-aware tab detachment to a new process window; and hardware-preferred H.264 export with forced Media Foundation hardware mode, automatic software fallback, and actual-path reporting.
- HDR boundary: PQ/HLG transfer metadata now remains attached to D3D11VA frames. The renderer queries the exact Video Processor input/output color-space conversion before setting `ID3D11VideoContext1` colorspaces. The reference adapter rejected PQ-to-SDR conversion, and the app produced the typed unsupported-conversion error instead of presenting unverified colors. Ten-bit HDR pass-through remains intentionally disabled.
- Changed areas: core command registry; runtime decode metadata, D3D11 color conversion gate, preview cache, and export outcome; app worker events, timeline, grid configuration, tab detachment, export controls, shortcuts; architecture, roadmap, and README.
- Verification: local format, workspace all-target Clippy with warnings denied, and workspace all-target tests pass (app 10, core 23, runtime 22 plus 1 ignored live-Explorer integration test, integrations 3). Runtime tests generate and reuse a thumbnail, generate a waveform and duration, validate HDR transfer classification, and inspect forced hardware-encoder arguments. The hardware export test explicitly skipped its hardware assertion because the reference adapter exposed no usable encoder, then verified the software fallback output. Real-window smokes showed the 4×4 grid, waveform timeline, cached hover thumbnail, and window-outside tab drop increasing the process/window count from one to two. GitHub Actions run `33894668084` independently passed fixture generation, formatting, Clippy, linking, preview/export tests, and all portable tests on `windows-2022`.
- Commit: `b4a4a26`.
- Status: `m7_complete`.
- Next action: no roadmap milestone remains; package/distribution work requires a separate decision about FFmpeg binaries and licensing.

## 2026-09-05 00:41 JST - implementation / complete M6 non-destructive editing and export

- Trigger: the independently verified M5 checkpoint completed and the owner requested continued milestone-by-milestone implementation.
- Intent: complete M6 edit history, guards, and software export without starting M7 HDR, cache, multi-window, or hardware-encode work.
- Result: added per-tab non-destructive crop, quarter-turn rotation, horizontal/vertical flip, trim endpoints, volume, and rate operations; branch-aware undo/redo and saved cursors; image UV-mesh preview; video crop selection; unsaved tab/window/status indicators; and blocking Export / Discard / Cancel guards for folder navigation, tab close, and process exit. Save As stores an export target and Save reuses it; neither mutates the source path.
- Export boundary: runtime converts operations into ordered FFmpeg video/audio filters, copies input metadata, decomposes audio tempo outside 0.5–2.0, chooses software codecs by output family, and rejects source-equal targets or invalid trim ranges. No FFmpeg or native handles leave runtime, and hardware encoding remains unimplemented.
- Changed areas: core edit operations/history and command contexts; runtime save dialog and FFmpeg export; app per-tab edit state, image preview, video selection, commands, indicators, guards, and shortcuts; architecture, roadmap, and README.
- Verification: local format, workspace all-target Clippy with warnings denied, and workspace all-target tests pass (app 8, core 23, runtime 18 plus 1 ignored live-Explorer integration test, integrations 2). An FFmpeg fixture test preserved a 40×30 source while exporting crop+rotate as 30×20; another exported and re-decoded H.264/AAC with trim, 2× rate, and 50% volume. A real D3D11 window showed clockwise image preview with dirty indicators, and WM_CLOSE displayed the blocking unsaved-edit modal instead of exiting. GitHub Actions run `33891124458` independently passed formatting, Clippy, linking, image export, video/audio export, and all portable tests on `windows-2022`.
- Commit: `2e6d0ed`.
- Status: `m6_complete`.
- Next action: begin M7 with measured presentation features; do not claim HDR until the output path and metadata are verified.

## 2026-09-05 00:14 JST - implementation / complete M5 images and reading mode

- Trigger: M4 was complete and the owner requested continued milestone-by-milestone implementation while preserving the actual Explorer Sort By order.
- Intent: complete M5 image presentation and reading mode without beginning M6 destructive or persisted editing.
- Result: added safe RGBA decoding for BMP, JPEG, PNG, TIFF, static/animated GIF, WebP, APNG, and AVIF; EXIF orientation; deadline-driven animation; fit, actual-size, cursor-anchored zoom, right-drag pan; normalized selection with edge resize, square creation, ratio-preserving resize; and pixel-preserving crop preview. Reading mode displays 2–10 current-and-following images from the shared Shell-ordered `FolderSnapshot`, with horizontal/vertical layout and visual-order reversal. Menu, palette, shortcuts, and status controls share the new command identities.
- Architecture evidence: the `image` crate's `avif` feature is encoder-only and its native decoder would add a system dav1d dependency. AVIF therefore uses the already pinned FFmpeg software boundary; other image formats use `image` 0.25.10. Runtime exports only owned dimensions, RGBA bytes, and frame durations, while app-owned egui textures use the existing D3D11 device/back buffer. Image frames do not enter the video decode queue or Video Processor path.
- Changed areas: core image/reading geometry and commands; runtime image decoding; app image textures, animation scheduling, interaction, reading layout, status and shortcuts; pinned dependencies; architecture, roadmap, and README.
- Verification: `cargo fmt --all --check`, workspace all-target Clippy with warnings denied, and workspace all-target tests pass (app 5, core 20, runtime 14 plus 1 ignored live-Explorer integration test, codec integration 1). Runtime tests decode every advertised non-AVIF static path and verify GIF frame pixels/timing. A locally generated AVIF decoded through FFmpeg and rendered correctly; real-window smokes confirmed fitted static image presentation, changing animated-GIF frames, and a two-page Explorer-ordered reading layout on the shared D3D11 surface. GitHub Actions run `33888539982` independently passed formatting, Clippy, linking, and all portable tests on `windows-2022`.
- Commit: `4c262c5`.
- Status: `m5_complete`.
- Next action: begin M6 with platform-independent non-destructive edit history and close guards before implementing mutation or export.

## 2026-09-04 23:34 JST - implementation / complete M4 application shell and navigation

- Trigger: M3 was complete and the owner requested continued milestone-by-milestone implementation under `AGENTS.md`, with Explorer ordering defined as the actual per-folder Sort By state.
- Intent: complete M4 without beginning M5 image rendering or later editing features.
- Result: added an egui application shell with tabs, status bar, shared command registry, searchable command palette, reloadable and prefix-capable shortcuts, same-folder audio playlist tabs, and an all-media filmstrip with explicit middle-click new tabs. Menu, palette, and keyboard input dispatch the same context-aware command identities.
- Explorer ordering: `FolderOrderProvider` runs on a dedicated Shell STA, prefers a matching live Explorer `IFolderView2` selected by foreground then recently observed window, and otherwise navigates a hidden `IExplorerBrowser` without a custom property bag. It captures view-order items, Shell identities, `PROPERTYKEY` sort columns, source, generation, and timestamp; only Shell failure uses logged Windows natural-name fallback. Overlapped `ReadDirectoryChangesW` with 150 ms debounce refreshes the shared snapshot, and the active item is remapped by Shell identity before canonical path.
- Rendering evidence: integrating egui exposed that the D3D11 Video Processor rejects the software fallback's RGBA texture on the reference adapter. Software frames now use a full-screen D3D11 shader on the same device/back buffer, while D3D11VA frames retain the zero-copy Video Processor path and UI is composed before the single Present.
- CI correction: the first pushed M4 run exposed a startup race in the folder watcher test: a change could occur before the worker armed its first `ReadDirectoryChangesW`. Construction now waits for an explicit ready handshake after the request is armed; the focused test passed 10 consecutive local runs before the corrected checkpoint was pushed.
- Changed areas: core media/command/navigation/tab contracts; Shell order provider and dialogs; directory watcher; D3D11 UI/software rendering; application tabs, commands, palette, shortcuts, playlist, filmstrip, and status; pinned dependencies; architecture, roadmap, and README.
- Verification: `cargo fmt --all --check`, workspace all-target Clippy with warnings denied, and workspace all-target tests pass (app 3, core 15, runtime 11 plus 1 ignored live-Explorer integration test, codec integration 1). The live Explorer fixture test was run explicitly and passed Name, Date modified, Date created, Size, and Type ascending/descending, ties, multiple columns, and repeated sort recapture; the hidden Explorer fixture and debounced directory watcher tests also pass. On adapter `00000000:0001311b`, the final H.264 smoke presented 60/60 D3D11VA frames with 0 CPU transfers, and FFV1 presented 60/60 software frames with 60 CPU transfers and no render error.
- Commit: `f8be5e4`.
- Status: `m4_complete`.
- Next action: begin M5 image decoding/rendering and reading-mode primitives; do not begin M6 editing while M5 is active.

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
