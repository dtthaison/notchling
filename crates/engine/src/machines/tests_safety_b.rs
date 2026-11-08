//! End-to-end safety properties of the repo and sd worker steps, over a Mac and a Linux machine
//! (Swift `MachinesSafetyTests`, the parts that concern these steps). Nothing starts ssh or `gh`:
//! every Fix runs against a scripted runner that records and answers success.

use super::fakes_b::*;
use super::registry::{AdminAccess, Machine, MachineRepo};
use super::runner::CommandResult;
use super::shell;
use super::step::{FixPlan, SetupStep, StepFix, StepId};
use super::steps_repo;
use super::steps_sd;
use super::tests_repo_replace::{replace_rig, Opts};

fn catalog(m: &Machine) -> Vec<SetupStep> {
    let repo = m.repos()[0].clone();
    let mut all = steps_repo::all(m, &repo);
    all.extend(steps_sd::all(
        m,
        m.sd_worker().unwrap(),
        std::slice::from_ref(m),
    ));
    all
}

/// A Linux machine runs its admin steps as root over ssh (sudo as the worker is refused).
fn admin_runnable(m: &Machine) -> Machine {
    let mut m = m.clone();
    if m.os == Some(super::registry::MachineOs::Linux) {
        m.set_admin(AdminAccess::RootSsh);
    }
    m
}

fn machines() -> Vec<Machine> {
    vec![mac("m1"), linux("vps-sing-1")]
}

fn is_linux(m: &Machine) -> bool {
    m.os == Some(super::registry::MachineOs::Linux)
}

/// The base64 blobs a script carries (the files an admin step installs), decoded and appended.
fn with_decoded_blobs(text: &str) -> String {
    let mut out = text.to_string();
    let mut run = String::new();
    let mut blobs = vec![];
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' {
            run.push(c);
        } else {
            if run.len() >= 40 {
                blobs.push(std::mem::take(&mut run));
            }
            run.clear();
        }
    }
    for b in blobs {
        if let Some(bytes) = decode_base64(&b) {
            if let Ok(s) = String::from_utf8(bytes) {
                out.push('\n');
                out.push_str(&s);
            }
        }
    }
    out
}

