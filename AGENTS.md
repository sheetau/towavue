# Working on towavue

towavue is a Windows media viewer/player in Rust.

## Find the relevant context

- Start ongoing work with `python .dashboard/app.py brief`; fetch relevant IDs, unanswered questions and changes since the last event cursor. Use [dashboard operations](.dashboard/README.md) for commands. Private data is authoritative; [STATUS](docs/STATUS.md) is a generated public checkpoint. If the local ledger is missing, use STATUS/Git as limited context and recover the private backup; do not initialize an empty ledger as if nothing were pending.
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
- The separately authorized local development dashboard is outside the product dependency constraint above. Keep changes scoped; preserve user edits. Make routine reversible decisions and proceed. Ask when a choice materially changes scope, data safety, or the accepted design.

## Verification and handoff

- Use checks proportional to the change; see DEVELOPMENT. Documentation-only changes need link/content/diff checks, not a Rust rebuild. Run affected regressions for code changes and the full suite for cross-cutting changes.
- Distinguish automated, offscreen, visible-window, and physical-input evidence. Skips are not passes; old measurements are not proof for a changed path.
- Update the local task/question/note when scope, state, answers or useful evidence changes; preserve original intent, record the next action and distinguish implementation, validation and release. Recheck owner edits/answers at work boundaries. Only act within the current authorized scope; planned/backlog work is not an instruction to start. Ask questions in chat, record answers with their source, and never treat a recommendation as consent.
- Keep the short project handoff current. Export STATUS only from the explicitly reviewed `public_summary` field; never copy private task text into tracked documents automatically. Record decisions/evidence once and link them. Archived notes are historical evidence, not current instructions.
- Change ARCHITECTURE only when a durable contract changes; DEVELOPMENT only when instructions change; README only when the product description changes. Do not copy a checkpoint into multiple documents.
- Use English for code, comments, diagnostics, tests, commits, this file, README and public summaries. Private owner-facing tasks, questions and handoff may use Japanese; retain original wording when migrating.
- Keep dashboard databases, backups, exports, private archives, generated media, build output, caches, local FFmpeg, and `concepts/` untracked. Respect `.dashboard/.gitignore`'s structural allowlist; never force-add personal records. Never commit secrets, machine-specific paths, or redistributable binaries without a distribution decision.
- Before every non-documentation push, run all `scripts/test-ci.ps1` phases locally and fix failures before pushing. Then verify CI on that exact commit; documentation-only pushes use the lighter checks above.
- Inspect the worktree before staging and after pushing. Commit coherent verified work; push when requested. Do not force-push, rewrite history, or discard unrelated changes.
