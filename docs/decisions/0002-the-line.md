# 0002: The line

## Context

Agents will merge their own pull requests. Something has to decide what may land without the maintainer.

The first draft of the design listed protected folders. The reviews found that this was the wrong way round. Login switching, OAuth token use, trust prompts and process launching sat in "safe" folders. And the Swift app is one module, so any file can call anything.

From the design: [The line](https://github.com/dtthaison/notchling/blob/d285219/docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md#the-line-decided).

## Decision

Agents may merge on their own **only in named safe areas**. Everything else waits for the maintainer.

- `CODEOWNERS` starts with `* @dtthaison`. Only the auto-merge zone is listed without an owner.
- The auto-merge zone is a list of named paths: views, the design kit, search, the transcript readers, `docs/` except the manual's layers, the phone's features except pairing, and new tests (not changes to existing ones). In the Rust tree: `crates/app/` views, `crates/kit/`, and the transcript and search modules of `crates/engine/`.
- A **capability check** sends a PR to the maintainer, wherever the file is, when it adds a new use of:
  - `Process`, `forkpty` or `posix_spawn`;
  - `NSAppleScript` or `osascript`;
  - `URLSession` or other network clients;
  - `SecItem*`, `/usr/bin/security` or `keyring`;
  - `FileManager` writes outside the app's folders;
  - a protected type (`VaultStore`, `AccountStore`, `LaneStore`, `ReplySender`, `HookBridge`, `OwnerPublish`) from outside its own files.
- **`maintainer-gate`** is a required check. It passes only when the PR touches nothing protected and trips no capability, or when the maintainer approved the PR's current head commit.
- Labels are informational only, since anyone with pull-request write can remove a label.

## Consequences

- A new file or folder is protected until someone names it in the zone. A wrong guess fails safe.
- The zone lives in `tools/guard/zones.json`, and `.github/CODEOWNERS` is generated from it. Both are protected, so widening the zone needs the maintainer.
- A push after the maintainer's approval needs a new approval, because the approval is bound to the head commit.
- In the Rust tree, crate boundaries and `cargo-deny` take over most of the capability check.
- Built in Phase 2: `zones.json`, the generated `CODEOWNERS`, `python3 -m guard gate` (protected paths, capability check, test weakening) and `.github/workflows/maintainer-gate.yml`.

## Status

Accepted. The maintainer decided the line *(decided)*.

The exact files in the zone are the designer's call ("Calls I made" in the design). Changing them is an edit to `zones.json`, which waits for the maintainer.

## Date

2026-11-07
