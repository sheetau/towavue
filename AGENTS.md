# Working on towavue

towavue is a Windows media viewer/player in Rust.

## Context and scope

- Start with `python .dashboard/app.py brief`, then fetch relevant IDs/events. The private ledger is authoritative; [STATUS](docs/STATUS.md) is a public checkpoint. Recover a missing ledger from backup rather than initializing an empty backlog; STATUS/Git provide limited fallback context.
- Follow the owner's current scope; backlog items are not authorization. Recheck owner edits at work boundaries. Ask questions in chat and keep answers in the corresponding thread; recommendations are not consent.
- Use [DEVELOPMENT](docs/DEVELOPMENT.md) for commands, relevant [ARCHITECTURE](docs/ARCHITECTURE.md) sections for contracts, and [dashboard operations](.dashboard/README.md) for records. Read packaging documents only for packaging work. Accepted contracts supersede `concepts/` drafts.

## Product constraints

- `towavue-core`: platform-independent types; no unsafe, Windows or FFmpeg types. `towavue-runtime-windows`: native APIs, workers and all unsafe code behind safe interfaces; document non-obvious ownership, lifetime and synchronization invariants. `towavue-app`: event loop/UI/dispatch; no COM/FFmpeg pointers or native frame handles.
- Preserve one D3D11 device, D3D11VA → software decode fallback, event-driven WASAPI Shared and Windows Shell view ordering.
- Pin dependencies/toolchains and retain Cargo.lock. Do not incidentally add frameworks, async runtimes, databases, telemetry, services or plugins. The authorized development dashboard is separate from the product.

## Records and verification

- Manage ordinary tasks by state. AI implementation completion is `review`; only actual owner confirmation makes it `done`. Corrections and further questions use `thread` on the same ID and reopen it. Create another item only for independent work.
- Do not require per-task next actions, acceptance prose, verification summaries or conclusions. Add notes only for useful decisions, caveats, interrupted work or reusable evidence; record shared validation once. Keep the project handoff brief and current.
- Run proportional checks (DEVELOPMENT): content/link/diff checks for docs, dashboard tests/browser checks for the dashboard, full suites for cross-cutting product changes. Distinguish automated/offscreen/physical evidence; skips are not passes.
- Update ARCHITECTURE for durable contracts, DEVELOPMENT for procedures, README for the English product introduction. Export STATUS only from reviewed `public_summary`; never automatically publish private records or duplicate evidence across documents.
- Use English for code, comments, diagnostics, tests, commits and public documentation. Private owner-facing records may use Japanese.
- Keep private dashboard data/backups/archives, generated media, build output, caches, local FFmpeg and `concepts/` untracked. Respect the dashboard allowlist. Never commit secrets, machine paths or redistributable binaries without a distribution decision.

## Git and handoff

- Preserve owner edits; inspect before staging and after pushing. Commit coherent verified work and push only when requested. Do not force-push, rewrite history or discard unrelated changes.
- Before every non-documentation push, run all `scripts/test-ci.ps1` phases locally and fix failures; then verify CI on that exact pushed commit. Documentation-only pushes use the lighter checks above.
- After a successful push and its required CI/checks, run `python .dashboard/app.py checkpoint --label <pushed-commit>`. This excludes older owner-confirmed completions from progress while retaining unfinished/review tasks and all history. Do not reset at local commits or failed pushes.
