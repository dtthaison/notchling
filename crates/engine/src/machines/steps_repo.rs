//! Steps 9-11 for one repo of the sd worker job: deploy key, clone, push refused (Swift
//! `RepoSteps`).
//!
//! The machine gets a read-only deploy key: an ed25519 pair made on the machine, so the private key
//! never leaves it. Only the public line comes back, and only a line that is exactly one ed25519
//! key is accepted. GitHub's host key is compared with `gh api meta` before the machine trusts it,
//! and the key is added with the user's own `gh`, never with the flag that makes it writable.
//! Nothing here pushes: the last step only asks git for a dry run. Replacing the key is
//! [`super::steps_replace`]; the push check is [`super::steps_push`].

use super::registry::{Machine, MachineRepo};
use super::repo_common::*;
use super::runner::Call;
use super::shell;
use super::step::{
    run, Cadence, CheckResult, CheckState, FixContext, FixOutcome, FixPlan, Probe, RemoteOutput,
    SetupStep, StepContext, StepFix, StepId, SubRow,
};
use crate::lanes::quote;
use std::collections::BTreeSet;
use std::path::Path;

/// The deploy key, clone and push-refused steps of one repo. A repo that isn't `owner/name` gets
/// three steps with no script and no Fix, only the way to repair it.
pub fn all(m: &Machine, repo: &MachineRepo) -> Vec<SetupStep> {
    let github = repo.github.as_str();
    if !is_valid_repo(github) {
        let bad = CheckResult::fail(format!("{github} is not owner/name"));
        let text = "Change the repo to owner/name in the machine's settings.";
        let how = |id: StepId, title: String, needs: Vec<StepId>, cadence: Cadence| {
            let bad = bad.clone();
            SetupStep::new(
                id,
                title,
                needs,
                cadence,
                Probe::local(move |_| bad.clone()),
                StepFix::HowTo(text.into()),
            )
        };
        return vec![
            how(
                StepId::deploy_key(github),
                format!("Deploy key ({github})"),
                vec![StepId::REACHABLE, StepId::BASE_TOOLS],
                Cadence::Slow,
            ),
            how(
                StepId::clone_repo(github),
                format!("Clone ({github})"),
                vec![StepId::deploy_key(github)],
                Cadence::EveryRefresh,
            ),
            how(
                StepId::push_refused(github),
                format!("Push is refused ({github})"),
                vec![StepId::clone_repo(github)],
                Cadence::Slow,
            ),
        ];
    }
    let mut key = SetupStep::new(
        StepId::deploy_key(github),
        format!("Deploy key ({github})"),
        vec![StepId::REACHABLE, StepId::BASE_TOOLS],
        Cadence::Slow,
        deploy_key_probe(github),
        StepFix::Run(deploy_key_fix(m, github)),
    );
    key.replace = Some(super::steps_replace::replace_key_plan(m, github));
    vec![
        key,
        SetupStep::new(
            StepId::clone_repo(github),
            format!("Clone ({github})"),
            vec![StepId::deploy_key(github)],
            Cadence::EveryRefresh,
            clone_probe(repo),
            StepFix::Run(clone_fix(repo)),
        ),
        SetupStep::new(
            StepId::push_refused(github),
            format!("Push is refused ({github})"),
            vec![StepId::clone_repo(github)],
            Cadence::Slow,
            super::steps_push::push_probe(repo, m.is_linux()),
            StepFix::HowTo(super::steps_push::HOW_TO.into()),
        ),
    ]
}

/// Whether the card offers Replace key for this Check: the key is a pass, or it is on the machine
/// and GitHub has no record of it. A missing key, a writable one, a missing alias and "can't tell"
/// are not offered.
pub fn offers_replace(result: Option<&CheckResult>) -> bool {
    match result {
        Some(r) => match &r.state {
            CheckState::Pass(_) => true,
            CheckState::Fail(_) => r.rows.contains(&not_listed_row()),
            CheckState::CantTell(_) => false,
        },
        None => false,
    }
}

fn mask_with(ctx: &StepContext) -> impl Fn(&str) -> String + '_ {
    move |s| ctx.mask(s)
}

// MARK: Deploy key

