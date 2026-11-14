---
name: add-provider
description: Use when adding a tool to Notchling (another CLI or app, like Codex or Antigravity, whose sessions, logins and limits Notchling should follow), or when closing a gap in the provider matrix for a tool.
---

A tool is a value of `contract::Tool` and 39 declared capabilities. The design is in `docs/decisions/` ("One capability contract for every tool") and the table of what each tool has today is `docs/provider-matrix.md`. Every file here is protected: the PR waits for the maintainer, and it should. A provider touches someone's real login.

Before starting, read `crates/engine/AGENTS.md` (the login rules, the Contract notes) and look at the smallest existing declaration, `crates/engine/src/contract/codex/`.

1. **The tool.** Add the `Tool` value in `crates/engine/src/contract/tool.rs` with its `name`, `title` and row-id prefix. The build now fails in every `contract/<tool>/support.rs`, in `Registry::standard` and in the conformance macro list: that is the checklist.
2. **The declaration.** Write `crates/engine/src/contract/<tool>/support.rs` and `mod.rs` (a provider with `WIRED`): every capability Native, Fallback or Not applicable, each with its one-line reason (120 characters at most). No Gap for a new tool; `gaps.json` and `unwired.json` must not name it, so its Native and Fallback cells are wired in the same change. Never a `_` arm.
3. **The ports.** Implement each Native capability over the tool's own files and APIs, with effects behind the existing seams (`Launcher`, `FakeSpawner`, `MemoryKeychain`, `ScriptedHttp`). Never renew a token that is in use; keep logins out of tests.
4. **The fixtures and the check.** Make `crates/engine/tests/fixtures/conformance/<tool>/` with made-up data (an idle session, a working one, one with a running and a finished subagent), add the check for each wired cell in `crates/engine/src/contract/conformance.rs`, and add the line `conformance!(<module>, Tool::<Variant>, "<name>");` to `crates/engine/tests/conformance/main.rs` (the one edit that file takes).
5. **Accounts and limits.** An `AccountProvider` and a `ProviderSource` (the login rules in `crates/engine/AGENTS.md`); models: a `CatalogSource` and the tool's rows in `crates/engine/data/catalog.json`.
6. **Setup and versions.** An `instructions::Target` (decision 0019) and the tool's lines in the delegate skill; a version reader and installer (the version-sync design).
7. **Checks.** Regenerate the matrix and read the diff, then run the checks:
   ```sh
   NOTCHLING_UPDATE_GOLDEN=1 cargo test -p notchling-engine --test provider_matrix
   cargo test --workspace --locked
   cargo clippy --workspace --all-targets -- -D warnings
   PYTHONPATH=tools/guard python3 -m guard lint
   ```
   The capability check will flag the provider's process, network and Keychain use. That's expected; list each use in the PR and why it's needed.
8. **Docs.** Add the tool's facts to "Things that bit before" in `crates/engine/AGENTS.md`, its row to `docs/feature-map.md`, and the tool to `README.md`.

Closing a gap for an existing tool: change its cell in `support.rs`, route it (add it to `WIRED`, cut it from `unwired.json`), remove it from `gaps.json`, add its conformance check, regenerate the matrix.

Tried: not yet
