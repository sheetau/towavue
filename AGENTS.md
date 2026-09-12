# Working on towavue

towavue is a Windows media viewer/player in Rust.

## Find the relevant context

- For ongoing feature work or a handoff, read [docs/STATUS.md](docs/STATUS.md): scope, remaining work, and evidence that prevents repeated investigations.
- For build commands and verification, use [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).
- For ownership, rendering, playback, editing, or Shell changes, read the relevant section of [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
- README is an English product introduction, not a development log. Packaging references are routed from DEVELOPMENT; read them only for packaging work.
- Do not read every document before every edit. Accepted tracked contracts supersede untracked `concepts/` drafts; the owner's current request sets the task scope.

## Project constraints

- `towavue-core`: platform-independent domain types; no unsafe, Windows, or FFmpeg types.
- `towavue-runtime-windows`: native APIs, FFmpeg, workers, queues, clocks, and all unsafe code behind safe interfaces. Document non-obvious ownership/lifetime/thread/synchronization invariants.
- `towavue-app`: event loop, UI state, command dispatch; no COM/FFmpeg pointers or native frame handles.
- Preserve one D3D11 device, D3D11VA → software decode fallback, event-driven WASAPI Shared, and Windows Shell view ordering.
- Pin dependencies/toolchains and retain Cargo.lock. Do not add a framework, async runtime, database, telemetry, network service, or plugin system incidentally.
- Keep changes scoped; preserve user edits. Make routine reversible decisions and proceed. Ask when a choice materially changes scope, data safety, or the accepted design.

## Verification and handoff

- Use checks proportional to the change; see DEVELOPMENT. Documentation-only changes need link/content/diff checks, not a Rust rebuild. Run affected regressions for code changes and the full suite for cross-cutting changes.
- Distinguish automated, offscreen, visible-window, and physical-input evidence. Skips are not passes; old measurements are not proof for a changed path.
- Update STATUS in place when scope, remaining work, or handoff evidence changes. Keep only a few useful recent checkpoints; Git is the detailed history.
- Change ARCHITECTURE only when a durable contract changes; DEVELOPMENT only when instructions change; README only when the product description changes. Do not copy a checkpoint into multiple documents.
- Use English for code, comments, diagnostics, tests, commits, this file, README, and handoff notes. Owner-facing planning/design may use Japanese.
- Keep generated media, build output, caches, local FFmpeg, and `concepts/` untracked. Never commit secrets, machine-specific paths, or redistributable binaries without a distribution decision.
- Inspect the worktree before staging and after pushing. Commit coherent verified work; push when requested. Do not force-push, rewrite history, or discard unrelated changes.
