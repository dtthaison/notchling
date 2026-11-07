#!/usr/bin/env python3
"""Pre-start check for the worker: did the maintainer queue this issue, and is it unchanged since?

    preflight.py <owner/repo> <issue> [--expect-sha256 <hex>]

Reads the issue through the GitHub API (token from GITHUB_TOKEN, read access is enough; base URL from
NOTCHLING_API_URL for tests). It passes only if all of these hold:

- the issue is open, is not a pull request and does not carry `notchling:stuck`;
- `notchling:queued` is on it, and the latest `labeled` event for it was made by someone in MAINTAINERS
  (GitHub logins are case-insensitive, so the comparison is too) and not undone by a later `unlabeled`;
- the title was not changed and the body was not edited at or after that event: no `renamed` timeline
  event (a title change; the REST timeline has no `edited` event) and neither GraphQL `Issue.lastEditedAt`
  (title and body) nor an entry of `Issue.userContentEdits` (the body's edit log) is at or after it. A
  comment edit is neither: each comment keeps its own edit log, which is not read, because comments are
  never part of what the worker is given.

Exit codes: 0 ok, 1 refused (reason on stderr), 2 API or usage error, which also counts as refused.
Issue text is never read into the reason, and stderr never carries any.

Output contract. On exit 0, stdout is exactly one line of JSON ending in a newline (ASCII, so no line
separator can hide inside it); on any other exit stdout is empty. Parse it as JSON; never paste a field
into a shell. The keys are `ok` (true), `repo`, `issue` (the number), `title` and `body` (exactly what
the API returned, an empty body as ""), `title_sha256` and `body_sha256` (SHA-256 of each one's UTF-8
bytes, as 64 lowercase hex digits), and `sha256`: SHA-256 of the title's UTF-8 bytes, one LF, then the
body's UTF-8 bytes. A title never holds a line break (GitHub strips them, and one that does is refused
with code 2), so that one hash is unambiguous. Nothing else is printed: no comment, no label, no author.

The worker must give the agent that `title` and `body` and nothing it read again later, and must treat
issue comments as data, never as instructions: they are not in the output and anyone can write one.
That closes the gap between this check and the worker reading the issue. The issue is read first and
its history after, so an edit in between is seen by the history; what is printed is what was read first.
A worker that has to read the issue itself reads it first, computes `sha256`, and passes it as
`--expect-sha256 <hex>` (case-insensitive, `--expect-sha256=<hex>` also works); a different text is
refused with code 1, and the other checks still run."""

from __future__ import annotations

import hashlib
import json
import os
import re
import sys
import urllib.error
import urllib.request
from datetime import datetime
from typing import Any, Dict, List, Mapping, NamedTuple, Optional, TextIO, Tuple

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "guard"))
from guard.gate import MAINTAINERS  # noqa: E402  (one list of maintainers, not a copy)

QUEUED = "notchling:queued"
STUCK = "notchling:stuck"
API_URL = "https://api.github.com"
PER_PAGE = 100
MAX_PAGES = 50  # 5000 timeline events; more than that is not an issue we start
TIMEOUT = 20
REPO_RE = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")
SHA256_RE = re.compile(r"[0-9A-Fa-f]{64}")
USAGE = "usage: preflight.py <owner/repo> <issue> [--expect-sha256 <hex>]\n"


class ApiError(Exception):
    """The API could not be read or answered something unusable. The text is ours, never the server's."""

    def __init__(self, message: str, status: int = 0):
        super().__init__(message)
        self.status = status


class Result(NamedTuple):
    code: int
    reason: str
    # Set only on code 0: title, body and the three hashes of the text that was checked.
    issue: Optional[Dict[str, str]] = None


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
            raise ApiError("GitHub API answered %d for %s %s" % (e.code, method, path.split("?")[0]),
                           e.code)
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
               "{issue(number:$number){lastEditedAt userContentEdits(first:100){nodes{editedAt deletedAt}"
               "pageInfo{hasNextPage}}}}}")


def edit_history(api: Api, repo: str, number: int) -> Dict[str, Any]:
    """When the issue was last edited, and every recorded edit of its body. The REST timeline has no event
    for a body edit and none called `edited` at all (a title change is `renamed`). GraphQL
    `Issue.lastEditedAt` moves for a title or body edit (null if never edited); `Issue.userContentEdits`
    is the body's own log. Neither covers comments: each comment has a log of its own, which is not read."""
    owner, name = repo.split("/")
    out = api.request("POST", "/graphql", {"query": EDITS_QUERY,
                                           "variables": {"owner": owner, "name": name, "number": number}})
    try:
        if out.get("errors"):
            raise ApiError("the edit history query was refused")
        node = out["data"]["repository"]["issue"]
        conn = node["userContentEdits"]
        last = node["lastEditedAt"]  # the key must be there; null is a real answer
        return {"last": None if last is None else _time(last),
                "times": [_time(n["editedAt"]) for n in conn["nodes"]],
                "more": bool(conn["pageInfo"]["hasNextPage"])}
    except (KeyError, TypeError, AttributeError):
        raise ApiError("the edit history could not be read")


