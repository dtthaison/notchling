"""Preflight: may the worker start this issue? Timeline JSON in the shapes the API returns, served by a
fake HTTP server on 127.0.0.1. No GitHub, no `gh`."""

from __future__ import annotations

import io
import json
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import urlsplit

import preflight

LABEL = "notchling:queued"
T0 = "2026-11-08T09:00:00Z"
T1 = "2026-11-08T10:00:00Z"
T2 = "2026-11-08T11:00:00Z"
T3 = "2026-11-08T12:00:00Z"


def issue(**over):
    base = {"number": 5, "state": "open", "labels": [{"name": LABEL}, {"name": "bug"}],
            "body": "text", "user": {"login": "reporter"}}
    base.update(over)
    return base


def labeled(actor, at, name=LABEL):
    return {"event": "labeled", "created_at": at, "label": {"name": name, "color": "ededed"},
            "actor": None if actor is None else {"login": actor, "type": "User"}}


def unlabeled(actor, at, name=LABEL):
    return {"event": "unlabeled", "created_at": at, "label": {"name": name},
            "actor": {"login": actor, "type": "User"}}


def commented(at):
    return {"event": "commented", "created_at": at, "actor": {"login": "reporter"}, "body": "hi"}


def graphql_edits(*edited_at, more=False):
    return {"data": {"repository": {"issue": {"userContentEdits": {
        "nodes": [{"editedAt": t, "deletedAt": None} for t in edited_at],
        "pageInfo": {"hasNextPage": more}}}}}}


class FakeGitHub:
    """Routes are {(method, path): (status, body)}; a path may be a callable taking the query dict."""

    def __init__(self, routes):
        self.requests = []
        outer = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *a):
                pass

            def _serve(self, method):
                parts = urlsplit(self.path)
                length = int(self.headers.get("Content-Length") or 0)
                body = self.rfile.read(length) if length else b""
                outer.requests.append({"method": method, "path": parts.path, "query": parts.query,
                                       "auth": self.headers.get("Authorization"),
                                       "body": body.decode() if body else ""})
                key = (method, parts.path + ("?" + parts.query if parts.query else ""))
                route = routes.get(key) or routes.get((method, parts.path))
                if route is None:
                    status, payload = 404, {"message": "Not Found"}
                else:
                    status, payload = route
                data = json.dumps(payload).encode()
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def do_GET(self):
                self._serve("GET")

            def do_POST(self):
                self._serve("POST")

        self.server = HTTPServer(("127.0.0.1", 0), Handler)
        self.url = "http://127.0.0.1:%d" % self.server.server_port
        self.thread = threading.Thread(target=self.server.serve_forever, kwargs={"poll_interval": 0.01}, daemon=True)
        self.thread.start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()


def routes_for(iss=None, timeline=None, edits=None, graphql_status=200, timeline_status=200,
               issue_status=200):
    iss = issue() if iss is None else iss
    timeline = [labeled("dtthaison", T1)] if timeline is None else timeline
    edits = graphql_edits() if edits is None else edits
    r = {("GET", "/repos/o/r/issues/5"): (issue_status, iss),
         ("POST", "/graphql"): (graphql_status, edits)}
    # Two pages of 100 at most; the fake serves by page number.
    pages = [timeline[i:i + 100] for i in range(0, len(timeline), 100)] or [[]]
    for n, page in enumerate(pages, start=1):
        r[("GET", "/repos/o/r/issues/5/timeline?per_page=100&page=%d" % n)] = (timeline_status, page)
    r[("GET", "/repos/o/r/issues/5/timeline?per_page=100&page=%d" % (len(pages) + 1))] = (200, [])
    return r


