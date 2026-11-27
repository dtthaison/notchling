use super::registry::MachineRepo;
use super::repo_common::{checkout_path, first_line};
use super::shell;
use super::step::{CheckResult, Probe, SubRow};
use std::collections::BTreeMap;

pub const HOW_TO: &str = "A push GitHub accepted means the key can write. Remove it on GitHub (Settings > Deploy keys) and run Deploy key's Set up again; it adds the key read-only.\nA login of the worker account itself (gh, a default ssh key, the keychain, ~/.git-credentials) is a second way to push: log it out or remove it on the machine, then Check again.";

fn login_checks(linux: bool) -> String {
    let mut lines = vec![
        "if command -v gh >/dev/null 2>&1; then gh auth status --hostname github.com >/dev/null 2>&1; echo \"gh=$?\"; else echo gh=none; fi".to_string(),
        "r=$(ssh -o BatchMode=yes -o ConnectTimeout=8 -o StrictHostKeyChecking=yes -T git@github.com </dev/null 2>&1); \
         n=$(printf '%s\\n' \"$r\" | sed -n 's|^Hi \\([A-Za-z0-9._/-]*\\)!.*|\\1|p' | head -n 1); \
         if [ -n \"$n\" ]; then echo \"ssh=hi:$n\"; else case \"$r\" in *denied*) echo ssh=denied;; *) echo ssh=unknown;; esac; fi".to_string(),
    ];
    if !linux {
        lines.push(
            "security find-internet-password -s github.com >/dev/null 2>&1; echo \"keychain=$?\""
                .to_string(),
        );
    }
    lines.push("grep -q 'github.com' ~/.git-credentials 2>/dev/null; echo \"credfile=$?\"".into());
    lines.join("\n")
}

pub fn push_probe(repo: &MachineRepo, linux: bool) -> Probe {
    let path = shell::path(&checkout_path(repo));
    let script = format!(
        "GIT_TERMINAL_PROMPT=0 GIT_SSH_COMMAND='ssh -o BatchMode=yes -o ConnectTimeout=8' \
         git -c core.hooksPath=/dev/null -C {path} push --dry-run origin HEAD:refs/heads/agents-push-test </dev/null 2>&1\n\
         echo \"@@push $?\"\n{}",
        login_checks(linux)
    );
    let github = repo.github.clone();
    Probe::remote(script, move |out, ctx| {
        let lines: Vec<&str> = out.text.split('\n').collect();
        let mask = |s: &str| ctx.mask(s);
        let marker = lines.iter().position(|l| l.starts_with("@@push "));
        let code = marker.and_then(|i| lines[i][7..].parse::<i64>().ok());
        let (Some(marker), Some(code)) = (marker, code) else {
            let line = first_line(&out.text, &mask, 200);
            return CheckResult::fail(if line.is_empty() {
                format!("the push check failed (code {})", out.code)
            } else {
                line
            });
        };
        let git = lines[..marker].join("\n");
        if code == 0 {
            return CheckResult::fail(format!(
                "this machine can push to {github}; the deploy key can write"
            ));
        }
        if !git.contains("marked as read only") {
            let line = first_line(&git, &mask, 200);
            return CheckResult::fail(if line.is_empty() {
                format!("the push check failed (code {code})")
            } else {
                line
            });
        }
        login_result(&lines[marker + 1..])
    })
}

fn is_plain_login(name: &str) -> bool {
    (1..=39).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn login_result(tokens: &[&str]) -> CheckResult {
    let mut values: BTreeMap<&str, &str> = BTreeMap::new();
    for line in tokens {
        if let Some((k, v)) = line.split_once('=') {
            values.insert(k, v);
        }
    }
    let mut rows = vec![SubRow::new(
        "Push with the deploy key",
        true,
        "GitHub refuses it",
    )];
    let mut reasons: Vec<String> = Vec::new();
    let mut doubts: Vec<String> = Vec::new();

    match values.get("gh").copied() {
        Some("0") => {
            reasons.push("gh is logged in as the worker; run gh auth logout".into());
            rows.push(SubRow::new("gh login", false, "logged in"));
        }
        Some("none") | Some("1") => rows.push(SubRow::new("gh login", true, "not logged in")),
        _ => {
            doubts.push("couldn't check gh".into());
            rows.push(SubRow::new("gh login", false, "couldn't check"));
        }
    }

    let ssh = values.get("ssh").copied().unwrap_or("");
    if let Some(name) = ssh.strip_prefix("hi:") {
        if name.contains('/') {
            rows.push(SubRow::new(
                "Default ssh key",
                true,
                "a deploy key for one repo",
            ));
        } else {
            let plain = is_plain_login(name);
            reasons.push(format!(
                "a default ssh key logs in to GitHub{}",
                if plain {
                    format!(" as {name}")
                } else {
                    String::new()
                }
            ));
            rows.push(SubRow::new(
                "Default ssh key",
                false,
                &if plain {
                    format!("logs in as {name}")
                } else {
                    "logs in".to_string()
                },
            ));
        }
    } else if ssh == "denied" {
        rows.push(SubRow::new(
            "Default ssh key",
            true,
            "GitHub does not know it",
        ));
    } else {
        doubts.push("couldn't ask GitHub about a default ssh key".into());
        rows.push(SubRow::new(
            "Default ssh key",
            false,
            "no answer from GitHub",
        ));
    }

    if let Some(keychain) = values.get("keychain") {
        match *keychain {
            "0" => {
                reasons.push("the keychain holds a github.com password".into());
                rows.push(SubRow::new(
                    "Keychain",
                    false,
                    "holds a github.com password",
                ));
            }
            "44" => rows.push(SubRow::new("Keychain", true, "no github.com password")),
            _ => {
                doubts.push("couldn't read the keychain".into());
                rows.push(SubRow::new("Keychain", false, "couldn't read it"));
            }
        }
    }

    if values.get("credfile").copied() == Some("0") {
        reasons.push("~/.git-credentials holds a github.com login".into());
        rows.push(SubRow::new(
            "~/.git-credentials",
            false,
            "holds a github.com login",
        ));
    } else {
        rows.push(SubRow::new(
            "~/.git-credentials",
            true,
            "no github.com login",
        ));
    }

    if !reasons.is_empty() {
        return CheckResult::fail(reasons.join("; ")).rows(rows);
    }
    if !doubts.is_empty() {
        return CheckResult::cant_tell(doubts.join("; ")).rows(rows);
    }
    CheckResult::pass("GitHub refuses a push with this key; no other GitHub login on this account")
        .rows(rows)
}
