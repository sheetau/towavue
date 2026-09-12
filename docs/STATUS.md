# Current work and handoff

Last consolidated: 2026-09-12. This is the only current-status and handoff document. Update rows in place; detailed history stays in Git. [Development](DEVELOPMENT.md) provides setup, checks, and historical lookup; [Architecture](ARCHITECTURE.md) provides accepted contracts.

## Scope and next action

The owner's goal is to finish the missing functionality, operations, and UI from the local concept/follow-up drafts, with accepted tracked decisions taking precedence. The goal is not complete. M0–M7 foundations are implemented; the active work is UX/functionality improvement, not another launch-preparation milestone.

Documentation consolidation is complete locally: README is an English product introduction and development status is centralized here. The next feature work is in the backlog below; the previous code checkpoint finished separate-poster APNG export. A useful next bounded task is remaining animation-preserving export or posterless APNG OVER rounding, with a failing fixture before a fix. Do not recreate already completed APNG work.

Publication, signing, and release recertification are deferred. An eventual assisted per-user installer is the accepted format, but not authorization to publish now. Windows 10 physical testing is optional; no installation into the owner's environment for that purpose. The owner permits visible-window interaction and clipboard use for scoped work, not unrelated writes or input.

## Handoff

- Code checkpoint: **2d72e75**, separate-poster APNG editing/export. Local verification: 699 tests passed (app 391, core 67, runtime 237, integration 4), 22 ignored; fmt, Clippy, Release passed. These are historical results, not checks of future code.
- No build or owned media job was left running at that checkpoint. No distribution work should be resumed merely because old logs list it as next.
- Native automation previously failed window activation twice. No successful physical-key/wheel result was obtained from that attempt. Do not repeatedly retry the same failure or bypass the supported tool through private input injection; pursue independent code/tests until tool state changes.
- Build setup and alternate FFmpeg prefixes belong in DEVELOPMENT/environment, not machine-specific paths here.

## Remaining work

IDs retain traceability to the old UX ledger. “Implemented” describes the main path, not a claim that every device/input combination was tested. The shared validation row applies to all relevant entries; do not duplicate it as a new requirement on every checkpoint.

