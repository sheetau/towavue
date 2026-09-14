# Development

Use this guide for setup and checks, [STATUS](STATUS.md) to resume work, and the relevant [ARCHITECTURE](ARCHITECTURE.md) section for design constraints. Product usage belongs in [README](../README.md).

## Build and run

Windows x64, Visual Studio's Desktop development with C++ workload, Windows SDK, LLVM/libclang, Git, and PowerShell are required. Rust and components are pinned by [rust-toolchain.toml](../rust-toolchain.toml); dependencies by [Cargo.lock](../Cargo.lock).

From Developer PowerShell at the repository root:

```powershell
$ffmpegDir = .\scripts\setup-ffmpeg.ps1
$env:FFMPEG_DIR = $ffmpegDir
$env:LIBCLANG_PATH = Join-Path $env:ProgramFiles 'LLVM\bin'
$env:PATH = (Join-Path $ffmpegDir 'bin') + ';' + $env:PATH
cargo run -p towavue-app -- 'path/to/media.mp4'
```

The setup script retrieves the checksum-pinned BtbN development build. **That build is not a distribution candidate:** its transitive FFTW linkage was rejected. For the native Windows rebuild and scoped ZVBI candidate, use the packaging references below. Do not introduce WSL.

To use an already prepared compatible FFmpeg prefix, set `FFMPEG_DIR` to its root and add its `bin` to this shell's PATH instead. Use `CARGO_TARGET_DIR` to isolate incompatible native build variants; do not put personal absolute paths in tracked documentation.

Helper lookup is deliberate: colocated ffmpeg.exe/ffprobe.exe take precedence; only a development layout with neither colocated helper uses FFMPEG_DIR/bin. A partially installed pair fails rather than borrowing another version from PATH.

## Verification by impact

| Change | Expected checks |
|---|---|
| Documentation only | Review diff, local links/anchors and claims; no Rust build, codec fixtures, installer tests, or native UI session |
| Local code change | Relevant regressions, rustfmt, affected-package Clippy; broaden if shared behavior is affected |
| Cross-crate/runtime contracts, dependencies, broad checkpoint | Full workspace checks below; Release when optimized behavior or an executable is relevant |
| UI/input/rendering | Relevant layout/input tests and targeted visible verification when needed; distinguish injected/offscreen tests from actual OS input |
| Performance/device behavior | Comparable representative measurements; record fixture, build, conditions, limits |
| Packaging | Affected packaging checks; installation, signing, and publication require their own scope |

```powershell
.\scripts\generate-m1-fixtures.ps1
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo build -p towavue-app --release
```

Generate fixtures only when missing or their inputs change. Reuse completed checks for an unchanged tree; after a failure fix, rerun affected checks rather than restarting unrelated tests. Poll an existing build/process to completion; a tool timeout is not process failure. Do not run all ignored tests indiscriminately: some need hardware, local media, or long measurements. Skips remain unverified capabilities.

