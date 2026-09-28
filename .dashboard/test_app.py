"""Behavior tests use private temporary ledgers; never the user's database."""
import concurrent.futures
import http.client
import json
from pathlib import Path
import re
import sqlite3
import subprocess
import sys
import tempfile
import threading
import unittest

import app


class LedgerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="project-ledger-test-")
        self.path = Path(self.temp.name) / "state.sqlite"
        self.store = app.Store(self.path, True, "Test project")

    def tearDown(self):
        self.store.close()
        self.temp.cleanup()

    def task(self, **fields):
        return self.store.create({"title": "Test task", **fields}, "assistant")

    def test_question_gates_default_is_not_recommendation(self):
        q = self.store.create({"kind": "question", "title": "Choose", "recommended": "Option A"}, "assistant")
        t = self.task(required_questions=[q["id"]], scope="current")
        for state in ["doing", "done"]:
            with self.assertRaises(app.Problem):
                self.store.update(t["id"], {"status": state}, t["rev"], "assistant")
        self.assertEqual(self.store.item(t["id"])["rev"], 1)
        self.store.update(q["id"], {"answer": "A", "status": "answered"}, q["rev"], "user")
        # An answer unlocks a gate, but never starts the task automatically.
        self.assertEqual(self.store.item(t["id"])["status"], "inbox")
        self.store.update(t["id"], {"status": "doing"}, t["rev"], "assistant")
        optional = self.store.create({"kind": "question", "title": "Optional", "requires_answer": False,
                                      "default_action": "Keep the existing color"}, "assistant")
        self.task(status="doing", required_questions=[optional["id"]])

    def test_overall_progress_excludes_inbox_canceled_archives_and_non_tasks(self):
        self.task(status="done", scope="current")
        self.task(status="todo", scope="later")
        self.task(status="waiting", scope="later", blocked_reason="Owner review")
        self.task(status="inbox")
        self.task(status="canceled")
        self.task(status="done", scope="current", archived=True)
        self.store.create({"kind": "note", "title": "Evidence"}, "assistant")
        self.store.create({"kind": "question", "title": "Open"}, "assistant")
        self.store.create({"kind": "question", "title": "Answered", "status": "answered", "answer": "Yes"}, "user")
        brief = self.store.brief()
        self.assertEqual(brief["progress"], {"done": 1, "total": 3})
        self.assertEqual(brief["counts"], {"done": 1})
        self.assertEqual(brief["open_questions"], 1)
        self.store.update_project({"current_scope": "later"}, 1, "user")
        self.assertEqual(self.store.brief()["progress"], brief["progress"])

    def test_owner_confirmation_requires_explicit_review_and_preserves_gates(self):
        task = self.task(status="waiting", blocked_reason="Review", owner_review=True,
                         verification="Automated checks passed")
        self.assertTrue(self.store.can_confirm(task))
        with self.assertRaises(app.Problem):
            self.store.confirm(task["id"], 1, "assistant")
        result = self.store.confirm(task["id"], 1, "user")
        self.assertEqual(result["status"], "done")
        self.assertIn("Automated checks passed\nOwner confirmed", result["verification"])
        self.assertEqual(result["blocked_reason"], "")
        self.assertEqual(self.store.history(task["id"])[0]["actor"], "user")
        with self.assertRaises(app.Problem):
            self.store.confirm(task["id"], 1, "user")
        question = self.store.create({"kind": "question", "title": "Needs an answer"}, "assistant")
        prerequisite = self.task(status="todo")
        for fields in [
            {}, {"owner_review": True, "required_questions": [question["id"]]},
            {"owner_review": True, "depends_on": [prerequisite["id"]]},
            {"owner_review": True, "checks": [{"text": "Not validated", "done": False}]},
        ]:
            item = self.task(status="waiting", blocked_reason="Waiting", **fields)
            self.assertFalse(self.store.can_confirm(item))
            with self.assertRaises(app.Problem):
                self.store.confirm(item["id"], item["rev"], "user")
        self.assertFalse(self.store.can_confirm(question))

    def test_simultaneous_writes_preserve_winner_and_reject_stale_copy(self):
        t = self.task()
        barrier = threading.Barrier(2)
        def worker(title):
            store = app.Store(self.path)
            try:
                barrier.wait()
                try:
                    return store.update(t["id"], {"title": title}, 1, "user")["title"]
                except app.Problem as error:
                    return error.status
            finally:
                store.close()
        with concurrent.futures.ThreadPoolExecutor(2) as pool:
            results = list(pool.map(worker, ["First", "Second"]))
        self.assertEqual(results.count(409), 1)
        self.assertEqual(self.store.item(t["id"])["rev"], 2)
        self.assertEqual(len(self.store.history(t["id"])), 2)

    def test_dependency_cycles_and_acceptance(self):
        a = self.task()
        b = self.task(depends_on=[a["id"]])
        with self.assertRaises(app.Problem):
            self.store.update(a["id"], {"depends_on": [b["id"]]}, 1, "assistant")
        with self.assertRaises(app.Problem):
            self.store.update(a["id"], {"parent_id": a["id"]}, 1, "assistant")
        with self.assertRaises(app.Problem):
            self.task(status="done", checks=[{"text": "Verify", "done": False}])
        with self.assertRaises(app.Problem):
            self.task(status="waiting")
        self.store.update(a["id"], {"status": "done"}, 1, "assistant")
        self.store.update(b["id"], {"status": "doing"}, 1, "assistant")

    def test_brief_and_search_do_not_dump_archives(self):
        self.task(scope="current", title="Current")
        self.store.create({"kind": "note", "title": "Legacy", "body": "large evidence " * 50000,
                           "archived": True, "source_date": "2026-09-20"}, "system")
        self.assertLess(len(app.encode(self.store.brief())), 4000)
        self.assertEqual(self.store.query("history")["total"], 0)
        result = self.store.query("archive", search="evidence")
        self.assertEqual(result["total"], 1)
        self.assertNotIn("body", result["items"][0])
        self.assertEqual(self.store.query(search="' OR 1=1 --")["total"], 0)

    def test_backup_export_restore_preserves_threads_and_clocks(self):
        t = self.task(title="日本語", body="User requirement")
        self.store.comment(t["id"], "Keep the original", 1, "user")
        backup = self.store.backup()
        copy = app.Store(backup)
        self.assertEqual(copy.item(t["id"]), self.store.item(t["id"]))
        copy.close()
        payload = self.store.export()
        target = Path(self.temp.name) / "restored.sqlite"
        app.restore(target, payload)
        recovered = app.Store(target)
        again = recovered.export()
        for key in ["project", "items", "events"]:
            self.assertEqual(again[key], payload[key])
        recovered.close()
        with self.assertRaises(app.Problem):
            app.restore(target, payload)
        broken = {**payload, "items": [{**payload["items"][0], "status": "unknown"}]}
        invalid = Path(self.temp.name) / "invalid.sqlite"
        with self.assertRaises(app.Problem):
            app.restore(invalid, broken)
        self.assertFalse(invalid.exists())

    def test_restore_retains_completed_history_if_dependency_reopened(self):
        a = self.task(status="done")
        self.task(status="done", depends_on=[a["id"]])
        self.store.update(a["id"], {"status": "todo"}, 1, "user")
        app.restore(Path(self.temp.name) / "history.sqlite", self.store.export())

    def test_changes_since_cursor_and_actor_provenance(self):
        t = self.task()
        cursor = self.store.brief()["cursor"]
        self.store.comment(t["id"], "Question answered in chat", 1, "user")
        events = self.store.history(after=cursor)
        self.assertEqual(len(events), 1)
        self.assertEqual(events[0]["actor"], "user")
        self.assertRegex(events[0]["at"], r"T\d\d:\d\d:\d\d\.\d+\+00:00$")
        self.assertEqual(self.store.history(after=events[0]["seq"]), [])

    def test_public_export_never_implicitly_includes_private_content(self):
        self.task(title="PRIVATE-UNIQUE-MARKER", body=r"C:\personal\records")
        self.store.update_project({"handoff": "PRIVATE-UNIQUE-MARKER", "public_summary": "Public checkpoint."}, 1, "user")
        output = Path(self.temp.name) / "STATUS.md"
        result = subprocess.run([sys.executable, str(app.ROOT / "app.py"), "--db", str(self.path),
                                 "export-status", "--output", str(output)], capture_output=True,
                                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        self.assertEqual(result.returncode, 0, result.stderr)
        text = output.read_text(encoding="utf-8")
        self.assertIn("Public checkpoint.", text)
        self.assertNotIn("PRIVATE-UNIQUE-MARKER", text)
        self.assertNotIn("personal", text)

    def test_http_storage_boundary_and_revision_conflict(self):
        httpd = app.server(self.path)
        thread = threading.Thread(target=httpd.serve_forever, daemon=True)
        thread.start()
        host = f"127.0.0.1:{httpd.server_port}"
        def request(method, path, payload=None, headers=None):
            connection = http.client.HTTPConnection("127.0.0.1", httpd.server_port, timeout=5)
            try:
                connection.request(method, path, app.encode(payload) if payload is not None else None,
                                   headers=headers or {})
                response = connection.getresponse()
                return response.status, response.read().decode("utf-8")
            finally:
                connection.close()
        try:
            status, html = request("GET", "/")
            self.assertEqual(status, 200)
            token = re.search(r"const token\s*=\s*['\"]([^'\"]+)['\"]", html)[1]
            headers = {"X-Dashboard-Token": token, "Content-Type": "application/json", "Origin": "http://" + host}
            self.assertEqual(request("GET", "/api/brief")[0], 403)
            self.assertEqual(request("GET", "/", headers={"Host": "evil.example"})[0], 403)
            self.assertEqual(request("POST", "/api", {}, {**headers, "Origin": "https://evil.example"})[0], 403)
            self.assertEqual(request("GET", "/state.sqlite", headers=headers)[0], 404)
            status, body = request("POST", "/api", {"action": "create", "data": {"title": "Browser entry"}}, headers)
            self.assertEqual(status, 200, body)
            item = json.loads(body)
            command = {"action": "update", "id": item["id"], "rev": 1, "data": {"title": "Edited"}}
            self.assertEqual(request("POST", "/api", command, headers)[0], 200)
            self.assertEqual(request("POST", "/api", command, headers)[0], 409)
            self.assertEqual(request("POST", "/api", [], headers)[0], 400)
            self.assertEqual(self.store.history(item["id"])[0]["actor"], "user")
        finally:
            httpd.shutdown()
            httpd.server_close()
            thread.join()


if __name__ == "__main__":
    unittest.main()