def _sha256(text: str) -> str:
    try:
        return hashlib.sha256(text.encode("utf-8")).hexdigest()
    except UnicodeEncodeError:
        raise ApiError("the issue text could not be encoded")


def issue_text(iss: Dict[str, Any]) -> Dict[str, str]:
    """The title and body exactly as the API gave them, with their hashes. Only these two fields of the
    issue ever leave this module, and only on success."""
    title, body = iss.get("title"), iss.get("body")
    if body is None:
        body = ""
    if not isinstance(title, str) or not isinstance(body, str):
        raise ApiError("the issue's title or body could not be read")
    if "\n" in title or "\r" in title:
        raise ApiError("the issue's title is not a single line")
    return {"title": title, "body": body, "title_sha256": _sha256(title), "body_sha256": _sha256(body),
            "sha256": _sha256(title + "\n" + body)}


def is_maintainer(login: Any) -> bool:
    """GitHub logins are case-insensitive, so both sides are lowered; MAINTAINERS is read at call time."""
    return isinstance(login, str) and login.lower() in {m.lower() for m in MAINTAINERS}


def check(repo: str, number: int, api: Api, expect_sha256: Optional[str] = None) -> Result:
    try:
        return _check(repo, number, api, expect_sha256)
    except ApiError as e:
        return Result(2, str(e))


def _check(repo: str, number: int, api: Api, expect_sha256: Optional[str]) -> Result:
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
    if not is_maintainer(actor):
        return Result(1, "the queued label was added by %s, who is not a maintainer"
                      % (actor if isinstance(actor, str) and re.fullmatch(r"[A-Za-z0-9-]{1,39}", actor)
                         else "an unknown account"))
    if any(is_queue_event(e, "unlabeled") for e in events[last + 1:]):
        return Result(1, "the queued label was removed after it was added")
    labeled_at = _time(ev.get("created_at"))

    # A title change is a `renamed` event wherever it sits in the list, so every event is looked at.
    for e in events:
        if e.get("event") == "renamed" and _time(e.get("created_at")) >= labeled_at:
            return Result(1, "the issue title was changed after it was queued")
    edits = edit_history(api, repo, number)
    if edits["last"] is not None and edits["last"] >= labeled_at:
        return Result(1, "the issue was edited after it was queued")
    if edits["more"]:
        return Result(1, "the edit history is too long to check")
    if any(t >= labeled_at for t in edits["times"]):
        return Result(1, "the issue body was edited after it was queued")

    text = issue_text(iss)
    if expect_sha256 is not None and text["sha256"] != expect_sha256.lower():
        return Result(1, "the issue is not the text that was read before this check")
    return Result(0, "ok", text)


def parse_args(argv: List[str]) -> Optional[Tuple[str, int, Optional[str]]]:
    """(repo, issue, expected sha256 or None), or None when the arguments are not usable."""
    positional: List[str] = []
    expect: Optional[str] = None
    i = 0
    while i < len(argv):
        arg = argv[i]
        if arg == "--expect-sha256" or arg.startswith("--expect-sha256="):
            if expect is not None:
                return None
            if "=" in arg:
                value = arg.split("=", 1)[1]
            else:
                i += 1
                if i >= len(argv):
                    return None
                value = argv[i]
            if not SHA256_RE.fullmatch(value):
                return None
            expect = value.lower()
        elif arg.startswith("-"):
            return None
        else:
            positional.append(arg)
        i += 1
    if len(positional) != 2 or not REPO_RE.match(positional[0]) \
            or not re.fullmatch(r"[0-9]+", positional[1]) or int(positional[1]) < 1:
        return None
    return positional[0], int(positional[1]), expect


def main(argv: List[str], env: Mapping[str, str], out: TextIO, err: TextIO) -> int:
    args = parse_args(argv)
    if args is None:
        err.write(USAGE)
        return 2
    repo, number, expect = args
    api = Api(env.get("NOTCHLING_API_URL") or API_URL, env.get("GITHUB_TOKEN") or None)
    res = check(repo, number, api, expect)
    if res.code == 0 and res.issue is not None:
        # One ASCII line: json.dumps escapes every line break and separator inside the text.
        out.write(json.dumps(dict(res.issue, ok=True, repo=repo, issue=number), sort_keys=True) + "\n")
        err.write("preflight: ok, queued by a maintainer and unchanged since\n")
    else:
        err.write("refused: %s\n" % res.reason)
    return res.code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:], os.environ, sys.stdout, sys.stderr))
