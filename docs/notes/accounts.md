# Accounts

Account switching and the account log. The rules that keep a real login safe are in the root `AGENTS.md`; each file's notes are in `crates/engine/AGENTS.md` (`accounts/`).

Part of the Rust app's notes, moved from `crates/AGENTS.md`; the index is in the root `AGENTS.md`. Keep it true: change the note with the code, and add what bites you under Things that bit before.

## Things that bit before

- `security -i` reads 4 KB lines, and Claude Code's login item also holds MCP logins, so as hex it passed that and every switch was refused. It goes as quoted text on stdin, never argv; long saved logins split over several items.
- Account switches went unrecorded from 2026-11-11 (the Swift app's retirement) to this fix: `account-log.json`, which cost reads to know who was active when, had a reader in `crates/` and no writer, so every Claude Code message after the last Swift entry was charged to that account. Nothing failed and no test ran the switch and then looked at the file. The old `audit.jsonl` could not restore the gap: it recorded only the op name, time, tier and result, never the account, so the log started again from the account found live at the first sync after the daemon started. Anything that changes Claude Code's login goes through `ClaudeAccounts` and its `note_active`; a test of a new account path reads the `account.switched` events afterwards (`notchlingd/src/accounts_ops_log_tests.rs`).
