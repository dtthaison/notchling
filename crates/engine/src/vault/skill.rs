//! The text of the `notchling-vault` skill (Swift `VaultSkill`): the vault exists, which names it
//! holds, how to pick one, how to use it without reading it, and how to ask the user for a missing
//! one. The Swift text with the `notchling` subcommands (`secrets`, `secret`, `secret-request`) in
//! place of the app's `--secrets`/`--secret`/`--secret-request` flags.
//!
//! This is only the text. Installing it into the tools' skills folders belongs to E13 (`install`),
//! and nothing here writes a file.

use super::entry::VaultEntry;
use super::list::label;

/// The skill's name and its folder name.
pub const NAME: &str = "notchling-vault";

/// Whose skills folder the text is for; each gets a note on its sandbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillTool {
    Claude,
    Codex,
    Antigravity,
}

impl SkillTool {
    pub const ALL: [SkillTool; 3] = [SkillTool::Claude, SkillTool::Codex, SkillTool::Antigravity];

    /// What the agent must know about running `notchling` from inside this tool's sandbox.
    pub fn sandbox_note(self) -> &'static str {
        match self {
            SkillTool::Claude => "",
            SkillTool::Codex => "\n## In Codex's sandbox\n\nNotchling's command talks to the Notchling daemon over a local socket, and the commands that use a secret usually need the network, so neither works inside Codex's workspace-write sandbox without network (\"isn't running\" means the sandbox blocked it). Run `secrets`, `secret` and `secret-request` with escalated permissions (outside the sandbox) and give the user a one-line reason; if this session can't escalate, tell the user it needs network access.\n",
            SkillTool::Antigravity => "\n## In Antigravity\n\nRun these through the terminal tool. If the session runs with `--sandbox` and the call says \"isn't running\", the sandbox blocked the local socket; ask the user to allow the command.\n",
        }
    }
}

/// The names the description lists: `github (api.github.com)`, sorted.
pub fn names(entries: &[VaultEntry]) -> Vec<String> {
    let mut names: Vec<String> = entries.iter().map(label).collect();
    names.sort();
    names
}

/// The skill's text, with `binary` (the `notchling` program's path) and the saved `names`.
pub fn content(binary: &str, names: &[String], tool: SkillTool) -> String {
    let saved = if names.is_empty() {
        "none yet".to_string()
    } else {
        names.join(", ")
    };
    TEMPLATE
        .replace("{saved}", &saved)
        .replace("{binary}", binary)
        .replace("{sandbox_note}", tool.sandbox_note())
}

const TEMPLATE: &str = r#"---
name: notchling-vault
description: Use whenever a task needs a password, API token, username or other credential (logging in, calling an API, publishing a package, pushing to a registry, a .env value) — instead of asking the user to paste one into the chat. The user keeps credentials in Notchling's Secret Vault; saved now: {saved}.
---

# Notchling Secret Vault

The user keeps credentials in Notchling so they never go through the chat. Only Notchling can read them. You use one by name and never see its value. Never ask the user to type a secret into the chat.

## 1. Find the right one

```sh
'{binary}' secrets '<URL, host or words from the task>'
```

Prints the saved secrets that fit, best first (exit code 1 when none does). Each is a name, its kind (login or token) and the environment variables it sets, then its URL (also in `$<NAME>_URL`), the user's note, how to get or renew it, its expiry (`renew soon` within 7 days, `EXPIRED` once past: ask the user to renew it), and whether the user approves each use. `secrets` alone lists them all. Values are never printed.

## 2. Missing or wrong? Ask the user to add or update it

```sh
'{binary}' secret-request <name> --kind token|login --url '<url>' --note '<what it is for>' --how '<steps to get it>' --expires YYYY-MM-DD --reason '<why you need it now>'
```

Opens Notchling's form with those fields filled in; the user types the value there. Whenever you ask for a token, fill `--how` with the steps for getting it (where to click, which scopes to tick, a link to the page): the form shows it under the value field. Add `--expires` whenever you know the last day; Notchling reminds the user before it. `--note` and `--how` may span lines. For a name that exists (an expired token, a changed password) it opens that secret for editing: each of `--note`, `--url`, `--how` and `--expires` you pass replaces the saved text in the form, the others keep their saved values, and the user saves or cancels. It waits for the user, so give it the longest timeout your shell tool allows (Claude Code's Bash: 600000 ms). It prints the saved secret (exit 0) or says the user cancelled (exit 1). Use names like `github`, `npm`, `aws-prod`.

## 3. Use it

```sh
'{binary}' secret <name>[,<name>…] -- <command> [args…]
```

The command runs with the secret's variables set. With a single argument after `--` it runs through `sh -c`, so single-quote it and let that shell expand the variables:

```sh
'{binary}' secret github -- 'curl -sf -H "Authorization: Bearer $GITHUB_TOKEN" "$GITHUB_URL/user"'
'{binary}' secret registry -- 'printf %s "$REGISTRY_PASSWORD" | docker login -u "$REGISTRY_USERNAME" --password-stdin "$REGISTRY_URL"'
```

Its output comes back with every value replaced by `[secret:<name>]`; its exit code is the command's. When the secret says the user approves each use, Notchling shows them the command first and the call waits (exit 3 when they deny it), so give it a long timeout too.

Rules:
- Never echo, print, encode, log or write a value to a file, a commit, a URL you show, or your reply. Don't put a value on a command line where `ps` would see it when a tool takes it from the environment or stdin.
- Don't try to read the vault's Keychain item or work around Notchling. A command whose purpose is to reveal a value is misuse even if its output is hidden.
- Notchling must be running; if it isn't, tell the user.
{sandbox_note}"#;