/// The remote part prints the key, then `@@config` with the alias check, then `@@ssh` with what
/// GitHub said. The markers are read from the end, so a key file that holds a marker line cannot
/// fake a later part.
fn deploy_key_probe(github: &str) -> Probe {
    let alias = alias(github);
    let pubfile = shell::path(&format!("{}.pub", key_file(github)));
    let script = format!(
        "f={pubfile}; if [ -f \"$f\" ]; then head -c 1024 \"$f\"; fi; echo; \
         echo @@config; if grep -Eq {} \"$HOME/.ssh/config\" 2>/dev/null; then echo alias=yes; else echo alias=no; fi; \
         echo @@ssh; ssh -T -o BatchMode=yes -o ConnectTimeout=8 {alias} </dev/null 2>&1 || true",
        quote(&alias_pattern(&alias))
    );
    let github = github.to_string();
    Probe::remote(script, move |out, ctx| parse_deploy_key(out, &github, ctx))
}

fn parse_deploy_key(out: &RemoteOutput, github: &str, ctx: &StepContext) -> CheckResult {
    let machine = &ctx.machine.name;
    let alias = alias(github);
    let lines: Vec<&str> = out.text.split('\n').collect();
    let Some(ssh_at) = lines.iter().rposition(|l| *l == "@@ssh") else {
        return CheckResult::cant_tell(format!("no answer from {machine}"));
    };
    let Some(config_at) = lines[..ssh_at].iter().rposition(|l| *l == "@@config") else {
        return CheckResult::cant_tell(format!("no answer from {machine}"));
    };
    let pub_text = lines[..config_at].join("\n");
    let alias_ok = lines[config_at + 1..ssh_at].contains(&"alias=yes");
    let ssh_text = lines[ssh_at + 1..].join("\n");
    let mask = mask_with(ctx);

    if pub_text.trim().is_empty() {
        return CheckResult::fail(format!("no deploy key on {machine} yet"));
    }
    let Some(pubkey) = single_ed25519_line(&pub_text) else {
        return CheckResult::fail(format!("unexpected public key on {machine}"));
    };

    let answer = ctx.runner.run(&Call::secs(list_argv(github), 30));
    if answer.code != 0 {
        let why = first_line(
            if answer.err.is_empty() {
                &answer.out
            } else {
                &answer.err
            },
            &mask,
            200,
        );
        let tail = if why.is_empty() {
            String::new()
        } else {
            format!(": {why}")
        };
        return CheckResult::cant_tell(format!(
            "gh could not list the deploy keys of {github}{tail}"
        ));
    }
    let Some(keys) = parse_keys(&answer.out) else {
        return CheckResult::cant_tell("can't read GitHub's list of deploy keys");
    };
    let Some(entry) = listed(&pubkey, &keys) else {
        return CheckResult::fail(format!("the key is not on {github} yet"))
            .rows(vec![not_listed_row()]);
    };
    if !read_only(entry) {
        return CheckResult::fail(can_write(github)).rows(vec![SubRow::new(
            "Key on GitHub",
            false,
            "can write",
        )]);
    }
    let mut rows = vec![SubRow::new("Key on GitHub", true, "read-only")];

    if !alias_ok {
        rows.push(SubRow::new("ssh alias", false, "missing"));
        return CheckResult::fail(format!("no ssh alias {alias} on {machine}")).rows(rows);
    }
    rows.push(SubRow::new("ssh alias", true, &alias));

    if ssh_text.contains("successfully authenticated") {
        rows.push(SubRow::new("ssh to GitHub", true, "authenticated"));
        return CheckResult::pass(format!("read-only key on {github}")).rows(rows);
    }
    let said = first_line(&ssh_text, &mask, 200);
    rows.push(SubRow::new("ssh to GitHub", false, &said));
    if ssh_text.contains("Permission denied") || ssh_text.contains("Host key verification failed") {
        return CheckResult::fail(format!("GitHub refused ssh through {alias}: {said}")).rows(rows);
    }
    let tail = if said.is_empty() {
        String::new()
    } else {
        format!(": {said}")
    };
    CheckResult::cant_tell(format!("can't tell if ssh reaches GitHub{tail}")).rows(rows)
}

/// Writes the public key line where `gh` can read it: a private folder under `scratch`, a file that
/// is removed again by the caller. `None` when it can't be written.
pub(super) fn write_pub_file(scratch: &Path, file_name: &str, line: &str) -> Option<ScratchFile> {
    notchling_platform::files::ensure_private_dir(scratch).ok()?;
    let path = scratch.join(file_name);
    notchling_platform::files::write_private(&path, format!("{line}\n").as_bytes()).ok()?;
    Some(ScratchFile(path))
}

