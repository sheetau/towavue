# SESSION_LOG.md

This log preserves compact, factual continuity across sessions. New entries are added first.

## 2026-09-04 18:16 JST - implementation / M0 foundation

- Intent: establish the pre-application repository foundation only; do not begin playback, UI, or Shell runtime implementation.
- Result: initialized the Rust workspace and documented the accepted Windows, media, graphics, audio, licensing, workflow, and milestone boundaries.
- Requirement update: folder ordering means the actual per-folder Explorer Sort By state. The architecture now prefers a matching live `IFolderView2`, otherwise loads persisted Shell view state through a read-only hidden `IExplorerBrowser`; natural-name ordering is failure-only fallback.
- Changed areas: repository policy, licenses, three empty workspace crates, Windows CI, architecture, roadmap, and continuity documentation.
- Verification: pending M0 format, Clippy, tests, ignore audit, initial commit, push, and clean-worktree check.
- Status: `m0_validation_pending`.
- Next action: run the complete M0 checks and push the verified initial checkpoint to `origin/main`; then stop before M1.
