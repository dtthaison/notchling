# Decision records

One record per decision: why Notchling is built the way it is. [ARCHITECTURE.md](../../ARCHITECTURE.md) says how the parts fit; these say why.

Most of them come from the design, [revision 2](https://github.com/dtthaison/notchling/blob/d285219/docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md), which the maintainer approved on 2026-11-07.

## Index

| # | Decision | Status |
| --- | --- | --- |
| [0001](0001-gpui-spike.md) | GPUI spike: the rebuild goes ahead | Accepted |
| [0002](0002-the-line.md) | The line: agents merge only in named safe areas; everything else waits for `maintainer-gate` | Accepted |
| [0003](0003-agent-instructions-in-agents-md.md) | Agent instructions live in `AGENTS.md`; `CLAUDE.md` is one line, `@AGENTS.md` | Accepted |
| [0004](0004-notchlingd-engine-service.md) | The Rust engine runs as the always-on `notchlingd` service | Accepted |
| [0005](0005-gpui-desktop-app.md) | GPUI for every desktop app, web content in `wry` windows | Accepted |
| [0006](0006-flutter-phone-generated-types.md) | Flutter phone with generated protocol types and no Rust | Proposed |
| [0007](0007-local-api-tiers.md) | Local API with an app tier and an agent tier, callers known by executable path | Accepted |
| [0008](0008-mit-permissive-crates.md) | Stay MIT; only MIT, Apache and BSD crates, enforced by `cargo-deny` | Accepted |
| [0009](0009-rulesets-as-code.md) | Rulesets as code in `.github/rulesets/`, applied by the maintainer | Accepted |
| [0010](0010-build-then-sign-beta-then-stable.md) | Build, then sign apart; Beta daily, Stable by one click | Accepted |
| [0011](0011-one-owner-per-resource.md) | One owner per resource while the Swift app and the daemon run side by side | Accepted |
| [0012](0012-remote-computers.md) | Remote computers: identity by certificate, host-qualified session ids, nothing replicated | Proposed |
| [0013](0013-page-panel-web-view.md) | A web view in Chat's page panel, and the live claude.ai page | Proposed |
| [0014](0014-app-data-in-sqlite.md) | The app's own data in SQLite, and search by bm25 | Accepted |

0001 was written before records had a Status section. Its status is Accepted.

## Status

- **Proposed**: written down, not settled. The record says what would settle it.
- **Accepted**: in force. The maintainer decided it (*(decided)* in the design), or it came from the reviews (*(review)*) and is part of the approved revision 2.
- **Superseded by NNNN**: a newer record replaced it.

The design marks some choices *(my call)*: the designer's, open to change. A record whose core is one of those is Proposed. An Accepted record that contains one names it in its Status.

## Changing a decision

A decision isn't edited in place. To change one, write a new record with the next number. Say which record it replaces and why. Then set the old record's status to "Superseded by NNNN"; that is the only edit the old record gets.

The other edit a record gets is Proposed to Accepted, when the maintainer accepts it.

Everything under `docs/decisions/` is protected, so a new record or a status change waits for the maintainer ([0002](0002-the-line.md)).

## Template

File name: `NNNN-short-slug.md`, with the next free number.

```markdown
# NNNN: Short title

## Context

What forces the decision. Facts first. Link the design section, issue or record it comes from.

## Decision

What we do, in short sentences or a list.

## Consequences

What follows, good and bad. What exists today and what is planned.

## Status

Proposed, Accepted, or Superseded by NNNN. Who decided it.
For Proposed: what would settle it.

## Date

YYYY-MM-DD
```
