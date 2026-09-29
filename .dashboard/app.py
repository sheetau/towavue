"""Local project ledger. Python 3.11+, standard library only; no agent execution."""
from __future__ import annotations

import argparse
import contextlib
import datetime as dt
import hmac
import json
import os
from pathlib import Path
import secrets
import sqlite3
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

ROOT = Path(__file__).resolve().parent
VERSION = 1
STATES = {
    "task": {"inbox", "todo", "doing", "waiting", "blocked", "review", "done", "canceled"},
    "question": {"open", "answered", "closed"},
    "note": {"recorded"},
}
EXTRA = {
    "next_action", "acceptance", "blocked_reason", "parent_id", "depends_on",
    "required_questions", "checks", "answer", "recommended", "default_action",
    "requires_answer", "asked_by", "answer_by", "source", "source_date",
    "verification", "commit", "archived", "resolution", "owner_review", "deleted", "thread_waiting_for",
}
PROJECT_FIELDS = {"name", "current_scope", "handoff", "next_action", "public_summary", "progress_since", "progress_label"}
ACTORS = {"user", "assistant", "system"}


def now():
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec="microseconds")


def encode(value):
    return json.dumps(value, ensure_ascii=False, indent=2)


class Problem(Exception):
    def __init__(self, message, status=400):
        super().__init__(message)
        self.status = status