fn decode_base64(s: &str) -> Option<Vec<u8>> {
    let mut out = vec![];
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.trim_end_matches('=').chars() {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' => 62,
            '/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// A rig with the sd templates and a sd 1.19.3 on this Mac, so the service and check Fixes get as
/// far as their commands.
fn full_rig(m: &Machine) -> Rig {
    let rig = Rig::new(admin_runnable(m));
    let sd = sd_folder(&rig.tmp, "sd");
    rig.with_mac(mac_facts("1.19.3", &sd))
}

fn run_plan(m: &Machine, plan: &FixPlan) -> Vec<String> {
    let rig = full_rig(m);
    (plan.run)(&rig.fix());
    rig.argvs().into_iter().map(|a| a.join(" ")).collect()
}

/// Every text a step can put in front of a machine or the user: displays, probe scripts, toggle
/// scripts, and the argv of every Fix run against a recording runner.
fn all_texts(m: &Machine, admin_only: bool) -> Vec<(String, String)> {
    let mut texts = vec![];
    for step in catalog(m) {
        let label = format!("{}/{}", m.name, step.id.raw());
        if let Some(script) = step.probe.script() {
            if !admin_only {
                texts.push((format!("{label} probe"), script.to_string()));
            }
        }
        match &step.fix {
            StepFix::HowTo(_) => {}
            StepFix::Toggle { needs_admin } => {
                if admin_only && !needs_admin {
                    continue;
                }
                for on in [true, false] {
                    texts.push((
                        format!("{label} toggle {on}"),
                        steps_sd::toggle_script(is_linux(m), on),
                    ));
                }
            }
            StepFix::Run(plan) => {
                if admin_only && !plan.needs_admin {
                    continue;
                }
                texts.push((format!("{label} display"), plan.display.clone()));
                for call in run_plan(m, plan) {
                    texts.push((format!("{label} run"), call));
                }
            }
        }
        // Replace key is no Fix of Set up, but its commands are held to the same scans
        if let Some(replace) = &step.replace {
            if !admin_only {
                texts.push((format!("{label} replace display"), replace.display.clone()));
                let mut rig = replace_rig(Opts::default());
                rig.machine = m.clone();
                (replace.run)(&rig.fix());
                for a in rig.argvs() {
                    texts.push((format!("{label} replace run"), a.join(" ")));
                }
            }
        }
    }
    texts
}

fn find_all(text: &str, start: &str, stop: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = text;
    while let Some(i) = rest.find(start) {
        let tail = &rest[i..];
        let end = tail
            .char_indices()
            .skip(start.len())
            .find(|(_, c)| stop.contains(*c))
            .map_or(tail.len(), |(j, _)| j);
        out.push(tail[..end].to_string());
        rest = &tail[end..];
    }
    out
}

#[test]
fn admin_scripts_never_touch_production_services() {
    let allowed = [
        "systemctl daemon-reload",
        "systemctl enable --now sd-worker.service",
        "systemctl disable --now sd-worker.service",
    ];
    let (mut admin, mut systemctl) = (0, 0);
    for m in machines() {
        for (label, raw) in all_texts(&m, true) {
            admin += 1;
            let text = with_decoded_blobs(&raw);
            // the unit file's own `Restart=on-failure` is a directive, not a restart of a service,
            // and its comments name tools the worker needs; neither is a command
            let lower = text
                .to_lowercase()
                .lines()
                .filter(|l| !l.trim_start().starts_with('#'))
                .collect::<Vec<_>>()
                .join("\n")
                .replace("restart=", "");
            for name in shell::PRODUCTION_SERVICES {
                assert!(!lower.contains(name), "{label} names {name}");
            }
            for word in ["restart", "reload-or-restart", "docker"] {
                // the verb, not words that contain it (`pmset autorestart`)
                let verb = lower
                    .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
                    .any(|w| w == word || w == format!("try-{word}"));
                assert!(!verb, "{label} uses {word}");
            }
            assert!(
                !lower.replace("daemon-reload", "").contains(" reload"),
                "{label} reloads a service"
            );
            for found in find_all(&text, "systemctl", "\n;'&|\"") {
                systemctl += 1;
                assert!(allowed.contains(&found.trim()), "{label} runs `{found}`");
            }
            for killer in ["kill ", "pkill", "killall", "xkill"] {
                assert!(!lower.contains(killer), "{label} kills a process");
            }
            for found in find_all(&text, "apt-get", "\n;'&|\"") {
                if found.contains(" install") {
                    assert!(found.contains("--no-upgrade"), "{label}: {found}");
                }
            }
        }
    }
    assert!(admin > 2, "the admin steps were looked at");
    assert!(systemctl >= 2, "the systemctl commands were looked at");
}

/// Every admin command (the service Fix and the Running switch) goes out through the absolute
/// `/usr/bin/sudo` (or as root over ssh, with no sudo), runs its script under `env -i` with a fixed
/// PATH, and carries nothing of the worker account's own.
#[test]
fn every_admin_command_uses_the_absolute_sudo_and_a_fixed_environment() {
    let mut seen = 0;
    for machine in machines() {
        let m = admin_runnable(&machine);
        let mut commands: Vec<(String, Vec<String>)> = vec![];
        for step in catalog(&m) {
            match &step.fix {
                StepFix::Run(plan) if plan.needs_admin => {
                    let rig = full_rig(&m);
                    (plan.run)(&rig.fix());
                    for a in rig.argvs() {
                        commands.push((format!("{}/{}", m.name, step.id.raw()), a));
                    }
                }
                StepFix::Toggle { needs_admin: true } => {
                    for on in [true, false] {
                        let rig = full_rig(&m);
                        super::step::run::admin(
                            &rig.fix(),
                            &steps_sd::toggle_script(is_linux(&m), on),
                            30,
                        );
                        for a in rig.argvs() {
                            commands.push((format!("{}/{} {on}", m.name, step.id.raw()), a));
                        }
                    }
                }
                _ => {}
            }
        }
        for (label, argv) in commands {
            seen += 1;
            let text = argv.join("\n");
            for needle in [
                shell::prelude().as_str(),
                "export PATH",
                "$HOME",
                ".local/bin",
                ".cargo",
                "mise",
                "/opt/homebrew",
                "/usr/local",
            ] {
                assert!(!text.contains(needle), "{label} carries {needle}");
            }
            assert!(
                text.contains("/usr/bin/env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin HOME="),
                "{label} has no fixed environment"
            );
            if m.admin() == AdminAccess::RootSsh {
                assert!(argv.contains(&format!("root@{}", m.host)), "{label}");
                assert!(!text.contains("sudo"), "{label} uses sudo as root");
            } else {
                assert!(
                    text.contains("/usr/bin/sudo -S -k"),
                    "{label} doesn't call the absolute sudo"
                );
                assert_eq!(
                    text.matches("sudo").count(),
                    text.matches("/usr/bin/sudo").count(),
                    "{label} has a sudo found by PATH"
                );
            }
        }
    }
    assert!(seen >= 3, "the admin commands were looked at: {seen}");
}

#[test]
fn the_admin_steps_of_these_catalogs_are_the_ones_the_spec_names() {
    let mut names = vec![];
    for m in machines() {
        for step in catalog(&m) {
            let admin = match &step.fix {
                StepFix::Run(p) => p.needs_admin,
                StepFix::Toggle { needs_admin } => *needs_admin,
                StepFix::HowTo(_) => false,
            };
            if admin {
                names.push(format!("{}/{}", m.name, step.id.raw()));
            }
        }
    }
    assert_eq!(names, ["vps-sing-1/service", "vps-sing-1/running"]);
}

/// Nothing pushes except the dry run of Replace key, and only the read-only deploy-key add writes
/// to GitHub; the one delete is by an integer id.
#[test]
fn nothing_pushes_except_the_read_only_deploy_key_add() {
    let writers = [
        "gh issue",
        "gh pr ",
        "gh release",
        "gh secret",
        "gh gist",
        "gh repo create",
        "gh repo delete",
        "gh repo edit",
        "gh repo fork",
        "deploy-key delete",
        "--method POST",
        "--method PUT",
        "--method PATCH",
        "--method DELETE",
        "-X POST",
        "-X PUT",
        "-X PATCH",
        "-X DELETE",
        "--allow-write",
    ];
    let stop = "\n;'\"&|";
    let (mut adds, mut deletes) = (0, 0);
    for m in machines() {
        for (label, raw) in all_texts(&m, false) {
            let text = with_decoded_blobs(&raw);
            for found in find_all(&text, "git push", stop) {
                assert!(found.contains("--dry-run"), "{label} pushes: {found}");
            }
            let replacing = label.contains("replace");
            for w in writers {
                if replacing && w == "deploy-key delete" {
                    continue;
                }
                assert!(!text.contains(w), "{label} uses {w}");
            }
            for found in find_all(&text, "deploy-key delete", stop) {
                assert!(replacing, "{label} deletes a deploy key: {found}");
                if label.ends_with("replace run") {
                    // `<gh> repo deploy-key delete <integer id> -R <repo>`
                    let words: Vec<&str> = found.split_whitespace().collect();
                    assert!(
                        words[2].chars().all(|c| c.is_ascii_digit()) && !words[2].is_empty(),
                        "{label}: {found}"
                    );
                    deletes += 1;
                } else {
                    assert!(
                        found.contains("<id of the old key>")
                            || found.contains("<id of the new key>"),
                        "{label}: {found}"
                    );
                }
            }
            assert!(!text.contains("allow-write"), "{label} has the write flag");
            for found in find_all(&text, "deploy-key add", stop)
                .into_iter()
                .chain(find_all(&text, "deploy-key delete", stop))
            {
                assert!(!format!("{found} ").contains(" -w "), "{label} has -w");
            }
            // `gh api` only reads (the host-key `meta` call): a field or an input body turns it into a POST
            for found in find_all(&text, "gh api", stop) {
                let found = format!(" {found} ");
                for flag in [" -f ", " -F ", " --field", " --raw-field", " --input"] {
                    assert!(
                        !found.contains(flag),
                        "{label} sends a body with gh api: {found}"
                    );
                }
            }
            for found in find_all(&text, "gh repo deploy-key add", stop) {
                adds += 1;
                assert!(found.contains("--title agents-"), "{label}: {found}");
            }
        }
    }
    assert!(adds > 0, "the deploy-key add was looked at");
    assert!(
        deletes > 0,
        "Replace key's delete of the old key was looked at"
    );
}

/// Replace key is the Deploy key step's `replace` and nothing else: no Fix, toggle or probe of any
/// step in the catalog deletes a deploy key or makes a `.new` key.
#[test]
fn no_step_but_replace_key_deletes_a_key_or_makes_a_new_one() {
    for m in machines() {
        for step in catalog(&m) {
            if let Some(script) = step.probe.script() {
                assert!(
                    !script.contains("deploy-key delete") && !script.contains(".new"),
                    "{}",
                    step.id.raw()
                );
            }
            if let StepFix::Run(plan) = &step.fix {
                for call in run_plan(&m, plan) {
                    assert!(
                        !call.contains("deploy-key delete"),
                        "{}: {call}",
                        step.id.raw()
                    );
                    assert!(
                        !call.contains("agents-dtthaison-lingeva-mono.new"),
                        "{}",
                        step.id.raw()
                    );
                }
                assert!(
                    !plan.display.contains("deploy-key delete"),
                    "{}",
                    step.id.raw()
                );
            }
        }
        let deploy = find(&catalog(&m), &StepId::deploy_key(REPO));
        assert!(deploy.replace.is_some());
        assert!(
            matches!(deploy.fix, StepFix::Run(_)),
            "the Fix stays the add"
        );
    }
}

/// The text with every single-quoted part taken out (`'\''` is a quote inside one), so what is
/// left is what a shell would expand.
fn outside_quotes(s: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in s.replace("'\\''", "Q").chars() {
        if c == '\'' {
            inside = !inside;
        } else if !inside {
            out.push(c);
        }
    }
    out
}

#[test]
fn hostile_remote_values_never_reach_a_script() {
    // a home folder with characters Notchling won't use, a checkout path that quotes and expands
    let mut m = linux("vps-sing-1");
    m.abilities.as_mut().unwrap().home = Some("/home/a b;x".into());
    {
        let job = m.sd_worker_mut().unwrap();
        job.sd_path = String::new();
        job.repos = vec![MachineRepo {
            machine_path: Some("/srv/it's$(id)".into()),
            ..job.repos[0].clone()
        }];
    }
    for step in catalog(&m) {
        let label = step.id.raw();
        if let Some(s) = step.probe.script() {
            for bad in [";x", "a b"] {
                assert!(!s.contains(bad), "{label} probe carries {bad}: {s}");
            }
            // a checkout path may be there, but only inside single quotes
            assert!(
                !outside_quotes(s).contains("$(id)"),
                "{label} probe expands a path: {s}"
            );
        }
        if let StepFix::Run(plan) = &step.fix {
            for bad in [";x", "a b"] {
                assert!(!plan.display.contains(bad), "{label} display carries {bad}");
            }
            assert!(
                !outside_quotes(&plan.display).contains("$(id)"),
                "{label} display"
            );
            for call in run_plan(&m, plan) {
                for bad in [";x", "a b;"] {
                    assert!(
                        !call.contains(bad),
                        "{label} runs a hostile value ({bad}): {call}"
                    );
                }
                assert!(
                    !outside_quotes(&call).contains("$(id)"),
                    "{label} expands a path: {call}"
                );
            }
        }
    }
}

/// A non-integer key id from GitHub never becomes part of a command.
#[test]
fn a_hostile_key_id_from_github_never_becomes_part_of_a_command() {
    let m = mac("m1");
    let rig = replace_rig(Opts {
        old_id: Some("\"11; touch /tmp/x\""),
        ..Opts::default()
    });
    let step = find(&catalog(&m), &StepId::deploy_key(REPO));
    (step.replace.unwrap().run)(&rig.fix());
    for a in rig.argvs() {
        let text = a.join(" ");
        assert!(!text.contains("touch"), "{text}");
    }
}

/// Machine output with a vault value in it never reaches an outcome or a display unmasked, whichever
/// Fix it came back to.
#[test]
fn a_vault_value_in_machine_output_is_masked_in_every_fix_outcome() {
    let token = long_token();
    for m in machines() {
        for step in catalog(&m) {
            let StepFix::Run(plan) = &step.fix else {
                continue;
            };
            let rig = full_rig(&m).hiding("claude-worker", &token);
            rig.runner.on(
                |_| true,
                CommandResult::new(1, &format!("out {token}\n"), &format!("failed: {token}\n")),
            );
            let out = (plan.run)(&rig.fix());
            assert!(
                out.message.is_empty() || !out.message.contains(&token[10..30]),
                "{}: {}",
                step.id.raw(),
                out.message
            );
            for a in rig.argvs() {
                assert!(!a.join(" ").contains(&token[10..30]), "{}", step.id.raw());
            }
            assert!(!plan.display.contains(&token[10..30]));
        }
    }
}
