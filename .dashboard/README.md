# Local project dashboard

Python 3.11+ and a browser are sufficient; no package installation or build step is required. This development tool is independent of the product, release pipeline, and application dependencies.

Open it on Windows with `powershell -NoProfile -File .dashboard/open.ps1`. The launcher reuses the project's local server or starts a hidden process, then opens the browser. The server listens only on `127.0.0.1`; the page updates from the ledger without being regenerated. Closing the browser does not stop the server. Its owned PID and URL are in the ignored `runtime.json`; stop only that matching process when needed. A server restart creates a consistent SQLite backup. No AI is launched by this tool.

## Private records and public structure

`state.sqlite` is the authoritative local record. The folder's allowlist `.gitignore` excludes databases, SQLite sidecars, exports, backups, archives, logs, screenshots and migration scripts. Only the HTML, Python backend/tests, launcher and this guide are tracked. Never force-add a private artifact.

After a fresh clone, recover the private ledger from backup. A missing ledger does **not** mean the project has no pending work. Only a genuinely new project should run `python .dashboard/app.py init --name "Project name"`. Without a private ledger, use the public STATUS checkpoint and relevant Git history; do not fabricate or automatically reset tasks.

The UI provides tasks, questions, history and an archive; clicking a row opens its detail without replacing the list. New ideas default to inbox. Scope is separate from status: planned work in another release is not authorization to start it. A waiting/blocked task needs a reason. Tasks may link dependencies, prerequisite questions and acceptance checks. Starting/completing rejects unmet prerequisite questions/dependencies; completion also requires its checklist. An answer unlocks a gate but never starts a task. A recommended option is not consent; optional defaults must be explicit. Reopening a prerequisite does not erase historical completion evidence.

Overview progress covers all task scopes and excludes inbox, canceled and archived records; questions and notes are not tasks. Filters and the current work scope never change this denominator. The dense layout uses a sticky header and normal document scrolling, with 12 list entries per page and content-height detail panels.

Set `owner_review: true` only when human behavior/appearance confirmation is the remaining step. A waiting task then offers a one-button completion action if its prerequisites and acceptance checklist are satisfied. The `confirm` operation requires a user actor and current revision, records owner confirmation separately from prior automated evidence, and never answers questions or silently completes unfinished checks. Do not infer review eligibility merely from a waiting status or title.

Delete moves any record to Trash, excluding it from ordinary lists and progress. Restore returns it with its original status, content, links and history. This is reversible removal, not permanent erasure. Use `{"action":"delete","id":"T-example","rev":1}` or `{"action":"restore-item","id":"T-example","rev":2}`; browse with `list --view trash`. Deleted records are read-only until restored. Deleting a prerequisite task/question never satisfies it or silently removes its links; restore it or explicitly revise the dependent task. Exports and backups retain deleted records and their events.

Question threads can link follow-up questions and implementation tasks. Humans may answer in the browser or chat. The main agent records chat answers with their provenance; it asks questions in chat, never through terminal prompts. Browser editing does not wake a stopped agent. Poll for updates at meaningful work boundaries and on session resumption.

All write paths use transactions and revision checks. A stale form receives a conflict and retains its input. Reload/reconcile deliberately; do not retry blindly with a newer revision. Saved events use the actual UTC clock; the UI displays Japan time. Imported source dates remain separate, and absent original timestamps stay unknown. Imports preserve original requirements rather than treating old suggestions as new instructions.

## Agent interface

Run from the repository root. Commands emit UTF-8 JSON. Prefer `brief`, then read relevant IDs or search instead of dumping the ledger.

```powershell
python .dashboard/app.py brief
python .dashboard/app.py list --scope v1.0.3
python .dashboard/app.py list --view questions
python .dashboard/app.py list --view archive --search "AAC"
python .dashboard/app.py get T-example
python .dashboard/app.py events --after 42 --limit 20
```

`brief` includes current scope, handoff, next action, scoped tasks with gates, outstanding questions and an event cursor. Lists exclude bodies and are paginated (`--limit`, `--offset`). `get` truncates large bodies at 6,000 characters unless `--full` is requested. `events` supports `--id`, `--before`, `--after` and `--limit`; page until the requested interval has been read. Archive records are searchable but excluded from normal task/history views.

