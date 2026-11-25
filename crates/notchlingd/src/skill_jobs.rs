use crate::skills_family::{minimal, KEY_RULE, REASON_RULE};

pub const NAME: &str = "notchling-jobs";

const DESCRIPTION: &str = "Use to send a job to a machine of your organization and follow it: dispatch it, list jobs, read one, tail its output, bring its result back or stop it.";

const TITLE: &str = "Notchling jobs";

const ABOUT: &str = "A job runs on a machine of your organization. You see your own jobs in full and others by state only.";

const STEPS: &str = r#"
## Send a job

```sh
'{binary}' dispatch <machine> --tool claude --folder <path> --text "<repo, branch, task>" --dry-run --json
```

Then the same line with `--reason "<what and why>" --key <key> --no-wait --json` instead of `--dry-run`.

- A dispatch always asks the user, for every tool, and only the Mac may allow it. The machine must be in your organization, or it is exit 4 `other_organization`.
- `--text` names the repo, the branch and the task: `Repo acme/api, branch fix/login: make the login test pass, then stop.` The job starts with nothing else.
- `--tool shell` takes `--command <line>` instead of `--text`.
- Name a secret with `--secret <name>`, never in `--text` or `--command`: the user sees the names on the card and the machine gets the values.
- The `cluster` switch must be on. Exit 4 with `error.reason` `switch_off` carries `error.ask`: run that command once, never in a loop.
- The same `--key` sends one job and opens one card, so a retry after exit 124 or a lost answer is safe. A different job needs a new key.

## Follow it

```sh
'{binary}' jobs --json
'{binary}' job <id> --wait --timeout 900 --json
'{binary}' job tail <id> --dry-run --json
'{binary}' job return <id> --dry-run --json
'{binary}' job stop <id> --dry-run --json
```

- `jobs` shows the jobs you sent in full and other jobs of your organization by state only.
- `job <id> --wait` exits 0 when the job ends, whatever the state: read `state` in the output. It exits 124 if the job still runs when `--timeout` passes: run it again, or run it in the background.
- `job tail`, `job return` and `job stop` are free for a job you sent. For any other job they open a card and wait: add `--no-wait`, then wait with `'{binary}' consent <id> --wait --timeout 600 --json`, which exits 0 (applied), 5 (the user said no) or 124 (no answer), or follow several with `'{binary}' watch --topic mine.consents --until applied --timeout 600 --json`.
- `--from <offset>` on `job tail` reads on from where the last tail ended.
- Exit 6: the request is saved and waits for the user; its id is in the output. Exit 124: no answer in time, the request is closed.

## What to write

- {reason} 300 characters or fewer, and never a secret.
- {key} One key per logical request; a new request gets a new key.
"#;

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
#[path = "skill_jobs_tests.rs"]
mod tests;