For Shell apartment diagnostics, add `--features towavue-runtime-windows/shell-lifecycle-verification` to a targeted app test. `SHELL_APARTMENT` stderr lines report PID, native thread ID, elapsed microseconds, phase and the current [CoGetApartmentType](https://learn.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-cogetapartmenttype) result (`hr` is the query result, not the Shell call). Type 3 is the main STA; type 0 is another STA. The trace neither initializes COM nor retains its objects and emits no paths, but logging changes timing. It covers Shell worker initialization/closure and path parsing, not every COM user or process shutdown. The feature is off by default. Preserve a native failure's matching executable/PDB/dump before rebuilding; compare lifecycle evidence rather than rerunning until green.

For the isolated `audio_playback::tests::audio_buttons_fit_small_windows_and_accessibility_invokes_shared_commands` and `tab_transfer::tests::image_transfer_rebinds_current_pixels_and_preserves_animation_edits_and_view` controls, use the app-level `--features shell-lifecycle-verification` and set `TOWAVUE_SHELL_TEST_LIFETIME` to `drain` or `anchor`. Both wait up to ten seconds for instrumented provider work/apartment cleanup after the existing test body; `anchor` additionally holds a verified main STA with a message-aware loop, without parsing paths or prewarming Shell factories, then verifies its cleanup. No variable means the original test lifetime. These controls require an isolated process with no earlier providers; completion is not an OS-thread/DLL-shutdown guarantee. Do not enable an anchor across ordinary tests merely to hide a crash or use this blocking verification wait in the UI.

[CI](../.github/workflows/ci.yml) retains full Windows checks; this documentation change does not alter CI policy. There is no requirement to repeat those checks locally for prose-only edits.

## Locate code and tests

| Area | Entry points |
|---|---|
| Commands, time, history, geometry | `crates/towavue-core/src/` |
| Window, media, graphics, Shell, export | `crates/towavue-runtime-windows/src/` |
| UI, workers, tab state | `crates/towavue-app/src/` |
| Native image renderer patch | `vendor/egui-directx11/`, [patch notes](../vendor/egui-directx11/TOWAVUE-PATCH.md) |
| PNG row-hook patch | `vendor/png/`, [source, scope and checks](../vendor/png/TOWAVUE-PATCH.md); preserve the pinned version and upstream unsafe prohibition |
| Generated media and specialized checks | `scripts/`, `tests/`; find the relevant test with rg |

Tests are colocated with implementations or under the crate's tests directory. Search for the behavior/test before creating another fixture or diagnostic.

For source-waveform cost, run `cargo run -p towavue-runtime-windows --example waveform-cost --release --features waveform-verification --locked --offline`. It generates owned one-second/three-minute PCM and AAC, alternates native/historical CLI overview order using independent empty preview caches, and separately times edited-timeline refinement and warm cache hits. Both overview paths include identical PNG/cache handling and must match full pixels; input length/mtime and repeated results are checked. System file caches are warm; no UI/GPU, cold-disk or peak-process-memory claim is implied. The CLI backend exists only with this verification feature. Fixture files are removed on success and retained on failure.

For isolated image-color experiments, `cargo run -p towavue-app --example image-color-cost --release --locked --offline` uses the same conversion source as the app and checks every output pixel against egui. It generates its own inputs by default; an explicit `TOWAVUE_COLOR_REFERENCE_PATH` instead selects one read-only static file, checking its length/mtime and emitting no paths or pixels. Conversion timings exclude decode, GPU work and equality checks; they do not replace full navigation measurements. Alternative comparison strategies are verification-only; the default app entry uses packed conversion. This small target avoids rebuilding the application test binary for each conversion experiment.

Set `TOWAVUE_COLOR_INTEGER_TRIAL=1` for current-packed/historical-lookup/egui/egui/historical-lookup/current-packed batches. The historical lookup control retains its opaque-row shortcut; production uses opaque blocks. The example's ordinary Release tests cover every component/alpha pair and generated row boundaries; full image equality is outside timed conversions.

Set `TOWAVUE_COLOR_PARALLEL_TRIAL=1` (without the integer option) for current/serial-initialized/two-way/two-way/serial-initialized/current batches instead of the default block-conversion comparison. Both candidates use the same row-opacity algorithm but retain historical lookup conversion; two-way splits disjoint output rows between one scoped worker and the caller. This is no longer a pure parallelism comparison against current production. Timings include output initialization, thread creation and joining. Calling-thread CPU diagnostics apply only to the current strategy, not the parallel worker. Run the example's ordinary Release tests without `--ignored` for generated split/alpha boundary equality checks.

Set `TOWAVUE_COLOR_SCALAR_TRIAL=1` for current-packed/historical-integer-rows/historical-integer-rows/current-packed batches. Production converts four RGBA pixels at a time on x64, with scalar tail/fallback and no extra worker or image-sized scratch buffer. Run its exhaustive component/alpha and unaligned-tail tests with `cargo test -p towavue-runtime-windows --release --locked --offline image_color` before timing; the example tests also check row boundaries and the instrumented app entry.

The reference navigation test accepts `TOWAVUE_NAV_LOOKUP_COLOR=1` for the former serial lookup conversion, `TOWAVUE_NAV_PARALLEL_COLOR=1` for the historical two-way control at every size, or `TOWAVUE_NAV_SCALAR_COLOR=1` for historical integer rows. All default to `0` (current packed conversion); enabling more than one is rejected. The former `TOWAVUE_COLOR_SSE2_TRIAL` and `TOWAVUE_NAV_SSE2_COLOR` options are obsolete: packed conversion is now the default, and the new scalar controls select the previous implementation. Compare modes on the same Release test binary, with other settings fixed and no concurrent build. These opt-ins are compiled out of the product. App color wall time includes allocation and, for parallel mode, worker creation/join; calling-thread CPU excludes the worker. Preserve order/source-stamp gates and compare whole-process memory and navigation latency, not just the selected conversion stage. Pixel equality belongs in the shared helper/entry tests and isolated example, not inside timed navigation.

Use `cargo test -p towavue-app --example image-color-cost --release --locked --offline -- --ignored --nocapture` to include the app harness's `cfg(test)` diagnostics. With an explicit static reference, `TOWAVUE_COLOR_CONCURRENT_DECODE=1` adds one looping same-source decoder; this is a stress control, not the real neighbor sequence. The reference navigation harness also accepts `TOWAVUE_NAV_IDLE_FRAME_MS` (default 0; 16 is the measured comparison): only event-free redraws are delayed, never commands or completion-event draws. Neither setting represents physical input or the native redraw scheduler.

For opt-in PNG decoder comparisons, run `cargo run -p towavue-runtime-windows --example png-decoder-cost --release --features png-decoder-verification --locked --offline` with an explicit read-only static `TOWAVUE_PNG_REFERENCE_PATH`. `TOWAVUE_PNG_BACKEND` selects `wic` (default), `wic-bgra`, or `libpng`; the latter requires `TOWAVUE_LIBPNG_DLL`, an absolute path to a trusted x64 libpng 1.6.58 DLL with its dependencies beside it. Loading a DLL executes native code: use the audited development copy, not an arbitrary download. Generated RGB/RGBA controls and the reference must match full pixels without printing them; timings exclude equality and decoder-service setup. These experimental backends are not used by the app and are not certified for its full format/cancellation/color contracts. Results belong only in STATUS.

The ignored `image_navigation::performance_tests::reference::reference_folder_reports_unpaced_completion_under_fixed_rate_commands` test requires `TOWAVUE_NAV_REFERENCE_DIR` and Release/hardware D3D11. It reports exclusive whole-loop phase totals and each phase's longest interval/start/progress, including loading and held frames, plus first-original presentation time. Window/device/app construction precedes its command clock; this is not end-to-end process startup or CPU-active time. Select one loader trace with `TOWAVUE_NAV_TRACE_INDEX` (zero-based lexical source index, even for reverse trials) or `TOWAVUE_NAV_TRACE_LARGEST_PNG`, never both. No reference pixels or filenames are emitted; measurement does not flush system caches or simulate physical input. `TOWAVUE_NAV_DECODE_CACHE_MIB` compares decoded-cache budgets (1–512 MiB, default 384) only before this test's first load; it does not change production defaults, entry/neighbor counts, texture limits or the per-canvas limit. Compare latency and memory together before considering a product-policy change. `TOWAVUE_NAV_DIRECTIONAL_PREFETCH` accepts `1` (default, production eight-ahead/one-back policy) or `0` (historical balanced comparison), without changing the nine-entry count or queued-step priority; combine with an explicit decoded budget to isolate lookahead versus capacity. This override exists only in the app test binary.

The same example's `stages`, `stages-sse2` and `stages-serial-paeth` modes isolate inflation/parsing, row reconstruction and packing for noninterlaced RGB/RGBA8 on x64, using png's opt-in benchmark APIs. They retain an additional full filtered canvas for measurement; do not substitute them for the production row decoder. Run the kernel's exhaustive/generated checks with `cargo test -p towavue-runtime-windows --example png-decoder-cost --release --features png-decoder-verification --locked --offline` before reference timing. Never print pixel buffers on equality failure. `stages-sse2` uses the current runtime kernel; `stages-serial-paeth` retains the historical per-pixel SSE2 control. The ignored `repeated_pixels_report_row_cost` test compares flat, mixed-run and noisy generated upper rows with resets and exact equality outside timing. These are row-kernel controls, not navigation or whole-process memory measurements.

User settings live under `%APPDATA%\towavue` (shortcuts.conf, grid.conf, recent-files.txt). Preview disk cache is under `%LOCALAPPDATA%\towavue\preview-cache`. Build output, `tests/generated`, and local FFmpeg are ignored. Never clear user settings or media to make a test pass.

For bug reports retain reproduction, expected/actual result, build, Windows/GPU/driver/DPI, and codec/dimensions/duration. Use disposable generated media, not private files. For Explorer ordering include Sort By, whether Explorer was open, and reported snapshot source.

Selected reference traces and the opt-in color-example test also report calling-thread kernel+user CPU accounting from [GetThreadTimes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getthreadtimes). These counters exclude other workers and GPU execution; their accounting granularity does not support exact wall-minus-CPU wait durations. CPU sampling is verification-only; ordinary color conversion leaves it disabled.

The reference navigation harness accepts `TOWAVUE_NAV_PARALLEL_PREFETCH=0` for the historical serial policy; unset or `1` uses production's bounded one-image lookahead. Compare disabled/default/default/disabled on the same Release binary, with cache/directional policy fixed and no concurrent builds. Include order, blanks/previews, memory, cancellation and source-identity checks; isolated paired-decode throughput does not prove navigation latency. This override is verification-only, not a user preference; the runtime contract is in ARCHITECTURE.

For initial-open ordering with real Shell notifications and large-image workers, run `cargo test -p towavue-app --bin towavue --locked --offline native_folder_notifications_preserve_initial_large_image_burst -- --ignored --nocapture --test-threads=1`. It generates 100 owned 4096×2304 JPEGs using the configured FFmpeg, queues 100 Right shortcuts before presentation, handles real worker events and derives expected navigation from the returned Shell order. It requires hidden-window hardware D3D11; inspect PASS/SKIP and the reported order source, not just the test exit code. Debug is sufficient for correctness, not latency comparisons; elapsed time includes fixture generation. The separate `native_initial_large_images_preserve_every_accepted_step` opt-in retains deliberately delayed scripted order as a control; tiny real/scripted-order variants run normally. Successful runs remove their isolated fixtures; failures retain them.

## Documentation maintenance

STATUS is the only current-work/handoff record. Update rows in place rather than appending a chronology. Retain new evidence when it changes the next decision or prevents repeated work; include a commit/test reference and its limits. Small documentation corrections need not create status entries.

ARCHITECTURE describes current durable contracts, not every implementation step. DEVELOPMENT describes repeatable procedures, not dated trial reports. Source/license records retain their own exact provenance; do not refresh them for unrelated UX work.

Pre-consolidation evidence remains in Git at [2d72e75](https://github.com/sheetau/towavue/tree/2d72e75001375f31130e027d8d1532f04f99bf80). Search only the relevant investigation:

```powershell
git show 2d72e75:SESSION_LOG.md | Select-String -Pattern 'APNG|PREVIOUS' -Context 2,5
git show 2d72e75:docs/DEVELOPMENT.md | Select-String -Pattern 'long GOP' -Context 2,5
```

The old README, architecture, roadmap, UX ledger, and known-gaps snapshots are recoverable at the same commit. They are historical evidence, not current instructions. There is no duplicate archive tree to read or maintain.

## Packaging references — only when needed

Publication is deferred; this index is not authorization to resume it.

- [DISTRIBUTION](DISTRIBUTION.md): packaging decisions and audit entry point.
- [LOCAL_SETUP](LOCAL_SETUP.md): per-user install/update/rollback/uninstall contracts and lifecycle evidence.
- [NATIVE_FFMPEG_BUILD](NATIVE_FFMPEG_BUILD.md), [FFMPEG_REBUILD](FFMPEG_REBUILD.md), [MABS_BUILD](MABS_BUILD.md): native Windows FFmpeg options, reproducibility, rejected inputs.
- [CANDIDATE_MATERIALS](CANDIDATE_MATERIALS.md), [APP_MATERIALS](APP_MATERIALS.md), [NATIVE_MATERIAL_CATALOG](NATIVE_MATERIAL_CATALOG.md): candidate/source/notice correspondence and specialized audits.
- [VC_REDIST](VC_REDIST.md), [INSTALLER_FIXTURE](INSTALLER_FIXTURE.md): prerequisite handling and isolated installer checks.

These records describe particular candidates. Rebind source, binaries, notices, and evidence before using them for a new release; their presence does not certify the current executable.