| ID | Current state | Remaining work / completion evidence |
|---|---|---|
| U01 | Native caption, borders, fullscreen/monitor restoration implemented; 96/192 DPI scenarios tested | Quantify window-drag latency; remaining native glyph/background/system-menu/Snap and DPI combinations |
| U02 | Native file picker and ordinary unsaved confirmation; stateful forms remain egui | Long-content/focus/scale/UIA edge cases, without rewriting all dialogs |
| U03 | Figtree/Codicon/Japanese fallback and tabular numbers; Welcome font fixed | Final font/icon coverage as UI changes |
| U04 | Grayscale styling, tab insets, left-anchored clock, status notifications, async file size, shadows implemented | Remaining media states, physical resizing/drag jitter, shadow quality/GPU cost and layout audit |
| U05 | Export progress on toolbar boundary, including unknown duration/normalization | Targeted visible/input/DPI confirmation; no navigation progress flashing |
| U06 | Welcome, recent 40 paths, async thumbnails, picker focus return | Remaining physical-input/accessibility combinations; no session/unsaved-backup feature implied |
| U07 | Tab image/transport/focus/scroll preservation, background audio, retained video, content-aware dirty state | Large edited saved snapshots/animation comparison cost; all-tab resource budget, recovery/endpoint failure combinations and full UI response |
| U08 | Same-host launch forwarding, live tab split/merge, filmstrip independent Open, monitor/DPI placement | Remaining DPI ratios/media/codecs/Explorer input and transfer latency; no recovery of legacy independent-process live state |
| U09 | Context menu, close groups, path copy/Explorer, path-only reopen; keyboard entry tested | Remaining focus/UIA and menu/tooltip lifetime paths |
| U10 | Shared/deduplicated previews, first-frame preview/prefetch, video sheets, immediate hover | Cold/other-format latency, GPU texture sharing where useful, all preview lifetimes and peak costs |
| U11 | Image inversion outline; accessible edge focus/status; time selection has its own updated style | Remaining physical/DPI/focus combinations; preserve hit areas and keyboard adjustments |
| U12 | Compact seek endpoints and hover progress; CTI triangle-only drag | Remaining physical mixed-DPI verification |
| I01 | Fit/Cover implemented and tested | Retain regressions; no standalone implementation left |
| I02 | Joined reading pages, first-spread offset, count/axis/order controls, dirty/edit guards | Remaining physical mixed-DPI continuous navigation |
| I03 | Cancellable decode, prefetch handoff, staged previews, sequential full-image presentation | Actual rapid keys, cold/other formats, all 100 images visible, IrfanView comparison and browsing peak resource reduction |
| I04 | Image aliases, bounded numbered jumps, Shell order and reading navigation | Keyboard layout/IME/custom binding physical checks |
| I05 | Edited-pixel clipboard, resize/resample and display-only interpolation | Resize-specific GPU recovery/mixed-DPI cases; avoid treating viewing interpolation as a saved edit |
| I06 | Aspect presets, quarter/free rotation, image/video UI and reading aliases | Full-format/HDR quality and large-animation performance; native rotation/selection interactions |
| I07 | Immediate event-position image wheel zoom; old full image retained during navigation; last-tab cleanup | Physical latency/continuous load; no full-process memory cap or browsing-peak improvement has been proved |
| I08 | Bounded pan, scrollbars, vertical/Shift horizontal wheel, Grabbing | Visible controls, non-active OS wheel delivery, UIA and mixed-DPI input |
| I09 | Corner/ratio resize, selection movement, outside clear, selection zoom replacing crop preview | Native gesture combinations and video-corner quality |
| V01 | Up-drag opens video timeline; dedicated button and timeline hover thumbnail removed | Preserve gesture/context regressions; further timeline work is V03 |
| V02 | Shared video sheets, single fallback and main-view scrub with commit/cancel | Long-GOP/all-codec quality, first-response distributions, mixed-DPI and peak load |
| V03 | Time selection, range playback, Delete/Keep, gain/stretch, Undo/Redo and export connected | Long deleted-span decode cost, initial-seek sample phase, atempo seams/tail, exact final UI/export and real-media quality |
| V04 | Viewing/edit context, J/K/L, actual-PTS video stepping, 10ms audio stepping, temporary 2× hold | Long-GOP responsiveness, real-media fine seek/hold transitions and remaining native input |
| V05 | Video zoom/pan, resize/resample, same-device raster and export connected | All-format/HDR resampling quality, sustained GPU cost and native/mixed-DPI checks |
| A01 | Auto-next, repeat/shuffle, white active row, async visible durations, volume HUD | Real-media duration/row/HUD verification and OS/DPI wheel behavior; gapless and restart-persistent modes are not implemented |
| A02 | Always-visible audio timeline and shared selection/gain/stretch/range playback | Final operation audit and V03 audio-quality work |
| E01 | Audio-only output, normalization/channel options, metadata UI, PNG/JPEG/static WebP fields, full supported APNG export including posters | GIF/WebP/AVIF animation-preserving export and conversion policy; posterless APNG OVER rounding; full 16-bit/ICC; AlbumArtist/EXIF/IPTC/COM/unknown or Extended XMP and cross-format metadata |
| M01 | Direct fixed-position File/Edit/View menus and directional logo gesture | Maximized/edge/DPI/focus cases; preserve normal keyboard/menu access |
| G01 | Shared command/guard/Shell infrastructure and non-active wheel gates | Targeted real OS keyboard/mouse/wheel/IME/UIA, correct cursors, overlay/hover ownership and failure recovery across the changed paths |

No marker/text/color-correction feature is added by this backlog. The draft's `-#` file-search/“>” command-palette proposal is explicitly thought-only; do not implement it without a new decision. Do not substitute CUDA/QSV, default Exclusive audio, registry Bags, telemetry, plugins, or a new framework.

## Evidence worth reusing

Keep results that prevent redoing an investigation. Find exact test names/conditions in the cited commit or the historical lookup described in DEVELOPMENT. Do not rerun old long benchmarks without a relevant change or new question.

