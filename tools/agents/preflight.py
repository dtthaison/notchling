#!/usr/bin/env python3
"""Pre-start check for the worker: did the maintainer queue this issue, and is it unchanged since?

    preflight.py <owner/repo> <issue>

Reads the issue through the GitHub API (token from GITHUB_TOKEN, read access is enough; base URL from
NOTCHLING_API_URL for tests). It passes only if all of these hold:

- the issue is open, is not a pull request and does not carry `notchling:stuck`;
- `notchling:queued` is on it, and the latest `labeled` event for it was made by someone in MAINTAINERS
  and not undone by a later `unlabeled`;
- the body was not edited at or after that event.

Exit codes: 0 ok, 1 refused (reason on stderr), 2 API or usage error, which also counts as refused.
Issue text is never read into the reason."""

from __future__ import annotations

import json
import os
import re
import sys
import urllib.error
import urllib.request
from datetime import datetime
from typing import Any, Dict, List, Mapping, NamedTuple, Optional, TextIO

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "guard"))
from guard.gate import MAINTAINERS  # noqa: E402  (one list of maintainers, not a copy)

QUEUED = "notchling:queued"
STUCK = "notchling:stuck"
API_URL = "https://api.github.com"
PER_PAGE = 100
MAX_PAGES = 50  # 5000 timeline events; more than that is not an issue we start
TIMEOUT = 20
REPO_RE = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")


class ApiError(Exception):
    """The API could not be read or answered something unusable. The text is ours, never the server's."""


class Result(NamedTuple):
    code: int
    reason: str


class Api:
    def __init__(self, base_url: str = API_URL, token: Optional[str] = None):
        self.base_url = base_url.rstrip("/")
        self.token = token

    def request(self, method: str, path: str, body: Optional[Any] = None,
                token: Optional[str] = None, bearer: str = "Bearer") -> Any:
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(self.base_url + path, data=data, method=method)
        req.add_header("Accept", "application/vnd.github+json")
        req.add_header("X-GitHub-Api-Version", "2022-11-28")
        req.add_header("User-Agent", "notchling-agents")
        if data is not None:
            req.add_header("Content-Type", "application/json")
        tok = token or self.token
        if tok:
            req.add_header("Authorization", "%s %s" % (bearer, tok))
        try:
            with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
                raw = resp.read()
        except urllib.error.HTTPError as e:
            e.close()
            raise ApiError("GitHub API answered %d for %s %s" % (e.code, method, path.split("?")[0]))
        except (urllib.error.URLError, OSError, ValueError) as e:
            raise ApiError("GitHub API not reachable (%s)" % type(e).__name__)
        try:
            return json.loads(raw) if raw else None
        except ValueError:
            raise ApiError("GitHub API answered something that is not JSON")


def _time(value: Any) -> datetime:
    try:
        return datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ")
    except (TypeError, ValueError):
        raise ApiError("a timestamp from the API could not be read")


def _names(labels: Any) -> List[str]:
    if not isinstance(labels, list):
        raise ApiError("the issue's labels could not be read")
    return [l.get("name") if isinstance(l, dict) else l for l in labels]


def timeline(api: Api, repo: str, number: int) -> List[Dict[str, Any]]:
    events: List[Dict[str, Any]] = []
    for page in range(1, MAX_PAGES + 1):
        batch = api.request("GET", "/repos/%s/issues/%d/timeline?per_page=%d&page=%d"
                            % (repo, number, PER_PAGE, page))
        if not isinstance(batch, list):
            raise ApiError("the timeline could not be read")
        events.extend(e for e in batch if isinstance(e, dict))
        if len(batch) < PER_PAGE:
            return events
    raise ApiError("the timeline is too long to read")


EDITS_QUERY = ("query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name)"
               "{issue(number:$number){userContentEdits(first:100){nodes{editedAt deletedAt}"
               "pageInfo{hasNextPage}}}}}")


def body_edits(api: Api, repo: str, number: int) -> Dict[str, Any]:
    """Every recorded edit of the issue body. The REST timeline has no event for a body edit, so this is
    GraphQL `Issue.userContentEdits`, which GitHub fills each time the body (or a comment) is saved."""
    owner, name = repo.split("/")
    out = api.request("POST", "/graphql", {"query": EDITS_QUERY,
                                           "variables": {"owner": owner, "name": name, "number": number}})
    try:
        if out.get("errors"):
            raise ApiError("the edit history query was refused")
        conn = out["data"]["repository"]["issue"]["userContentEdits"]
        return {"times": [_time(n["editedAt"]) for n in conn["nodes"]],
                "more": bool(conn["pageInfo"]["hasNextPage"])}
    except (KeyError, TypeError, AttributeError):
        raise ApiError("the edit history could not be read")


def check(repo: str, number: int, api: Api) -> Result:
    try:
        return _check(repo, number, api)
    except ApiError as e:
        return Result(2, str(e))


def _check(repo: str, number: int, api: Api) -> Result:
    iss = api.request("GET", "/repos/%s/issues/%d" % (repo, number))
    if not isinstance(iss, dict):
        raise ApiError("the issue could not be read")
    if "pull_request" in iss:
        return Result(1, "this is a pull request, not an issue")
    if iss.get("state") != "open":
        return Result(1, "the issue is not open")
    labels = _names(iss.get("labels"))
    if STUCK in labels:
        return Result(1, "the issue is marked stuck")
    if QUEUED not in labels:
        return Result(1, "the queued label is not on the issue")

    events = timeline(api, repo, number)

    def is_queue_event(e: Dict[str, Any], kind: str) -> bool:
        return e.get("event") == kind and isinstance(e.get("label"), dict) \
            and e["label"].get("name") == QUEUED

    adds = [i for i, e in enumerate(events) if is_queue_event(e, "labeled")]
    if not adds:
        return Result(1, "no queued label event in the timeline")
    last = adds[-1]
    ev = events[last]
    actor = (ev.get("actor") or {}).get("login")
    if not isinstance(actor, str) or actor.lower() not in MAINTAINERS:
        return Result(1, "the queued label was added by %s, who is not a maintainer"
                      % (actor if isinstance(actor, str) and re.fullmatch(r"[A-Za-z0-9-]{1,39}", actor)
                         else "an unknown account"))
    if any(is_queue_event(e, "unlabeled") for e in events[last + 1:]):
        return Result(1, "the queued label was removed after it was added")
    labeled_at = _time(ev.get("created_at"))

    for e in events[last + 1:]:
        if e.get("event") == "edited" and _time(e.get("created_at")) >= labeled_at:
            return Result(1, "the issue was edited after it was queued")
    edits = body_edits(api, repo, number)
    if edits["more"]:
        return Result(1, "the edit history is too long to check")
    if any(t >= labeled_at for t in edits["times"]):
        return Result(1, "the issue body was edited after it was queued")
    return Result(0, "ok")


def main(argv: List[str], env: Mapping[str, str], out: TextIO, err: TextIO) -> int:
    if len(argv) != 2 or not REPO_RE.match(argv[0]) or not argv[1].isdigit() or int(argv[1]) < 1:
        err.write("usage: preflight.py <owner/repo> <issue>\n")
        return 2
    api = Api(env.get("NOTCHLING_API_URL") or API_URL, env.get("GITHUB_TOKEN") or None)
    res = check(argv[0], int(argv[1]), api)
    if res.code == 0:
        out.write("ok\n")
    else:
        err.write("refused: %s\n" % res.reason)
    return res.code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:], os.environ, sys.stdout, sys.stderr))
