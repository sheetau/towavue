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

[CI](../.github/workflows/ci.yml) retains full Windows checks; this documentation change does not alter CI policy. There is no requirement to repeat those checks locally for prose-only edits.

## Locate code and tests

| Area | Entry points |
|---|---|
| Commands, time, history, geometry | `crates/towavue-core/src/` |
| Window, media, graphics, Shell, export | `crates/towavue-runtime-windows/src/` |
| UI, workers, tab state | `crates/towavue-app/src/` |
| Native image renderer patch | `vendor/egui-directx11/`, [patch notes](../vendor/egui-directx11/TOWAVUE-PATCH.md) |
| Generated media and specialized checks | `scripts/`, `tests/`; find the relevant test with rg |

Tests are colocated with implementations or under the crate's tests directory. Search for the behavior/test before creating another fixture or diagnostic.

User settings live under `%APPDATA%\towavue` (shortcuts.conf, grid.conf, recent-files.txt). Preview disk cache is under `%LOCALAPPDATA%\towavue\preview-cache`. Build output, `tests/generated`, and local FFmpeg are ignored. Never clear user settings or media to make a test pass.

For bug reports retain reproduction, expected/actual result, build, Windows/GPU/driver/DPI, and codec/dimensions/duration. Use disposable generated media, not private files. For Explorer ordering include Sort By, whether Explorer was open, and reported snapshot source.

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