| Topic | Evidence / location | Limit or next decision |
|---|---|---|
| APNG frame loss | c17b0a3 reproduced three frames saved as one; independent-PNG assembly preserves controls/payload | Do not return to the old single-image export path or assume native APNG encoder disposal is equivalent |
| APNG PREVIOUS | 994f0a9: explicit 2×1 BACKGROUND→PREVIOUS golden exposes stale canvas restoration in image 0.25.10; runtime compositor fixes it; one-frame/Adam7 tests pass | Existing `export_png_metadata_tests.rs` covers first/consecutive PREVIOUS, palette/gray/16→8 conversion, preview/budgets/cancel; full color fidelity remains open |
| Separate APNG poster | 2d72e75: FFmpeg cannot demux the valid partial-first-animation-frame fixture directly; stream runtime-composited poster + animation through one FFmpeg job | Actual edits, text Keep/Set/Remove, exact pixels/controls, resave, corruption/I/O/cancel/source-change target protection tested; extra disk/encoding cost |
| Dirty image comparison | 1f47ef7 exact all-frame comparison; 5adb349 shares materialized display Arc | 4096×2304 generated nearest down/up: two Release runs, comparison median ~48→1.1ms, observed peak commit ~207→171MiB; materialization ~46ms unchanged. Not GUI or all-animation timing |
| Timeline dirty state | a59f9c9 effective settings; 3d1449a exact duration-aware intervals | Preserve conservative behavior for unknown duration, integer rounding and different source ranges; Undo history stays |
| Image resource cleanup | 494cd5e clears last-image-tab caches; 638d5db empty-host delayed Trim; 7fc0b7c visible-host 100-image run | Two generated 4096×2304 JPEG runs: ~1.02s idle, GPU ~303→17MiB, private ~959→132MiB, reopen ~44/50ms. Explicit navigation, not physical keys/cold reading; peak remains unaddressed |
| Image transfer/order | Existing sequence tests cover old-image retention, each completed presentation, queue cancellation and three-scale pixel checks | Offscreen/batched commands are not evidence that 100 physical-key images were all visibly inspected |
| Long-GOP Seek | 2026-09-09 historical DEVELOPMENT: 120s 1080p single-keyframe fixture p95 ~873–979ms; FFmpeg-alone first frame ~836–936ms; 2s-keyframe control ~40–108ms | 300ms target missed on long GOP; not proof of a UI-only bug or absence of optimization opportunities |
| Long playback | Same historical Release: 30min 4K, 107771 frames, drop/CPU transfer 0, drift p95 4.812ms/max 17.349ms | One build/machine/fixture, not all-media stability or a current-build certificate |
| Native automation | Historical activation failure after refresh/retry; prior native UIA tests had separate successful sessions | Check changed tool state before retry; do not relabel injected/offscreen input as physical verification |
| FFmpeg provenance | BtbN development binary rejected for transitive GPL FFTW; native rebuild uses audited alternative/scoped ZVBI | Preserve source/license audit records in packaging references. LGPL labels or old candidate hashes do not certify a new distribution |

## Recent checkpoints

Keep at most a few meaningful handoffs here; older details are in Git.

- **2026-09-12 — documentation consolidation (uncommitted):** merged roadmap, UX backlog, known gaps and session handoff here; replaced chronological architecture/development text with current contracts/procedures and made README an English product introduction. Eight main documents became five, approximately 2.43 MB → 40 KB. All 31 legacy requirement IDs retained; 186 local Markdown links/anchors and diff checks passed. Old evidence is recoverable at 2d72e75; exact packaging provenance remains separate. No code/CI changes or unrelated Rust/installer checks. Next: review/commit this documentation checkpoint, then resume the feature backlog.
- **2d72e75 — APNG poster export:** implemented and verified as described above; next is remaining animation/color work, not publication.
- **994f0a9 — APNG compositor:** correct PREVIOUS restoration and allow single-frame saves; preserve golden regression evidence.
- **5adb349 — image comparison reuse:** remove redundant current-side rendering; keep saved-side comparison and conservative dirty state.
