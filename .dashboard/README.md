# Local project dashboard

The browser UI in `index.html` is local-only and ignored by Git; the CLI, launcher and recovery tools remain tracked. Before using the browser in a fresh checkout, restore the HTML from a private backup or the repository history.

Python 3.11+ and a browser are sufficient. This standard-library tool is independent of the product and launches no AI. Run `powershell -NoProfile -File .dashboard/open.ps1`: it reuses or starts a hidden loopback server and opens the browser. Closing the browser leaves the server running; its matching PID/URL are in ignored `runtime.json`.

## Records and workflow

The private `state.sqlite` ledger is authoritative. New tasks default to **todo** (planned); use **inbox** explicitly for uncommitted ideas. Scope and state are separate: a planned task is not authorization to start it.

Ordinary tasks need only a title, request and state. `next_action`, `acceptance`, `verification`, `resolution`, checklists and relationships are optional advanced fields for useful conditions, caveats or interrupted work. Do not fill them as a completion ritual. Record shared validation once in a note or short thread entry instead of copying it to every task.

Task flow: **todo → doing → review → done**. `review` means implementation finished, awaiting owner confirmation. AI attempts to set `done` are converted to `review`, including record creation; no evidence paragraph is required. Only actual owner confirmation makes a task done. Confirmation preserves revision/prerequisite checks and records actor/time automatically. Legacy waiting + `owner_review` records remain supported.

Corrections and questions stay on the **same ID**:

- `thread` / `request`: append a correction/additional requirement and reopen the task to todo.
- `thread` / `question`: reopen a question, or put a task into waiting for the other participant's answer.
- `thread` / `answer`: preserve the answer; a task returns to todo, a question becomes answered. Only the requested participant can answer a task's pending question.
- `thread` / `comment`: append a note without changing state.

Original requirements and earlier replies remain. Use separate records only for independent work, not each review round. Agents ask in chat, attribute actual answers correctly, and check edits at work boundaries. Browser saves do not wake an idle agent.

Explicit dependencies/checklists still apply. Implementation can proceed after a prerequisite reaches review; final confirmation requires its confirmation. Unanswered mandatory questions, including those in the same thread, block work/completion. Recommendations are not consent; optional defaults must be explicit. Answers do not start execution automatically.

Delete moves records to **Trash**, excluding them from ordinary lists/progress. Restore retains original state, links and history. Deleted records are read-only; deleting a prerequisite never satisfies it or silently removes links. Exports/backups retain deleted records.

## Progress periods

Progress covers all scopes, regardless of filters: unfinished tasks plus owner-confirmed completions since the last checkpoint. Inbox, canceled, deleted and archived records, questions and notes are excluded. Review tasks remain unfinished.

After a successful push **and verification of its required checks/CI**, run:

```powershell
python .dashboard/app.py checkpoint --label <pushed-commit>
```

This timestamps the boundary and excludes older confirmed completions; it changes no item state and deletes no history. Unfinished/review tasks carry over. Repeating the same label is a no-op. Reopening an old item brings it back into progress. Local commits and failed pushes do not reset progress. This is an agent workflow step, not a Git hook or network monitor. Project settings also offer a manual checkpoint for another milestone.

## Agent interface

Run from the repository root; output is UTF-8 JSON. Use bounded reads:

```powershell
python .dashboard/app.py brief
python .dashboard/app.py list --scope v1.0.3
python .dashboard/app.py list --view archive --search AAC
python .dashboard/app.py get T-example
python .dashboard/app.py events --after 42 --limit 20
```

Lists omit bodies and support `--limit`/`--offset`. `get` truncates at 6,000 characters unless `--full` is supplied. Events support `--id`, `--before`, `--after` and `--limit`; page the requested interval completely. `brief` includes handoff, scoped active records, questions, progress and cursor. Archives are evidence, not current instructions.

Write a private UTF-8 operation file (or JSON on stdin), then run `python .dashboard/app.py apply --file .dashboard/operation.json --actor assistant`. Illustrative operations:

```json
{"action":"create","data":{"title":"Adjust spacing","scope":"current"}}
{"action":"update","id":"T-example","rev":1,"data":{"status":"review"}}
{"action":"thread","id":"T-example","rev":2,"mode":"question","text":"Which spacing should change?"}
{"action":"thread","id":"T-example","rev":3,"mode":"answer","text":"The toolbar spacing."}
{"action":"thread","id":"T-example","rev":4,"mode":"request","text":"Use a smaller gap."}
{"action":"confirm","id":"T-example","rev":5}
{"action":"delete","id":"T-example","rev":6}
{"action":"restore-item","id":"T-example","rev":7}
```

Use `--actor user` only for actual owner actions/answers; `confirm` requires it. Writes are revision-checked, and conflicts retain browser input. Reconcile instead of retrying blindly. Timestamps use the real UTC clock; the UI displays Japan time. Imported source dates remain separate; unknown dates are not invented.

## Privacy, backup and recovery

The `.gitignore` allowlist tracks structure, not databases, exports, backups, archives, logs, screenshots or migration scripts. Never force-add private data. A missing ledger after cloning is not an empty backlog: recover a backup, using STATUS/Git as limited fallback. `init` is for genuinely new projects only.

```powershell
python .dashboard/app.py backup
python .dashboard/app.py export-json --output .dashboard/exports/checkpoint.json
python .dashboard/app.py --db .dashboard/recovered.sqlite restore --input .dashboard/exports/checkpoint.json
```

Startup and Backup create consistent SQLite snapshots in `.dashboard/backups/`; normal Save already persists edits. Use snapshots before bulk changes. They are local recovery points, not drive-loss protection; copy a completed backup to private backup storage. Export/restore preserve revisions, threads and periods, and refuse overwriting existing destinations. Verify recovery and stop the matching server before replacing a live ledger. Older backups are not automatically deleted.

Review `public_summary`, then explicitly run `python .dashboard/app.py export-status` to generate `docs/STATUS.md`. No tasks, questions, private handoff or counts are automatically exported. Review the diff; STATUS is a public checkpoint, not a second task queue.

## UI and verification

Use a compact single HTML page, sticky single-line header, content-height split panels and 12-row pagination. Escape user text and preserve dirty forms during polling. Test browser mutations against disposable ledgers, since browser writes are attributed to the user.

Run `python -m unittest discover -s .dashboard -p test_app.py -v`; UI changes also need headless browser checks. Cover review/confirmation, same-record follow-ups, periods, conflicts, deletion/recovery and narrow layouts. No product Rust rebuild is needed for this tool.

GET: `/api/brief`, `/api/items?view=&scope=&search=&limit=&offset=`, `/api/item?id=`, `/api/events?id=&before=&after=&limit=`. POST `/api` accepts CLI operations, `backup`, and `checkpoint` with project revision and label. Item responses include `can_confirm`. The server substitutes `__DASHBOARD_TOKEN__` in HTML; send it as `X-Dashboard-Token`. Host/origin checks and fixed routes restrict this loopback-only service; it is not for remote hosting.
