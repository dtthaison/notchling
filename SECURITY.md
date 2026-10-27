# Security Policy

Notchling handles real logins: it reads and writes Claude Code's Keychain item, `~/.claude.json`, Claude Desktop's cookie database and Antigravity's Google login, and keeps copies of saved accounts in its own Keychain items. A bug here can sign someone out or expose a token, so security reports are taken seriously.

## Supported versions

Only the latest release, and `main`, get security fixes.

## Reporting a vulnerability

Please **don't open a public issue** for a security problem.

- Preferred: use [Report a vulnerability](https://github.com/dtthaison/notchling/security/advisories/new) on the repo's Security tab (GitHub private vulnerability reporting).
- Or email dtthaison@gmail.com with "Notchling security" in the subject.

Include what you found, how to reproduce it, and the macOS and Notchling versions. Please don't include real tokens or cookies; redacted examples are fine.

You should hear back within 7 days. Once the issue is confirmed, a fix is released as soon as practical and you'll be credited in the release notes unless you'd rather not be.

## What's in scope

- Tokens, cookies or account data written anywhere other than the Keychain and the files listed in the README, or with looser permissions than needed.
- Any network call that sends credentials somewhere other than Anthropic's own endpoints, or anything leaving the Mac for Antigravity (its limits come only from the local language server).
- The `--hook` process: anything that lets another process answer a permission prompt, or makes Claude Code run a tool the user didn't approve.
- Account switching that refreshes the token Claude Code is using, or damages other entries in `Claude Code-credentials` or `~/.claude/settings.json`.

## What's out of scope

- Problems in Claude Code, Claude Desktop or Antigravity themselves; please report those to their makers.
- Attacks that need someone who already runs code as your user (they can read the Keychain items and files directly).
