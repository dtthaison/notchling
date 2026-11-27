use crate::skills_family::{minimal, KEY_RULE, REASON_RULE};

pub const NAME: &str = "notchling-sessions";

const DESCRIPTION: &str = "Use to follow, answer or control the agent sessions Notchling shows: list them, start one, reply to one, stop, remove or resume one, answer its question, run a slash command in it, or read this session's shells, prompts, queued messages and delivery receipts.";

const TITLE: &str = "Notchling sessions";

const ABOUT: &str = "A session is one agent run that Notchling shows. A free command acts on a session you started; any other session asks the user on a card.";

const STEPS: &str = r#"
## Find a session

```sh
'{binary}' sessions --json
'{binary}' search <words> --kind sessions --since 7d --json
'{binary}' sessions history --limit 30 --json
```

`sessions` lists the sessions of your organization. `search` finds one by what was said in it; `--since` takes a duration (`30m`, `24h`, `7d`), a date (`2026-11-01`) or epoch seconds. Use the `id` it prints. `sessions history` lists what the Chat Sidebar hides: sessions that ended in the last day, then older Claude Code sessions of your organization from the last 14 days, newest first, with masked titles; pass its `next` as `--before <cursor>` for the next page (`--limit` is 1 to 100). Use `session resume <id>` to resume an ended Claude Code session.

## Whose session it is

- `self` is the session you run in. Only `session reload` takes it: `'{binary}' session reload self --after-turn` restarts you once your turn ends.
- A session you started with `session new` is yours to drive. Replying to it is free, up to 30 an hour, then exit 7; stopping or interrupting it is free; removing it is free once it is stopped; resuming it is free once it has ended. Your own session (`self`) is never yours to reply to, remove or resume: that is exit 2.
- Any other session, and every `session answer`, `command`, `dialog`, `chrome` and `options`, even on your own child, asks the user on a card. `help --json` shows which: `own_free` is free for a session you started, `request` is always a card.

## Do one thing safely

1. Check it first. A dry run changes nothing and opens no card, and fails as the real call would. It says `would: "apply"` when the act is free and `would: "ask"` when a card opens.
2. Send it with `--reason` and `--key`.
3. A free act prints one sentence and exits 0. An act that needs a card waits for the user. Add `--no-wait` to get exit 6 at once with the request id, then wait in the background with `consent` or `watch`; do not poll in a loop.

```sh
'{binary}' session new <folder> --message "<task in a few words>" --dry-run --json
'{binary}' session reply <id> "<text>" --dry-run --json
'{binary}' session stop <id> --dry-run --json
'{binary}' session remove <id> --dry-run --json
'{binary}' session resume <id> --dry-run --json
'{binary}' session answer <id> --answers <json> --dry-run --json
'{binary}' session command <id> <command> --dry-run --json
'{binary}' session dialog <id> --pick <1-9> --dry-run --json
'{binary}' session chrome <id> on --dry-run --json
'{binary}' session options <id> <option> <value> --dry-run --json
```

Then the same line with `--reason "<what and why>" --key <key> --no-wait --json` instead of `--dry-run`, and:

```sh
'{binary}' consent <id> --wait --timeout 600 --json
'{binary}' watch --topic mine.consents --until applied --timeout 600 --json
```

- `consent <id> --wait` exits 0 when the user allowed it and it applied, 5 when the user said no (stop then, and do not ask again at once), 6 while it still waits and 124 when the time passes (the request is closed).
- `watch` exits 0 once a request of yours is applied and 124 if none is by `--timeout`. It cannot say no: use it for several requests at once, and `consent <id> --wait` for one.
- Without `--no-wait`, the command itself waits and exits the same way.
- On exit 124 the request is closed: ask again only if you still need it, with a new `--key`.
- If you were denied with a note, change what it says and ask again with `--retry-of <id>`.

## When it is refused

Exit 4 with `error.reason` `switch_off` means Act on Sessions is off. Run the command in `error.ask` once: it opens a card for the user. Never run it in a loop. Exit 4 with `error.never` true is final: only the user may do that, such as skipping permission prompts.

Exit 4 with `other_organization` or `no_session` is not fixable by asking: the session is in another organization, or you are not in a verified session.

## What to write

- {reason} 300 characters or fewer, and never a secret.
- {key} One key per logical request; a new request gets a new key.
- `session new --message`: the first line names the task in a few words; the next lines say the rest. The first line is how the user finds the session later.
- A reply, a command or an answer shows on the card as you wrote it: never put a token or a password in one."#;

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
#[path = "skill_sessions_tests.rs"]
mod tests;
