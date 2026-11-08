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

## Note, 2026-11-08: named exceptions for GPUI's tree

GPUI 0.2.2 (part U0) brings crates outside the list into the dependency tree. Linked into the macOS app: `option-ext` (MPL-2.0, through `dirs` and `zed-font-kit`, which finds the user's fonts). Build time only, never linked: `cbindgen` (MPL-2.0, GPUI's build script writes its Metal shader header) and `hexf-parse` (CC0-1.0, through `naga` in the same script). Compiled by no build of ours but still in `cargo-deny`'s graph, because it filters each dependency edge by target on its own while GPUI is a macOS-only dependency here: `dwrote` (MPL-2.0, Windows font-kit) and `tiny-keccak` (CC0-1.0, Linux x11 input methods).

The maintainer approved named license exceptions in `deny.toml` for exactly these crates on 2026-11-08, rather than widening the allow list. Each is pinned to the version in `Cargo.lock` today (`option-ext@0.2.0` and so on), so a new version of the same crate needs a new look. Why that is acceptable: MPL-2.0 is file-level copyleft that reaches only its own files, which we use unmodified with their source on crates.io, so Notchling stays MIT and no obligation falls on anyone building it; CC0-1.0 is a public-domain dedication. A new exception takes the maintainer, as before.

Two technical changes to `deny.toml` came with it, and neither widens what is allowed. `[graph] targets` narrows the check to the triples we build, a technical narrowing and not a policy change. And `[advisories] unmaintained = "all"` stays strict: the unmaintained crates that GPUI 0.2.2's tree brings (async-std, paste, rustybuzz, ttf-parser, the zbus family and a few more) are ignored one by one by their RUSTSEC id, each with its reason, so any other advisory, and any of our own direct dependencies going unmaintained, still fails. Vulnerability advisories stay errors.

## Date

2026-11-07
