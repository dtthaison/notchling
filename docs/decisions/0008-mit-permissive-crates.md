# 0008: Stay MIT, permissive crates only

## Context

Notchling is MIT. The Rust rebuild brings in crates, and some of the obvious ones are not permissive. Zed's `terminal`, `terminal_view` and `markdown` crates are GPL-3. `gpui` and `alacritty_terminal` are fine.

From the design: [Architecture](https://github.com/dtthaison/notchling/blob/d285219/docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md#architecture), "License".

## Decision

- Notchling stays MIT.
- Only MIT, Apache and BSD crates. `cargo-deny` enforces it as a required check, along with security advisories.
- Zed's `terminal`, `terminal_view` and `markdown` are not used. We write our terminal view and markdown view on permissive crates: `alacritty_terminal`, `pulldown-cmark`, `tree-sitter`.

## Consequences

- More code of our own: the terminal view and the markdown view.
- A crate under any other license fails CI. The `cargo-deny` config, `Cargo.toml` and `Cargo.lock` are protected, so loosening the list needs the maintainer ([0002](0002-the-line.md)).
- Lockfile updates never auto-merge.
- There is no Rust code in the tree yet (the throwaway spike is on branch `spike/gpui-phase1`), and there is no `cargo-deny` yet. It comes with the Cargo workspace in Phase 4.

## Status

Accepted. The maintainer decided to stay MIT and allow only MIT, Apache and BSD licenses *(decided)*.

## Date

2026-11-07
