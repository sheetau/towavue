# Current work and handoff

<!-- Generated from the explicitly curated public summary. Do not edit directly. -->
Generated: 2026-09-28T20:45:41.505995+00:00. Public project revision: 3.

This repository uses a local project dashboard for tasks, questions and evidence. The reusable structure is versioned; personal records, backups and migration archives are excluded from Git.

## Product checkpoint

- The application version remains 1.0.2. The last recorded release-preparation checkpoint is the v1.0.2 draft at commit `c15235b1ee844e17f7fba6e2d3ae2044790faa3f`; this summary does not query live GitHub publication state.
- Subsequent local work includes the integrated silent VC prerequisite and Gallery/Filmstrip updates. No new release is created by the dashboard migration.
- Filmstrip uses fixed 120-point columns with 20-point gaps, keyboard-priority navigation and one zoomed selection. At `b256f55`, automated input/layout and hidden-GPU checks were recorded as passing; physical appearance approval is separate.

## Record-management tooling

- A private SQLite ledger, single HTML dashboard and bounded JSON CLI replace the growing task/handoff files. The Windows launcher starts a hidden loopback server. No product dependency was added.
- Ledger behavior tests, headless browser editing/conflicts/polling, backup/restore and deployment into a fresh project passed. Original records are preserved privately; only reviewed summaries are exported.

## Working references

- [Development](DEVELOPMENT.md): build and validation commands, including pre-push CI phases.
- [Architecture](ARCHITECTURE.md): accepted product contracts.
- [Releasing](RELEASING.md): packaging and release workflow.
- Previous status history remains in Git and in the private dashboard archive. New verification records belong to their task or note, not this generated summary.

Private tasks, questions, evidence and archives stay in the local dashboard. Run `python .dashboard/app.py brief` for current work, or see [Dashboard operations](../.dashboard/README.md). This export is a checkpoint, not the live task queue.