class Store:
    def __init__(self, path, initialize=False, name="Project"):
        self.path = Path(path).resolve()
        if not self.path.exists() and not initialize:
            raise Problem("No local ledger. Run init, or restore a private backup.", 404)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.db = sqlite3.connect(self.path, timeout=10, isolation_level=None)
        self.db.row_factory = sqlite3.Row
        self.db.execute("PRAGMA foreign_keys=ON")
        if initialize:
            self.db.executescript("""
                CREATE TABLE IF NOT EXISTS project (
                    id INTEGER PRIMARY KEY CHECK(id=1), version INTEGER NOT NULL,
                    rev INTEGER NOT NULL, updated_at TEXT NOT NULL, data TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS items (
                    id TEXT PRIMARY KEY, kind TEXT NOT NULL, title TEXT NOT NULL,
                    status TEXT NOT NULL, scope TEXT NOT NULL, body TEXT NOT NULL,
                    data TEXT NOT NULL, rev INTEGER NOT NULL, created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL, closed_at TEXT);
                CREATE TABLE IF NOT EXISTS events (
                    seq INTEGER PRIMARY KEY AUTOINCREMENT, item_id TEXT REFERENCES items(id),
                    at TEXT NOT NULL, actor TEXT NOT NULL, action TEXT NOT NULL, data TEXT NOT NULL);
                CREATE INDEX IF NOT EXISTS item_state ON items(kind,status,scope);
                CREATE INDEX IF NOT EXISTS event_item ON events(item_id,seq);
            """)
            self.db.execute("INSERT OR IGNORE INTO project VALUES(1,?,?,?,?)", (
                VERSION, 1, now(), encode(dict(name=name, current_scope="current",
                handoff="", next_action="", public_summary=""))))
        row = self.db.execute("SELECT version FROM project WHERE id=1").fetchone()
        if not row or row[0] != VERSION:
            raise Problem("Unsupported ledger schema; preserve the file before upgrading.")

    def close(self):
        self.db.close()

    @contextlib.contextmanager
    def transaction(self):
        self.db.execute("BEGIN IMMEDIATE")
        try:
            yield
            self.db.execute("COMMIT")
        except BaseException:
            self.db.execute("ROLLBACK")
            raise

    def project(self):
        row = dict(self.db.execute("SELECT * FROM project WHERE id=1").fetchone())
        return {"progress_since": "", "progress_label": "Initial period", **json.loads(row["data"]),
                "rev": row["rev"], "updated_at": row["updated_at"]}

    def event(self, item, actor, action, data):
        if actor not in ACTORS:
            raise Problem("Invalid actor")
        self.db.execute("INSERT INTO events(item_id,at,actor,action,data) VALUES(?,?,?,?,?)",
                        (item, now(), actor, action, encode(data)))

    def update_project(self, patch, rev, actor):
        if not isinstance(patch, dict) or set(patch) - PROJECT_FIELDS:
            raise Problem("Unknown project field")
        if any(not isinstance(v, str) or len(v) > 24000 for v in patch.values()):
            raise Problem("Project fields must be text, at most 24,000 characters")
        with self.transaction():
            old = self.project()
            self.expect(old, rev)
            updated = {k: patch.get(k, old[k]) for k in PROJECT_FIELDS}
            if not updated["name"].strip() or not updated["current_scope"].strip():
                raise Problem("Name and current scope are required")
            self.db.execute("UPDATE project SET data=?,rev=rev+1,updated_at=? WHERE id=1",
                            (encode(updated), now()))
            self.event(None, actor, "project", {"before": {k: old[k] for k in patch}, "after": patch})
        return self.project()

    @staticmethod
    def expect(item, rev):
        if type(rev) is not int or item["rev"] != rev:
            raise Problem("This record changed. Reload and reconcile before saving.", 409)

    @staticmethod
    def unpack(row):
        result = dict(row)
        result.update(json.loads(result.pop("data")))
        return result

    def item(self, identity):
        row = self.db.execute("SELECT * FROM items WHERE id=?", (identity,)).fetchone()
        if not row:
            raise Problem("Record not found", 404)
        return self.unpack(row)

    def gates(self, item, require_confirmed=True):
        reasons = []
        if item.get("thread_waiting_for"):
            reasons.append(f"Thread needs an answer from {item['thread_waiting_for']}")
        for identity in item.get("depends_on", []):
            other = self.item(identity)
            if other.get("deleted"):
                reasons.append(f"Dependency {identity} was deleted; restore it or revise the prerequisite")
            elif other["status"] not in ({"done"} if require_confirmed else {"done", "review"}):
                reasons.append(f"Dependency {identity} is not done")
        for identity in item.get("required_questions", []):
            question = self.item(identity)
            answered = bool(question.get("answer", "").strip()) and question["status"] in {"answered", "closed"}
            permitted_default = question.get("requires_answer") is False and bool(question.get("default_action", "").strip())
            if question.get("deleted"):
                reasons.append(f"Question {identity} was deleted; restore it or revise the prerequisite")
            elif not answered and not permitted_default:
                reasons.append(f"Question {identity} needs an answer")
        return reasons

    def validate(self, item, enforce_state=True):
        kind = item.get("kind")
        if kind not in STATES or item.get("status") not in STATES[kind]:
            raise Problem("Invalid kind or status")
        if not isinstance(item.get("title"), str) or not item["title"].strip() or len(item["title"]) > 300:
            raise Problem("Title must contain 1-300 characters")
        for key in ("scope", "body", "next_action", "acceptance", "blocked_reason", "answer",
                    "recommended", "default_action", "asked_by", "answer_by", "source",
                    "source_date", "verification", "commit", "resolution", "thread_waiting_for"):
            if key in item and (not isinstance(item[key], str) or len(item[key]) > 2_000_000):
                raise Problem(f"Invalid text field: {key}")
        if item.get("thread_waiting_for", "") not in {"", "user", "assistant"}:
            raise Problem("Invalid thread recipient")
        for key in ("requires_answer", "archived", "owner_review", "deleted"):
            if key in item and type(item[key]) is not bool:
                raise Problem(f"Invalid boolean: {key}")
        checks = item.get("checks", [])
        if not isinstance(checks, list) or len(checks) > 100 or any(
            not isinstance(c, dict) or set(c) != {"text", "done"} or
            not isinstance(c["text"], str) or type(c["done"]) is not bool for c in checks
        ):
            raise Problem("Checks require text and a boolean done value")
        for key, expected in (("depends_on", "task"), ("required_questions", "question")):
            values = item.get(key, [])
            if not isinstance(values, list) or len(values) > 100 or any(not isinstance(v, str) for v in values):
                raise Problem(f"Invalid links: {key}")
            for identity in values:
                if identity == item["id"] or self.item(identity)["kind"] != expected:
                    raise Problem(f"Invalid {key} target")
        parent = item.get("parent_id", "")
        if not isinstance(parent, str):
            raise Problem("Parent must be a record ID")
        visited = {item["id"]}
        while parent:
            if parent in visited:
                raise Problem("Parent cycle")
            visited.add(parent)
            parent = self.item(parent).get("parent_id", "")
        stack = list(item.get("depends_on", []))
        visited = set()
        while stack:
            identity = stack.pop()
            if identity == item["id"]:
                raise Problem("Dependency cycle")
            if identity not in visited:
                visited.add(identity)
                stack.extend(self.item(identity).get("depends_on", []))
        if kind == "question" and item["status"] == "answered" and not item.get("answer", "").strip():
            raise Problem("An answered question needs an answer")
        if enforce_state and kind == "task" and item["status"] in {"doing", "review", "done"}:
            reasons = self.gates(item, require_confirmed=item["status"] == "done")
            if reasons:
                raise Problem("; ".join(reasons))
        if item["status"] in {"review", "done"} and any(not c["done"] for c in checks):
            raise Problem("Complete the acceptance checklist before marking done")
        if item["status"] in {"waiting", "blocked"} and not item.get("blocked_reason", "").strip():
            raise Problem("A waiting or blocked task needs a reason")

    def create(self, payload, actor):
        allowed = {"kind", "title", "status", "scope", "body"} | (EXTRA - {"deleted"})
        if not isinstance(payload, dict) or set(payload) - allowed:
            raise Problem("Unknown record field")
        kind = payload.get("kind", "task")
        default = {"task": "todo", "question": "open", "note": "recorded"}.get(kind)
        item = dict(kind=kind, title="", status=default, scope="backlog", body="")
        item.update(payload)
        if kind == "task" and item["status"] == "done" and actor != "user":
            item["status"] = "review"
        item["id"] = {"task": "T", "question": "Q", "note": "N"}.get(kind, "X") + "-" + secrets.token_hex(4)
        if kind == "question":
            item.setdefault("requires_answer", True)
        with self.transaction():
            self.validate(item)
            at = now()
            data = {k: v for k, v in item.items() if k in EXTRA}
            closed = at if item["status"] in {"done", "canceled", "closed", "recorded"} else None
            self.db.execute("INSERT INTO items VALUES(?,?,?,?,?,?,?,?,?,?,?)", (
                item["id"], kind, item["title"], item["status"], item["scope"], item["body"],
                encode(data), 1, at, at, closed))
            self.event(item["id"], actor, "created", {"source": item.get("source", "")})
        return self.item(item["id"])

    def update(self, identity, patch, rev, actor, thread_event=None):
        if not isinstance(patch, dict) or set(patch) - ({"title", "status", "scope", "body"} | (EXTRA - {"deleted"})):
            raise Problem("Unknown or immutable record field")
        with self.transaction():
            old = self.item(identity)
            self.expect(old, rev)
            if old.get("deleted"):
                raise Problem("Restore the deleted record before editing it")
            new = {**old, **patch}
            if new["kind"] == "task" and patch.get("status") == "done" and actor != "user":
                patch = {**patch, "status": "review"}
                new["status"] = "review"
            if new["kind"] == "task" and patch.get("status") == "review":
                patch = {**patch, "blocked_reason": "", "owner_review": False, "next_action": ""}
                new.update(patch)
            self.validate(new)
            at = now()
            terminal = new["status"] in {"done", "canceled", "closed", "recorded"}
            closed = (old["closed_at"] or at) if terminal else None
            data = {k: v for k, v in new.items() if k in EXTRA}
            self.db.execute("""UPDATE items SET title=?,status=?,scope=?,body=?,data=?,rev=rev+1,
                            updated_at=?,closed_at=? WHERE id=?""", (
                new["title"], new["status"], new["scope"], new["body"], encode(data), at, closed, identity))
            event_data = {"before": {k: old.get(k) for k in patch}, "after": patch}
            if thread_event:
                event_data.update(thread_event)
            self.event(identity, actor, "thread" if thread_event else "updated", event_data)
        return self.item(identity)

    def comment(self, identity, text, rev, actor):
        if not isinstance(text, str) or not text.strip() or len(text) > 24000:
            raise Problem("Comment must contain 1-24,000 characters")
        with self.transaction():
            item = self.item(identity)
            self.expect(item, rev)
            if item.get("deleted"):
                raise Problem("Restore the deleted record before commenting")
            self.db.execute("UPDATE items SET rev=rev+1,updated_at=? WHERE id=?", (now(), identity))
            self.event(identity, actor, "comment", {"text": text})
        return self.item(identity)

    def set_deleted(self, identity, rev, deleted, actor):
        with self.transaction():
            item = self.item(identity)
            self.expect(item, rev)
            if bool(item.get("deleted")) == deleted:
                raise Problem("Record is already in the requested state")
            data = {k: v for k, v in item.items() if k in EXTRA}
            data["deleted"] = deleted
            self.db.execute("UPDATE items SET data=?,rev=rev+1,updated_at=? WHERE id=?",
                            (encode(data), now(), identity))
            self.event(identity, actor, "deleted" if deleted else "restored", {})
        return self.item(identity)

    def can_confirm(self, item):
        return (item["kind"] == "task" and (item["status"] == "review" or
                (item["status"] == "waiting" and item.get("owner_review") is True))
                and not item.get("archived") and not item.get("deleted")
                and not self.gates(item) and all(c["done"] for c in item.get("checks", [])))

    def confirm(self, identity, rev, actor):
        if actor != "user":
            raise Problem("Only the owner can confirm a review", 403)
        item = self.item(identity)
        self.expect(item, rev)
        if not self.can_confirm(item):
            raise Problem("This task is not ready for owner confirmation")
        # update checks the same revision and all prerequisites again inside its transaction.
        return self.update(identity, {"status": "done", "owner_review": False,
                           "blocked_reason": "", "next_action": ""}, rev, actor)

    def thread(self, identity, rev, mode, text, actor):
        if mode not in {"comment", "request", "question", "answer"}:
            raise Problem("Unknown thread action")
        if not isinstance(text, str) or not text.strip() or len(text) > 24000:
            raise Problem("Thread text must contain 1-24,000 characters")
        item = self.item(identity)
        self.expect(item, rev)
        if item.get("archived") or item.get("deleted"):
            raise Problem("This record is read-only")
        if mode == "comment":
            return self.comment(identity, text, rev, actor)
        patch = {}
        if mode in {"request", "question"}:
            if item["kind"] == "note":
                raise Problem("Use a task or question for follow-up work")
            patch = {"status": "todo" if item["kind"] == "task" else "open",
                     "owner_review": False, "blocked_reason": "", "thread_waiting_for": ""}
            if item["kind"] == "question":
                patch["answer"] = ""
            if mode == "question" and item["kind"] == "task":
                patch.update(status="waiting", blocked_reason=text,
                             thread_waiting_for="user" if actor == "assistant" else "assistant")
        elif mode == "answer":
            if item["kind"] == "question":
                patch = {"answer": text, "status": "answered"}
            elif item["kind"] == "task" and item.get("thread_waiting_for") == actor:
                patch = {"status": "todo", "blocked_reason": "", "thread_waiting_for": ""}
            else:
                raise Problem("This thread is not awaiting your answer")
        return self.update(identity, patch, rev, actor, {"text": text, "mode": mode})

    def checkpoint(self, rev, label, actor):
        if not isinstance(label, str) or not label.strip():
            raise Problem("A checkpoint label is required")
        project = self.project()
        self.expect(project, rev)
        if project["progress_label"] == label and project["progress_since"]:
            return project
        return self.update_project({"progress_since": now(), "progress_label": label}, rev, actor)

    def history(self, identity=None, limit=50, before=None, after=None):
        conditions, params = [], []
        if identity:
            conditions.append("item_id=?")
            params.append(identity)
        if before:
            conditions.append("seq<?")
            params.append(int(before))
        if after is not None:
            conditions.append("seq>?")
            params.append(int(after))
        where = " WHERE " + " AND ".join(conditions) if conditions else ""
        rows = self.db.execute("SELECT * FROM events" + where + " ORDER BY seq DESC LIMIT ?",
                               (*params, min(max(int(limit), 1), 100)))
        return [{**dict(r), "data": json.loads(r["data"])} for r in rows]

    def query(self, view="tasks", scope="", search="", limit=50, offset=0):
        filters = {
            "tasks": "kind='task' AND status NOT IN ('done','canceled')",
            "questions": "kind='question' AND status!='closed'",
            "history": "(status IN ('done','canceled','closed') OR kind='note')",
            "archive": "json_extract(data,'$.archived')=1",
            "trash": "json_extract(data,'$.deleted')=1",
            "all": "1=1",
        }
        if view not in filters:
            raise Problem("Unknown view")
        conditions, params = [filters[view]], []
        if view not in {"archive", "all", "trash"}:
            conditions.append("COALESCE(json_extract(data,'$.archived'),0)=0")
        if view not in {"trash", "all"}:
            conditions.append("COALESCE(json_extract(data,'$.deleted'),0)=0")
        if scope:
            conditions.append("scope=?")
            params.append(scope)
        if search:
            conditions.append("(instr(lower(title),lower(?))>0 OR instr(lower(body),lower(?))>0 OR id=?)")
            params.extend([search, search, search])
        where = " AND ".join(conditions)
        total = self.db.execute("SELECT count(*) FROM items WHERE " + where, params).fetchone()[0]
        order = "updated_at DESC,id" if view in {"history", "archive", "all", "trash"} else (
            "CASE status WHEN 'doing' THEN 0 WHEN 'review' THEN 1 WHEN 'waiting' THEN 2 WHEN 'blocked' THEN 2 "
            "WHEN 'open' THEN 0 WHEN 'answered' THEN 1 WHEN 'todo' THEN 3 ELSE 4 END,updated_at DESC,id")
        columns = ("id,kind,title,status,scope,rev,created_at,updated_at,closed_at,"
                   "json_extract(data,'$.next_action') AS next_action,"
                   "json_extract(data,'$.blocked_reason') AS blocked_reason,"
                   "json_extract(data,'$.deleted') AS deleted,"
                   "json_extract(data,'$.source_date') AS source_date")
        rows = self.db.execute("SELECT " + columns + " FROM items WHERE " + where + " ORDER BY " + order + " LIMIT ? OFFSET ?",
                               (*params, min(max(int(limit), 1), 100), max(int(offset), 0)))
        return {"items": [dict(row) for row in rows], "total": total}

    def brief(self):
        project = self.project()
        counts = {r["status"]: r["n"] for r in self.db.execute(
            "SELECT status,count(*) AS n FROM items WHERE kind='task' AND scope=? "
            "AND COALESCE(json_extract(data,'$.archived'),0)=0 AND COALESCE(json_extract(data,'$.deleted'),0)=0 GROUP BY status",
            (project["current_scope"],))}
        overall = {r["status"]: r["n"] for r in self.db.execute(
            "SELECT status,count(*) AS n FROM items WHERE kind='task' "
            "AND COALESCE(json_extract(data,'$.archived'),0)=0 AND COALESCE(json_extract(data,'$.deleted'),0)=0 GROUP BY status")}
        completed = self.db.execute("SELECT count(*) FROM items WHERE kind='task' AND status='done' AND closed_at>=? "
                    "AND COALESCE(json_extract(data,'$.archived'),0)=0 AND COALESCE(json_extract(data,'$.deleted'),0)=0",
                    (project["progress_since"],)).fetchone()[0]
        progress = {"done": completed, "total": completed + sum(n for status, n in overall.items()
                    if status not in {"inbox", "canceled", "done"})}
        open_questions = self.db.execute("SELECT count(*) FROM items WHERE kind='question' AND status='open' "
                         "AND COALESCE(json_extract(data,'$.archived'),0)=0 AND COALESCE(json_extract(data,'$.deleted'),0)=0").fetchone()[0]
        questions = self.query("questions", limit=10)
        active = self.query("tasks", project["current_scope"], limit=20)
        for item in active["items"]:
            item["gates"] = self.gates(self.item(item["id"]), require_confirmed=False)
        return {"project": project, "counts": counts, "overall_counts": overall, "progress": progress,
                "open_questions": open_questions, "active": active, "questions": questions,
                "scopes": [r[0] for r in self.db.execute("SELECT DISTINCT scope FROM items ORDER BY scope")],
                "cursor": self.db.execute("SELECT COALESCE(max(seq),0) FROM events").fetchone()[0]}

    def backup(self):
        folder = self.path.parent / "backups"
        folder.mkdir(exist_ok=True)
        path = folder / (dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ") + ".sqlite")
        with contextlib.closing(sqlite3.connect(path)) as target:
            self.db.backup(target)
        return str(path)

    def export(self):
        with self.transaction():
            return {"version": VERSION, "exported_at": now(), "project": self.project(),
                    "items": [self.unpack(r) for r in self.db.execute("SELECT * FROM items ORDER BY id")],
                    "events": [{**dict(r), "data": json.loads(r["data"])} for r in self.db.execute("SELECT * FROM events ORDER BY seq")]}


def restore(path, payload):
    path = Path(path).resolve()
    if path.exists():
        raise Problem("Restore requires a new destination; existing ledgers are never overwritten")
    if payload.get("version") != VERSION:
        raise Problem("Unsupported export version")
    temporary = path.with_name(path.name + ".restore-" + secrets.token_hex(4))
    store = Store(temporary, True)
    try:
        with store.transaction():
            project = payload["project"]
            store.db.execute("UPDATE project SET rev=?,updated_at=?,data=?", (
                project["rev"], project["updated_at"], encode({k: project.get(k, "") for k in PROJECT_FIELDS})))
            for item in payload["items"]:
                store.db.execute("INSERT INTO items VALUES(?,?,?,?,?,?,?,?,?,?,?)", (
                    item["id"], item["kind"], item["title"], item["status"], item["scope"], item["body"],
                    encode({k: v for k, v in item.items() if k in EXTRA}), item["rev"],
                    item["created_at"], item["updated_at"], item["closed_at"]))
            for item in payload["items"]:
                store.validate(item, enforce_state=False)
            for event in payload["events"]:
                if event["actor"] not in ACTORS:
                    raise Problem("Invalid archived actor")
                store.db.execute("INSERT INTO events VALUES(?,?,?,?,?,?)", (
                    event["seq"], event["item_id"], event["at"], event["actor"], event["action"], encode(event["data"])))
        store.close()
        # Hard-link publication fails if a destination appeared during validation.
        os.link(temporary, path)
    finally:
        store.close()
        temporary.unlink(missing_ok=True)
    return {"restored": str(path)}


def mutate(store, request, actor):
    if not isinstance(request, dict):
        raise Problem("Operation must be an object")
    action = request.get("action")
    if action == "create":
        return store.create(request.get("data", {}), actor)
    if action == "update":
        return store.update(request.get("id"), request.get("data", {}), request.get("rev"), actor)
    if action == "comment":
        return store.comment(request.get("id"), request.get("text"), request.get("rev"), actor)
    if action == "confirm":
        return store.confirm(request.get("id"), request.get("rev"), actor)
    if action == "thread":
        return store.thread(request.get("id"), request.get("rev"), request.get("mode"), request.get("text"), actor)
    if action == "checkpoint":
        return store.checkpoint(request.get("rev"), request.get("label"), actor)
    if action in {"delete", "restore-item"}:
        return store.set_deleted(request.get("id"), request.get("rev"), action == "delete", actor)
    if action == "project":
        return store.update_project(request.get("data", {}), request.get("rev"), actor)
    if action == "backup":
        return {"backup": store.backup()}
    raise Problem("Unknown action")


def server(path, port=0):
    token = secrets.token_urlsafe(32)
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def respond(self, status, payload, content_type="application/json; charset=utf-8"):
            data = payload.encode("utf-8") if isinstance(payload, str) else encode(payload).encode("utf-8")
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(data)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("Content-Security-Policy", "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'")
            self.end_headers()
            self.wfile.write(data)

        def run_request(self, writing=False):
            store = None
            try:
                host = f"127.0.0.1:{self.server.server_port}"
                if self.headers.get("Host") != host:
                    raise Problem("Invalid host", 403)
                parsed = urlparse(self.path)
                if not writing and parsed.path == "/":
                    html = (ROOT / "index.html").read_text(encoding="utf-8").replace("__DASHBOARD_TOKEN__", token)
                    return self.respond(200, html, "text/html; charset=utf-8")
                if not writing and parsed.path == "/health":
                    return self.respond(200, {"service": "project-dashboard", "database": str(Path(path).resolve())})
                if not hmac.compare_digest(self.headers.get("X-Dashboard-Token", ""), token):
                    raise Problem("Invalid dashboard token", 403)
                if writing and self.headers.get("Origin") != "http://" + host:
                    raise Problem("Invalid origin", 403)
                store = Store(path)
                if writing:
                    if parsed.path != "/api" or self.headers.get_content_type() != "application/json":
                        raise Problem("Expected a JSON operation", 400)
                    size = int(self.headers.get("Content-Length", "0"))
                    if not 0 < size <= 3_000_000:
                        raise Problem("Request size is invalid", 413)
                    result = mutate(store, json.loads(self.rfile.read(size)), "user")
                else:
                    args = {k: v[0] for k, v in parse_qs(parsed.query).items()}
                    if parsed.path == "/api/brief":
                        result = store.brief()
                    elif parsed.path == "/api/items":
                        result = store.query(**{k: v for k, v in args.items() if k in {"view", "scope", "search", "limit", "offset"}})
                    elif parsed.path == "/api/item":
                        result = {"item": store.item(args.get("id")), "events": store.history(args.get("id")),
                                  "children": [dict(r) for r in store.db.execute(
                                      "SELECT id,title,kind,status FROM items WHERE json_extract(data,'$.parent_id')=?", (args.get("id"),))]}
                        result["can_confirm"] = store.can_confirm(result["item"])
                    elif parsed.path == "/api/events":
                        result = store.history(args.get("id"), args.get("limit", 50), args.get("before"), args.get("after"))
                    else:
                        raise Problem("Not found", 404)
                self.respond(200, result)
            except Problem as error:
                self.respond(error.status, {"error": str(error)})
            except (ValueError, TypeError, KeyError) as error:
                self.respond(400, {"error": "Invalid request: " + str(error)})
            except sqlite3.Error:
                self.respond(503, {"error": "Ledger unavailable; your edits have not been saved. Retry or restore a backup."})
            finally:
                if store:
                    store.close()

        def do_GET(self):
            self.run_request()

        def do_POST(self):
            self.run_request(True)

    instance = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    instance.daemon_threads = True
    return instance


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--db", type=Path, default=ROOT / "state.sqlite")
    commands = parser.add_subparsers(dest="command", required=True)
    init = commands.add_parser("init")
    init.add_argument("--name", default=ROOT.parent.name)
    commands.add_parser("brief")
    query = commands.add_parser("list")
    query.add_argument("--view", default="tasks", choices=["tasks", "questions", "history", "archive", "trash", "all"])
    query.add_argument("--scope", default="")
    query.add_argument("--search", default="")
    query.add_argument("--limit", type=int, default=30)
    query.add_argument("--offset", type=int, default=0)
    get = commands.add_parser("get")
    get.add_argument("id")
    get.add_argument("--full", action="store_true")
    events = commands.add_parser("events")
    events.add_argument("--id")
    events.add_argument("--before", type=int)
    events.add_argument("--after", type=int)
    events.add_argument("--limit", type=int, default=20)
    apply = commands.add_parser("apply")
    apply.add_argument("--file", type=Path, help="UTF-8 operation JSON; omit to read stdin")
    apply.add_argument("--actor", choices=sorted(ACTORS), default="assistant")
    commands.add_parser("backup")
    checkpoint = commands.add_parser("checkpoint")
    checkpoint.add_argument("--label", required=True, help="Verified push commit or explicit milestone")
    export = commands.add_parser("export-json")
    export.add_argument("--output", type=Path, required=True)
    recovery = commands.add_parser("restore")
    recovery.add_argument("--input", type=Path, required=True)
    status = commands.add_parser("export-status")
    status.add_argument("--output", type=Path, default=ROOT.parent / "docs" / "STATUS.md")
    serve = commands.add_parser("serve")
    serve.add_argument("--port", type=int, default=0)
    args = parser.parse_args()
    store = None
    try:
        if args.command == "restore":
            result = restore(args.db, json.loads(args.input.read_text(encoding="utf-8-sig")))
        else:
            store = Store(args.db, args.command == "init", getattr(args, "name", "Project"))
            if args.command in {"init", "brief"}:
                result = store.brief()
            elif args.command == "list":
                result = store.query(args.view, args.scope, args.search, args.limit, args.offset)
            elif args.command == "get":
                result = store.item(args.id)
                result["gates"] = store.gates(result)
                if not args.full and len(result["body"]) > 6000:
                    result["body"] = result["body"][:6000]
                    result["body_truncated"] = True
            elif args.command == "events":
                result = store.history(args.id, args.limit, args.before, args.after)
            elif args.command == "apply":
                text = args.file.read_text(encoding="utf-8-sig") if args.file else sys.stdin.read()
                result = mutate(store, json.loads(text), args.actor)
            elif args.command == "backup":
                result = {"backup": store.backup()}
            elif args.command == "checkpoint":
                result = store.checkpoint(store.project()["rev"], args.label, "assistant")
            elif args.command == "export-json":
                args.output.parent.mkdir(parents=True, exist_ok=True)
                with args.output.open("x", encoding="utf-8", newline="\n") as output:
                    output.write(encode(store.export()) + "\n")
                result = {"export": str(args.output)}
            elif args.command == "export-status":
                project = store.project()
                if not project["public_summary"].strip():
                    raise Problem("Set and review the public_summary field first")
                text = ("# Current work and handoff\n\n"
                        "<!-- Generated from the explicitly curated public summary. Do not edit directly. -->\n"
                        f"Generated: {now()}. Public project revision: {project['rev']}.\n\n"
                        + project["public_summary"].strip() + "\n\n"
                        "Private tasks, questions, evidence and archives stay in the local dashboard. "
                        "Run `python .dashboard/app.py brief` for current work, or see "
                        "[Dashboard operations](../.dashboard/README.md). "
                        "This export is a checkpoint, not the live task queue.\n")
                args.output.parent.mkdir(parents=True, exist_ok=True)
                args.output.write_text(text, encoding="utf-8", newline="\n")
                result = {"summary": str(args.output)}
            elif args.command == "serve":
                store.backup()
                store.close()
                store = None
                instance = server(args.db, args.port)
                manifest = args.db.resolve().parent / "runtime.json"
                info = {"pid": os.getpid(), "url": f"http://127.0.0.1:{instance.server_port}", "started_at": now()}
                manifest.write_text(encode(info), encoding="utf-8")
                if sys.stdout:
                    print(encode(info), flush=True)
                try:
                    instance.serve_forever()
                finally:
                    instance.server_close()
                    if manifest.exists() and json.loads(manifest.read_text(encoding="utf-8")).get("pid") == os.getpid():
                        manifest.unlink()
                return 0
        print(encode(result))
        return 0
    except (Problem, ValueError, KeyError, OSError, sqlite3.Error) as error:
        if sys.stderr:
            print(encode({"error": str(error)}), file=sys.stderr)
        return 1
    finally:
        if store:
            store.close()


if __name__ == "__main__":
    if sys.stdout:
        sys.stdout.reconfigure(encoding="utf-8")
    if sys.stderr:
        sys.stderr.reconfigure(encoding="utf-8")
    raise SystemExit(main())