Write an operation as a private UTF-8 JSON file, or pass JSON on stdin. Do not interpolate long user text into a shell command.

```powershell
python .dashboard/app.py apply --file .dashboard/operation.json --actor assistant
```

Examples (IDs and revisions are illustrative):

```json
{"action":"create","data":{"kind":"task","title":"Investigate the issue","scope":"current","status":"inbox","body":"Requirement and constraints"}}
{"action":"update","id":"T-example","rev":1,"data":{"status":"waiting","blocked_reason":"Owner appearance review","next_action":"Apply the review before release"}}
{"action":"comment","id":"T-example","rev":2,"text":"Automated controls passed; physical input remains unverified."}
{"action":"update","id":"Q-example","rev":1,"data":{"answer":"Keep the existing behavior","status":"answered"}}
{"action":"project","rev":1,"data":{"current_scope":"current","handoff":"Current facts and limits","next_action":"Next concrete action"}}
```

Use `--actor user` only when recording an actual owner action/answer, not an AI's interpretation. Browser writes are attributed to the user; test browser mutations against a disposable ledger. Record evidence and caveats on the task or a note. No need to record every command or micro-edit. Structured task extras include `checks: [{"text":"Verify output","done":false}]`, `depends_on: ["T-id"]`, `required_questions: ["Q-id"]`, `parent_id`, `verification` and `commit`. Questions support `asked_by`, `answer_by`, `recommended`, `default_action`, `requires_answer` and `answer`. Unknown fields and kind changes are rejected.

## Backup and recovery

```powershell
python .dashboard/app.py backup
python .dashboard/app.py export-json --output .dashboard/exports/checkpoint.json
python .dashboard/app.py --db .dashboard/recovered.sqlite restore --input .dashboard/exports/checkpoint.json
```

Backups use SQLite's consistent backup API. Exports include project data, all records, revisions and events. JSON export refuses to overwrite an existing file. Restore validates into a temporary database and publishes only to a **new** destination. Verify recovered records before replacing the live database, and stop its server before any deliberate file replacement. Backups under this folder are recovery points, not protection against losing the drive; copy a completed backup/export to your normal private backup destination. The tool does not delete older backups.

The header's Backup button creates another recovery snapshot under `.dashboard/backups/`. Edits already persist when saved; the button is useful before a bulk reorganization or import, not after every edit. Server startup also creates a snapshot. Copies remain on the same drive until you move a completed backup to your own backup storage.

## Public checkpoint

Edit and review the project's `public_summary` field, then run:

```powershell
python .dashboard/app.py export-status
```

Only that explicitly curated field is exported to `docs/STATUS.md`; private tasks, comments, handoff and counts are not included automatically. Inspect the generated diff before committing. STATUS is a public checkpoint, not a second editable task queue. Keep durable product contracts in ARCHITECTURE and repeatable commands in DEVELOPMENT. A public export is explicit, not an automatic consequence of answering a question.

## Verification and HTTP interface

```powershell
python -m unittest discover -s .dashboard -p test_app.py -v
```

Tests use disposable ledgers for concurrent writers, question/dependency gates, cycles, completion checks, private export boundaries, backup/restore, history cursors and HTTP origin/token enforcement. UI changes additionally need headless browser checks for navigation, edits, conflicts, polling, narrow viewports and script errors. No product Rust rebuild is needed for this isolated tool.

The single HTML document uses these endpoints: `GET /api/brief`, `/api/items?view=&scope=&search=&limit=&offset=`, `/api/item?id=`, and `/api/events?id=&before=&after=&limit=`. JSON mutations go to `POST /api` with the same operations as the CLI; `backup` is also available. The server substitutes `__DASHBOARD_TOKEN__` in HTML, and requests send it as `X-Dashboard-Token`. Host validation, same-origin writes and fixed API routes prevent arbitrary file serving or shell execution. User text must be escaped, never inserted as raw HTML. The service is local and is not a remote hosting solution.