/// A file under the scratch folder, removed when it goes out of scope.
pub(super) struct ScratchFile(std::path::PathBuf);

impl ScratchFile {
    pub(super) fn path(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for ScratchFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn known_hosts_script(host_key: &str) -> String {
    format!(
        "set -e; mkdir -p \"$HOME/.ssh\"; chmod 700 \"$HOME/.ssh\"; kh=\"$HOME/.ssh/known_hosts\"; touch \"$kh\"; chmod 600 \"$kh\"; \
         line={}; \
         grep -qxF \"$line\" \"$kh\" || printf '\\n%s\\n' \"$line\" >> \"$kh\"",
        quote(&format!("github.com {host_key}"))
    )
}

fn deploy_key_fix(m: &Machine, github: &str) -> FixPlan {
    let name = m.name.clone();
    let key = shell::path(&key_file(github));
    let key_pub = shell::path(&format!("{}.pub", key_file(github)));
    let alias = alias(github);
    let config_block = format!(
        "\nHost {alias}\n  HostName github.com\n  User git\n  IdentityFile ~/.ssh/agents-{}\n  IdentitiesOnly yes\n",
        slug(github)
    );
    let keygen = format!(
        "set -e; mkdir -p \"$HOME/.ssh\"; chmod 700 \"$HOME/.ssh\"\n\
         [ -f {key} ] || ssh-keygen -q -t ed25519 -N '' -C {} -f {key}\n\
         touch \"$HOME/.ssh/config\"; chmod 600 \"$HOME/.ssh/config\"\n\
         grep -Eq {} \"$HOME/.ssh/config\" || printf {} >> \"$HOME/.ssh/config\"\n\
         cat {key_pub}",
        quote(&format!("agents-{name}")),
        quote(&alias_pattern(&alias)),
        quote(&config_block)
    );
    let keyscan = "ssh-keyscan -t ed25519 github.com 2>/dev/null";
    let meta_argv: Vec<String> = [
        gh(),
        "api".into(),
        "meta".into(),
        "--jq".into(),
        ".ssh_keys[]".into(),
    ]
    .to_vec();
    let list = list_argv(github).join(" ");
    let display = format!(
        "On {name}:\n{keygen}\n\nOn this Mac:\n{}\n\n\
         On {name}:\n{keyscan}\n(its key must be one of the keys from gh api meta)\n\n\
         On {name}: add GitHub's key from gh api meta to ~/.ssh/known_hosts unless it is already there\n\n\
         On this Mac:\n{list}\n(the key is not added again if GitHub already lists it)\n\n\
         On this Mac:\n{}\n(read-only: the key is added without write access)",
        meta_argv.join(" "),
        add_argv("<the public key file>", github, &format!("agents-{name}")).join(" ")
    );
    let github = github.to_string();
    FixPlan::new(display, false, move |ctx| {
        deploy_key_run(ctx, &name, &github, &keygen, keyscan, &meta_argv)
    })
}

fn deploy_key_run(
    ctx: &FixContext,
    name: &str,
    github: &str,
    keygen: &str,
    keyscan: &str,
    meta_argv: &[String],
) -> FixOutcome {
    if !is_valid_machine_name(name) {
        return FixOutcome::fail(format!("the machine name {name} is not a plain name"));
    }
    let mask = |s: &str| ctx.mask(s);
    let said = |r: &super::runner::CommandResult| {
        first_line(if r.err.is_empty() { &r.out } else { &r.err }, &mask, 200)
    };

    // 1. the key and the ssh alias, made on the machine; only the public line comes back
    let made = run::script(ctx, keygen, None, 600);
    if made.code != 0 {
        return FixOutcome::fail(format!("could not make the key on {name}: {}", said(&made)));
    }
    let Some(pubkey) = single_ed25519_line(&made.out) else {
        return FixOutcome::fail(format!(
            "unexpected public key from {name}; nothing was added"
        ));
    };

    // 2. GitHub's host keys, from this Mac
    let meta = run::local(ctx, meta_argv.to_vec(), None, 120);
    let trusted: BTreeSet<String> = meta
        .out
        .split(['\n', '\r'])
        .map(|l| l.trim_matches([' ', '\t']))
        .filter(|l| is_key_body_line(l))
        .map(str::to_string)
        .collect();
    if meta.code != 0 || trusted.is_empty() {
        return FixOutcome::fail(format!(
            "gh api meta gave no GitHub host key: {}; nothing was added",
            first_line(&meta.err, &mask, 200)
        ));
    }

    // 3. what the machine sees at github.com must be one of them
    let scanned = run::script(ctx, keyscan, None, 600);
    let seen: Vec<String> = scanned
        .out
        .split(['\n', '\r'])
        .filter_map(|line| {
            let words: Vec<&str> = line.split(' ').filter(|w| !w.is_empty()).collect();
            (words.len() >= 3 && words[1] == "ssh-ed25519")
                .then(|| format!("ssh-ed25519 {}", words[2]))
        })
        .collect();
    if seen.is_empty() {
        return FixOutcome::fail(format!(
            "could not read GitHub's host key from {name}; nothing was added"
        ));
    }
    let Some(host_key) = seen.iter().find(|k| trusted.contains(*k)) else {
        return FixOutcome::fail(format!(
            "GitHub's host key seen from {name} differs from gh api meta; nothing was added"
        ));
    };

    // 4. trust this Mac's copy of it, never the machine's
    let trust = run::script(ctx, &known_hosts_script(host_key), None, 600);
    if trust.code != 0 {
        return FixOutcome::fail(format!(
            "could not write known_hosts on {name}: {}",
            first_line(&trust.err, &mask, 200)
        ));
    }

    // 5. a key GitHub already lists is not added again: read-only is a repair that worked,
    // writable stops
    let current = run::local(ctx, list_argv(github), None, 120);
    let keys = if current.code == 0 {
        parse_keys(&current.out)
    } else {
        None
    };
    let Some(keys) = keys else {
        return FixOutcome::fail(format!(
            "gh could not list the deploy keys of {github}: {}",
            said(&current)
        ));
    };
    if let Some(entry) = listed(&pubkey, &keys) {
        if read_only(entry) {
            return FixOutcome::ok(format!(
                "the read-only deploy key is already on {github}; the machine is set up"
            ));
        }
        return FixOutcome::fail(can_write(github));
    }

    // 6. the public key goes to GitHub through the user's gh, read-only
    let file_name = format!("{name}-{}.pub", slug(github));
    let Some(file) = write_pub_file(&ctx.step.scratch, &file_name, &pubkey) else {
        return FixOutcome::fail("could not write the public key file on this Mac");
    };
    let title = format!("agents-{name}");
    let added = run::local(ctx, add_argv(&file.path(), github, &title), None, 120);
    if added.code != 0 {
        return FixOutcome::fail(format!(
            "gh could not add the deploy key to {github}: {}",
            said(&added)
        ));
    }
    FixOutcome::ok(format!("added a read-only deploy key to {github}"))
}

// MARK: Clone

fn clone_probe(repo: &MachineRepo) -> Probe {
    let path = shell::path(&checkout_path(repo));
    let wanted = remote_url(&repo.github);
    let checkout = checkout_path(repo);
    Probe::remote(
        format!("git -C {path} remote get-url origin 2>/dev/null || echo none"),
        move |out, ctx| {
            let origin = first_line(&out.text, &|s| ctx.mask(s), 200);
            if origin.is_empty() && out.code != 0 {
                return CheckResult::cant_tell(format!("no answer from {}", ctx.machine.name));
            }
            if origin == wanted {
                return CheckResult::pass(checkout.clone());
            }
            if origin.is_empty() || origin == "none" {
                return CheckResult::fail("not cloned");
            }
            CheckResult::fail(format!("{checkout} has origin {origin}, not {wanted}"))
        },
    )
}

fn clone_fix(repo: &MachineRepo) -> FixPlan {
    let path = shell::path(&checkout_path(repo));
    let url = remote_url(&repo.github);
    let script = format!(
        "set -e; p={path}; if [ -e \"$p\" ]; then echo \"$p already exists; Notchling won't touch it\" >&2; exit 1; fi; \
         mkdir -p \"$(dirname \"$p\")\"; GIT_SSH_COMMAND='ssh -o BatchMode=yes' git clone -- {url} \"$p\""
    );
    let github = repo.github.clone();
    let checkout = checkout_path(repo);
    let display = script.clone();
    FixPlan::new(display, false, move |ctx| {
        let result = run::script(ctx, &script, None, 600);
        if result.code != 0 {
            return FixOutcome::fail(first_line(
                if result.err.is_empty() {
                    &result.out
                } else {
                    &result.err
                },
                &|s| ctx.mask(s),
                200,
            ));
        }
        FixOutcome::ok(format!("cloned {github} to {checkout}"))
    })
}
