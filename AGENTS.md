# AGENTS.md

This repository contains towavue, a lightweight Windows image, video, and audio viewer and player written in Rust.

## Source of Truth

Before changing the repository, read these files in order:

1. `README.md` for the current project status and entry points.
2. `docs/ARCHITECTURE.md` for accepted technical decisions and boundaries.
3. `docs/ROADMAP.md` for the active milestone and its completion gate.
4. `SESSION_LOG.md` for the latest completed work and next action.

The local `concepts/` directory is untracked reference material. It is not a plan or a source of truth. Move accepted decisions into tracked documentation instead of citing the directory as an implementation contract.

## Language

- Use English for application code, identifiers, comments, diagnostics, tests, commit messages, `AGENTS.md`, and `SESSION_LOG.md`.
- Japanese is allowed in user-facing `README.md` and design or planning documents intended for the project owner.
- Keep comments limited to non-obvious intent, ownership, invariants, and unsafe contracts.

## Architecture Boundaries

- `towavue-core` is platform-independent and must forbid unsafe code and Windows or FFmpeg types.
- `towavue-runtime-windows` owns Windows APIs, FFmpeg integration, threads, queues, clocks, and all unsafe code.
- `towavue-app` owns the event loop, UI state, and command dispatch. It must not receive COM pointers, FFmpeg pointers, or native frame handles.
- Preserve the single-D3D11-device design. Decode, video processing, and presentation must use the same adapter and device unless an architecture decision explicitly replaces this rule.
- Default audio output is event-driven WASAPI shared mode. Exclusive mode is optional future work, not a default fallback.
- The decode fallback is D3D11VA to software. Do not add CUDA, QSV, or another hardware path without a measured need and an architecture update.
- Use the Windows Shell view state for folder ordering. Do not replace it with filename sorting or parse undocumented Explorer registry Bags.

## Project Structure

- `crates/towavue-core/`: platform-independent domain contracts.
- `crates/towavue-runtime-windows/`: Windows-specific runtime boundary.
- `crates/towavue-app/`: executable and UI orchestration.
- `docs/ARCHITECTURE.md`: durable architecture decisions.
- `docs/ROADMAP.md`: milestone gates and current milestone.
- `SESSION_LOG.md`: compact cross-session history.

## Change Rules

- Implement only the active milestone. Do not begin a later milestone because its code is nearby or convenient.
- State assumptions and surface high-impact ambiguity before implementation.
- Make the smallest change that satisfies the milestone gate.
- Do not refactor unrelated code or reformat unrelated files.
- Isolate each unsafe block behind a safe interface and document its ownership, lifetime, thread, and synchronization invariants.
- Pin toolchains and dependency versions. Commit `Cargo.lock` for the application workspace.
- Do not add an async runtime, database, plugin system, telemetry, or network access unless the active milestone requires it.

## Verification

Run focused tests first, then the complete M0 checks before a checkpoint:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

Hardware-dependent tests must report an explicit skip reason when the required device or capability is absent. A skip must never be presented as proof that the hardware path works.

## Continuity Log

- Read `SESSION_LOG.md` before work that may overlap earlier activity.
- Add an entry when requirements change, a milestone or meaningful subtask completes, tests reveal architecture-relevant evidence, or a checkpoint is pushed.
- Put the newest entry first.
- Record the local timestamp, trigger, intent, result, changed areas, verification, commit when already available, status, and next action.
- Keep entries factual and compact. Do not paste chat transcripts, raw command output, or information already clear from Git history.

## Git Rules

- Keep `concepts/`, local FFmpeg builds, build output, logs, and caches untracked.
- Check `git status --short` before staging and after pushing.
- Commit and push only a coherent checkpoint whose required checks pass.
- Do not force-push, rewrite history, use destructive resets, or discard user changes.
- Do not commit secrets, machine-specific absolute paths, generated media, or redistributable binaries without an explicit distribution decision.

## Done Criteria

- Every changed line traces to the active milestone or request.
- Relevant format, lint, test, and milestone-specific checks pass.
- Architecture and roadmap documentation match implemented behavior.
- `SESSION_LOG.md` states the result and next action.
- The worktree is inspected and the verified checkpoint is pushed when requested.
