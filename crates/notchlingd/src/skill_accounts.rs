use crate::skills_family::{minimal, KEY_RULE, REASON_RULE};

pub const NAME: &str = "notchling-accounts";

const DESCRIPTION: &str = "Use to see the accounts and lanes of your organization, or to switch, add, remove or move an account or a lane.";

const TITLE: &str = "Notchling accounts";

const ABOUT: &str = "Accounts are shown by masked email. A change asks the user on a card, and an account of another organization is never changed.";

const STEPS: &str = r#"
## Read first

```sh
'{binary}' accounts --json
'{binary}' lanes list --json
```

`accounts` lists the accounts of your organization per tool. Each has an `id` (`acct-` and 16 hex digits), a masked `name`, `active`, `needsLogin`, its `lane` and its `organization`. `lanes list` gives each Claude Code lane an `id` (`lane-` and 16 hex digits), its account and how many sessions run in it.

Refer to an account or a lane by its `id`, never by an email. The commands refuse anything else, and a card shows only a masked email. `main` names the account Claude Code uses without a lane.

`limits` is not here: it lives on `notchling-delegate`. Run `'{binary}' limits --json` to see whether a tool can run now.

## At a limit

When a tool is at its limit, wait for the reset that `limits` shows, or ask to switch to another account. Do not switch on your own, and never loop on `limits`.

## Change an account or a lane

Switching and logging in always goes through a card. Every change here, `accounts switch`, `add`, `remove`, `move-all` and `move`, and `lanes create`, `remove` and `switch`, always asks the user on a card that names the tool, the account by a masked email, the organization and the change. No account change is free for an agent, and an account of another organization is never changed.

1. Check it first. A dry run changes nothing and opens no card.
2. Send it with `--reason` and `--key`, and `--no-wait` to get exit 6 at once with the request id.
3. Wait in the background, once, with `consent` or `watch`; do not poll in a loop.

```sh
'{binary}' accounts switch <account> --dry-run --json
'{binary}' accounts add claude --dry-run --json
'{binary}' accounts remove <account> --dry-run --json
'{binary}' accounts move-all <account> --dry-run --json
'{binary}' accounts move <session> <lane> --dry-run --json
'{binary}' lanes create <account> --dry-run --json
'{binary}' lanes remove <lane> --dry-run --json
'{binary}' lanes switch <lane> <account> --dry-run --json
```

Then the same line with `--reason "<what and why>" --key <key> --no-wait --json` instead of `--dry-run`, and:

```sh
'{binary}' consent <id> --wait --timeout 600 --json
```

- `consent <id> --wait` ends with exit 0 when the user allowed it and it applied, exit 5 when the user said no (stop, and do not ask again at once), exit 6 while it still waits and exit 124 when the time passes (the request is closed).
- On exit 124 ask again only if you still need it, with a new `--key`.
- If you were denied with a note, change what it says and ask again with `--retry-of <id>`.
- `accounts add` opens the login for the user to finish in the tool's own window. You never type or read the login.

## When it is refused

Exit 4 with `error.reason` `switch_off` means Logins and Lanes is off. Run the command in `error.ask` once: it opens a card for the user. Never run it in a loop.

Exit 4 with `other_organization` or `no_session` is not fixable by asking: the account is in another organization, or you are not in a verified session.

## Never

- Never refresh a token. Refresh tokens rotate, and the session that holds the old one is signed out.
- Never ask for a password in the chat, and never put one in a reason. The user logs in on the tool's own screen.
- Never read or copy a token, `~/.claude.json` or the Keychain item `Claude Code-credentials`. A secret you need comes from `notchling-vault`.

## What to write

- {reason} 300 characters or fewer, and never a secret.
- {key} One key per logical request; a new request gets a new key."#;

pub fn content(binary: &str) -> String {
    let base = minimal(NAME, DESCRIPTION, TITLE, ABOUT, binary);
    let steps = STEPS
        .replace("{binary}", binary)
        .replace("{reason}", REASON_RULE)
        .replace("{key}", KEY_RULE);
    match base.rfind("\nRun `") {
        Some(at) => format!("{}{}{}", &base[..at], steps, &base[at..]),
        None => format!("{base}{steps}"),
    }
}

#[cfg(test)]
#[path = "skill_accounts_tests.rs"]
mod tests;