class PreflightTests(unittest.TestCase):
    def run_check(self, **kw):
        srv = FakeGitHub(routes_for(**kw))
        self.addCleanup(srv.close)
        self.srv = srv
        api = preflight.Api(srv.url, token="read-token")
        return preflight.check("o/r", 5, api)

    def assertOk(self, **kw):
        res = self.run_check(**kw)
        self.assertEqual((res.code, res.reason), (0, "ok"), res)

    def assertRefused(self, fragment, **kw):
        res = self.run_check(**kw)
        self.assertEqual(res.code, 1, res)
        self.assertIn(fragment, res.reason)

    def test_maintainer_queued_and_untouched(self):
        self.assertOk()

    def test_wrong_actor_is_refused(self):
        self.assertRefused("not a maintainer", timeline=[labeled("TrietKun", T1)])

    def test_actor_compared_case_insensitively(self):
        self.assertOk(timeline=[labeled("DTThaison", T1)])

    def test_deleted_actor_is_refused(self):
        self.assertRefused("not a maintainer", timeline=[labeled(None, T1)])

    def test_never_labeled_is_refused(self):
        self.assertRefused("no queued label", timeline=[commented(T1)])

    def test_other_label_by_non_maintainer_does_not_matter(self):
        self.assertOk(timeline=[labeled("dtthaison", T0), labeled("stranger", T1, name="bug")])

    def test_label_removed_is_refused(self):
        self.assertRefused("not on the issue", iss=issue(labels=[{"name": "bug"}]),
                           timeline=[labeled("dtthaison", T1), unlabeled("dtthaison", T2)])

    def test_unlabel_event_after_label_is_refused_even_if_labels_list_is_stale(self):
        self.assertRefused("removed", timeline=[labeled("dtthaison", T1), unlabeled("dtthaison", T2)])

    def test_removed_and_readded_by_someone_else_is_refused(self):
        self.assertRefused("not a maintainer", timeline=[
            labeled("dtthaison", T0), unlabeled("dtthaison", T1), labeled("stranger", T2)])

    def test_removed_and_readded_by_maintainer_is_ok(self):
        self.assertOk(timeline=[labeled("stranger", T0), unlabeled("dtthaison", T1),
                                labeled("dtthaison", T2)], edits=graphql_edits(T0))

    def test_body_edited_after_label_is_refused(self):
        self.assertRefused("edited", edits=graphql_edits(T0, T2))

    def test_body_edited_before_label_is_ok(self):
        self.assertOk(edits=graphql_edits(T0))

    def test_body_edited_in_the_same_second_is_refused(self):
        self.assertRefused("edited", edits=graphql_edits(T1))

    def test_edited_timeline_event_after_label_is_refused(self):
        ev = {"event": "edited", "created_at": T2, "actor": {"login": "reporter"}}
        self.assertRefused("edited", timeline=[labeled("dtthaison", T1), ev])

    def test_too_many_edits_to_read_is_refused(self):
        self.assertRefused("edit history", edits=graphql_edits(T0, more=True))

    def test_a_comment_after_the_label_is_fine(self):
        self.assertOk(timeline=[labeled("dtthaison", T1), commented(T3)])

    def test_pull_request_is_refused(self):
        self.assertRefused("pull request", iss=issue(pull_request={"url": "x"}))

    def test_closed_is_refused(self):
        self.assertRefused("not open", iss=issue(state="closed"))

    def test_stuck_is_refused(self):
        self.assertRefused("stuck", iss=issue(labels=[{"name": LABEL}, {"name": "notchling:stuck"}]))

    def test_timeline_is_paginated(self):
        filler = [commented(T0)] * 100
        self.assertOk(timeline=filler + [labeled("dtthaison", T1)])
        paths = [r["query"] for r in self.srv.requests if r["path"].endswith("/timeline")]
        self.assertIn("per_page=100&page=2", paths)

    def test_label_event_on_second_page_by_stranger_is_refused(self):
        filler = [commented(T0)] * 100
        self.assertRefused("not a maintainer", timeline=[labeled("dtthaison", T0)] + filler[:99]
                           + [labeled("stranger", T1)])

    def test_api_error_on_timeline_is_code_2(self):
        res = self.run_check(timeline_status=500)
        self.assertEqual(res.code, 2)

    def test_api_error_on_issue_is_code_2(self):
        res = self.run_check(issue_status=404)
        self.assertEqual(res.code, 2)

    def test_graphql_error_is_code_2(self):
        res = self.run_check(graphql_status=502)
        self.assertEqual(res.code, 2)

    def test_graphql_errors_field_is_code_2(self):
        res = self.run_check(edits={"errors": [{"message": "nope"}], "data": None})
        self.assertEqual(res.code, 2)

    def test_malformed_timestamp_fails_closed(self):
        res = self.run_check(timeline=[labeled("dtthaison", "yesterday")])
        self.assertEqual(res.code, 2)

    def test_connection_refused_is_code_2(self):
        api = preflight.Api("http://127.0.0.1:1", token="t")
        self.assertEqual(preflight.check("o/r", 5, api).code, 2)

    def test_token_goes_in_the_header_only(self):
        self.assertOk()
        for r in self.srv.requests:
            self.assertEqual(r["auth"], "Bearer read-token")
            self.assertNotIn("read-token", r["path"] + r["query"])

    def test_maintainers_come_from_the_gate(self):
        from guard.gate import MAINTAINERS
        self.assertIs(preflight.MAINTAINERS, MAINTAINERS)


class MainTests(unittest.TestCase):
    def run_main(self, argv, env=None):
        out, err = io.StringIO(), io.StringIO()
        code = preflight.main(argv, env or {}, out, err)
        return code, out.getvalue(), err.getvalue()

    def test_usage_errors_are_code_2(self):
        for argv in ([], ["o/r"], ["o/r", "x"], ["nope", "5"], ["o/r", "0"], ["o/r", "-3"],
                     ["../r", "5"], ["o/r/x", "5"]):
            self.assertEqual(self.run_main(argv)[0], 2, argv)

    def test_ok_prints_ok_and_exits_0(self):
        srv = FakeGitHub(routes_for())
        self.addCleanup(srv.close)
        code, out, _ = self.run_main(["o/r", "5"], {"NOTCHLING_API_URL": srv.url, "GITHUB_TOKEN": "t"})
        self.assertEqual((code, out.strip()), (0, "ok"))

    def test_refusal_prints_reason_and_exits_1(self):
        srv = FakeGitHub(routes_for(iss=issue(state="closed")))
        self.addCleanup(srv.close)
        code, out, err = self.run_main(["o/r", "5"], {"NOTCHLING_API_URL": srv.url})
        self.assertEqual(code, 1)
        self.assertIn("refused", err)

    def test_api_error_prints_reason_and_exits_2(self):
        srv = FakeGitHub(routes_for(timeline_status=500))
        self.addCleanup(srv.close)
        code, _, err = self.run_main(["o/r", "5"], {"NOTCHLING_API_URL": srv.url})
        self.assertEqual(code, 2)
        self.assertIn("refused", err)

    def test_reason_never_carries_api_error_body_text(self):
        srv = FakeGitHub(routes_for(timeline_status=500))
        self.addCleanup(srv.close)
        res = preflight.check("o/r", 5, preflight.Api(srv.url))
        self.assertNotIn("<", res.reason)


if __name__ == "__main__":
    unittest.main()
