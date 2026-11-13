use super::registry::{Machine, MachineOs, SdWorkerSettings};
use super::runner::{Channel, OutputSink};
use super::shell;
use super::step::{
    run, Cadence, CheckResult, FixContext, FixOutcome, FixPlan, Probe, SecretSource, SetupStep,
    StepFix, StepId,
};
use super::token_creation;
use super::usage::{SecretFormat, SecretUsage};
use crate::accounts::Secret;
use crate::lanes::quote;
use crate::vault::{Redactor, VaultEntry, VaultRequest, VaultValue};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const MINIMUM_LENGTH: usize = 8;

pub fn claude_usage(m: &Machine) -> SecretUsage {
    SecretUsage {
        name: m.claude_secret.clone(),
        machine: m.name.clone(),
        remote_path: shell::CLAUDE_ENV.into(),
        format: SecretFormat::KeyValue(shell::CLAUDE_KEY.into()),
    }
}

pub fn issues_usage(m: &Machine, job: &SdWorkerSettings) -> SecretUsage {
    SecretUsage {
        name: job.issues_secret.clone(),
        machine: m.name.clone(),
        remote_path: shell::GITHUB_TOKEN.into(),
        format: SecretFormat::Raw,
    }
}

pub fn claude_login(m: &Machine) -> SetupStep {
    let usage = claude_usage(m);
    let request = VaultRequest {
        name: usage.name.clone(),
        kind: Some("token".into()),
        url: Some("https://claude.ai".into()),
        note: Some("Claude Code OAuth token of the separate worker account (every machine)".into()),
        reason: Some("Machines: the Claude token the workers run with".into()),
        how_to: Some("In your own Terminal (not an agent): run claude setup-token, sign in with the worker account in a private browser window, paste the token here, then clear the Terminal.".into()),
        expires: None,
    };
    let display = display(
        &usage,
        m,
        &format!(
            "Ask you to add {} in the Secret Vault, if it is not there yet.",
            usage.name
        ),
    );
    let probe = verify_probe(&usage, m.os == Some(MachineOs::Linux));
    let plan = FixPlan::new(display, false, move |ctx| {
        if !has_value(&*ctx.step.secrets, &usage.name) {
            ctx.step.opener.open_form(&request);
            return FixOutcome::fail(format!(
                "Add {} in the Secret Vault, then Set up again to send it.",
                usage.name
            ));
        }
        deliver(&usage, ctx)
    });
    SetupStep::new(
        StepId::CLAUDE_LOGIN,
        "Claude login",
        vec![StepId::REACHABLE],
        Cadence::EveryRefresh,
        probe,
        StepFix::Run(plan),
    )
}

pub fn issues_token(m: &Machine, job: &SdWorkerSettings) -> SetupStep {
    let usage = issues_usage(m, job);
    let repos: Vec<String> = job.repos.iter().map(|r| r.github.clone()).collect();
    let all_repos = job.token_all_repos == Some(true);
    let machine = m.name.clone();
    let display = display(
        &usage,
        m,
        &format!(
            "Open GitHub's token page, pre-filled, and ask you to add the token in the Secret Vault as {}, if it is not there yet.",
            usage.name
        ),
    );
    let probe = verify_probe(&usage, m.os == Some(MachineOs::Linux));
    let plan = FixPlan::new(display, false, move |ctx| {
        if !has_value(&*ctx.step.secrets, &usage.name) {
            token_creation::start(
                &*ctx.step.opener,
                &usage.name,
                &machine,
                &repos,
                all_repos,
                ctx.step.now,
            );
            return FixOutcome::fail(format!("Opened GitHub's token page. Create the token, add it in the Secret Vault as {}, then Set up again to send it.", usage.name));
        }
        deliver(&usage, ctx)
    });
    SetupStep::new(
        StepId::ISSUES_TOKEN,
        "GitHub issues token delivered",
        vec![StepId::REACHABLE],
        Cadence::EveryRefresh,
        probe,
        StepFix::Run(plan),
    )
}

pub fn usages(machines: &[Machine]) -> Vec<SecretUsage> {
    let mut list = Vec::new();
    for m in machines {
        list.push(claude_usage(m));
        for job in &m.jobs {
            if let Some(settings) = &job.sd_worker {
                list.push(issues_usage(m, settings));
            }
        }
    }
    list
}

pub fn deliver(usage: &SecretUsage, ctx: &FixContext) -> FixOutcome {
    if usage.machine != ctx.step.machine.name {
        return FixOutcome::fail(format!(
            "{} is for {}, not {}",
            usage.name, usage.machine, ctx.step.machine.name
        ));
    }
    let Some(secret) = ctx
        .step
        .secrets
        .value(&usage.name)
        .filter(|s| !s.is_empty())
    else {
        return FixOutcome::fail(format!("{} is not in the vault", usage.name));
    };
    let value = secret.expose();
    if value.chars().any(|c| c == '\n' || c == '\r' || c == '\0') {
        return FixOutcome::fail(format!(
            "the value of {} has a line break; save it again",
            usage.name
        ));
    }
    if value.len() < MINIMUM_LENGTH {
        return FixOutcome::fail(format!(
            "the value of {} is too short; save it again",
            usage.name
        ));
    }
    let Some(text) = file_text(&usage.format, value) else {
        return FixOutcome::fail(match &usage.format {
            SecretFormat::KeyValue(key) => format!("{key} is not a variable name"),
            SecretFormat::Raw => format!("the file for {} can't be built", usage.name),
        });
    };
    let mut masks = ctx.masks.clone();
    masks.hide(&usage.name, value);
    let filter = Arc::new(SecretLogFilter::new(value, &usage.name, ctx.log.clone()));
    let feed = filter.clone();
    let safe = FixContext {
        step: ctx.step.clone(),
        admin_password: None,
        log: Arc::new(move |channel, chunk| feed.feed(channel, chunk)),
        masks: masks.clone(),
    };
    let result = run::script(
        &safe,
        &shell::write_file(&usage.remote_path, "600"),
        Some(text.into_bytes()),
        600,
    );
    filter.finish();
    let hide = |t: &str| masks.apply(&filter.redact(t));
    if result.code != 0 {
        let said = shown(
            if result.err.is_empty() {
                &result.out
            } else {
                &result.err
            },
            &hide,
        );
        let mut message = format!(
            "could not write {} on {} (code {})",
            usage.remote_path, usage.machine, result.code
        );
        if !said.is_empty() {
            message.push_str(": ");
            message.push_str(&said);
        }
        return FixOutcome::fail(hide(&message));
    }
    FixOutcome::ok(format!(
        "{} is on {} at {}",
        usage.name, usage.machine, usage.remote_path
    ))
}

pub fn file_text(format: &SecretFormat, value: &str) -> Option<String> {
    match format {
        SecretFormat::Raw => Some(value.to_string()),
        SecretFormat::KeyValue(key) => {
            let mut chars = key.chars();
            let first_ok = chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
            let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
            (first_ok && rest_ok).then(|| format!("{key}={value}\n"))
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn verify_probe(usage: &SecretUsage, linux: bool) -> Probe {
    let script = format!(
        "{}; {}",
        shell::stat(&[&usage.remote_path], linux),
        shell::sha256(&usage.remote_path, linux)
    );
    let usage = usage.clone();
    Probe::remote(script, move |output, ctx| {
        if output.code != 0 {
            return CheckResult::cant_tell("the check didn't run");
        }
        let lines: Vec<&str> = output.text.split('\n').filter(|l| !l.is_empty()).collect();
        if lines.first() == Some(&"missing") {
            return CheckResult::fail("not on the machine");
        }
        let Some(facts) = shell::parse_stat(&output.text, 1)
            .into_iter()
            .next()
            .flatten()
        else {
            return CheckResult::cant_tell("couldn't read the file's mode and size");
        };
        if facts.mode != "600" {
            return CheckResult::fail(format!("mode {}, should be 600", facts.mode));
        }
        if facts.size == 0 {
            return CheckResult::fail("empty");
        }
        let found = format!("mode 600, {} bytes", facts.size);
        let Some(expected) = ctx
            .secrets
            .value(&usage.name)
            .filter(|v| !v.is_empty())
            .and_then(|v| file_text(&usage.format, v.expose()))
        else {
            return CheckResult::pass(found);
        };
        let remote = lines.get(1).map(|l| l.to_lowercase()).unwrap_or_default();
        if remote.len() != 64 || !remote.chars().all(|c| c.is_ascii_hexdigit()) {
            return CheckResult::cant_tell("couldn't compare the file with the vault");
        }
        let local = hex(&Sha256::digest(expected.as_bytes()));
        if local == remote {
            CheckResult::pass(format!("{found}, same as the vault"))
        } else {
            CheckResult::fail("different from the vault (Send again)")
        }
    })
}

fn shown(said: &str, hide: &dyn Fn(&str) -> String) -> String {
    if said.starts_with("[earlier output cut]") || said.chars().count() > 4000 {
        return "(output not shown)".into();
    }
    hide(said).trim().chars().take(300).collect()
}

fn has_value(secrets: &dyn SecretSource, name: &str) -> bool {
    secrets.value(name).is_some_and(|v| !v.is_empty())
}

fn display(usage: &SecretUsage, m: &Machine, missing: &str) -> String {
    format!("{missing}\nOtherwise: {}", send_command(usage, m))
}

pub fn send_display(usage: &SecretUsage, m: &Machine) -> String {
    format!(
        "Send the vault value of {} to {} again:\n{}",
        usage.name,
        usage.machine,
        send_command(usage, m)
    )
}

fn send_command(usage: &SecretUsage, m: &Machine) -> String {
    let source = match &usage.format {
        SecretFormat::Raw => format!("printf %s '[secret:{}]'", usage.name),
        SecretFormat::KeyValue(key) => {
            format!("printf '{key}=%s\\n' '[secret:{}]'", usage.name)
        }
    };
    let ssh = shell::ssh_script(m, &shell::write_file(&usage.remote_path, "600"))
        .iter()
        .map(|a| quote(a))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{source} | {ssh}")
}

struct SecretLogFilter {
    value: String,
    hide: BTreeMap<String, String>,
    sink: Arc<OutputSink>,
    redactors: Mutex<BTreeMap<Channel, Redactor>>,
}

impl SecretLogFilter {
    fn new(value: &str, name: &str, sink: Arc<OutputSink>) -> SecretLogFilter {
        SecretLogFilter {
            value: value.into(),
            hide: BTreeMap::from([(value.to_string(), format!("[secret:{name}]"))]),
            sink,
            redactors: Mutex::new(BTreeMap::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<Channel, Redactor>> {
        self.redactors.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn feed(&self, channel: Channel, chunk: &str) {
        let mut redactors = self.lock();
        let redactor = redactors
            .entry(channel)
            .or_insert_with(|| Redactor::new(&self.hide));
        let data = redactor.feed(chunk.as_bytes());
        self.emit(channel, &data);
    }

    fn finish(&self) {
        let mut redactors = self.lock();
        for channel in [Channel::Stdout, Channel::Stderr] {
            if let Some(held) = redactors.get_mut(&channel) {
                let data = held.finish();
                self.emit(channel, &data);
            }
        }
    }

    fn redact(&self, text: &str) -> String {
        let marker = self.hide.get(&self.value).map_or("", String::as_str);
        text.replace(&self.value, marker)
    }

    fn emit(&self, channel: Channel, data: &[u8]) {
        if !data.is_empty() {
            (self.sink)(channel, &String::from_utf8_lossy(data));
        }
    }
}

pub const LEGACY_NAMES: [(&str, &str); 3] = [
    ("sd-worker-m1", "m1-github-issues"),
    ("sd-worker-vps-sing-1", "vps-sing-1-github-issues"),
    ("sd-claude", "claude-worker"),
];

pub fn pending_renames(entries: &[VaultEntry]) -> Vec<(VaultEntry, String)> {
    LEGACY_NAMES
        .iter()
        .filter_map(|(from, to)| {
            let old = entries.iter().find(|e| e.id() == from.to_lowercase())?;
            if entries.iter().any(|e| e.id() == to.to_lowercase()) {
                return None;
            }
            Some((old.clone(), to.to_string()))
        })
        .collect()
}

type EntryLookup = Arc<dyn Fn(&str) -> Option<VaultEntry> + Send + Sync>;
type ValueRead = Arc<dyn Fn(&VaultEntry) -> Option<VaultValue> + Send + Sync>;

#[derive(Clone)]
pub struct VaultSecretSource {
    lookup: EntryLookup,
    read: ValueRead,
}

impl VaultSecretSource {
    pub fn new(
        entry: impl Fn(&str) -> Option<VaultEntry> + Send + Sync + 'static,
        value: impl Fn(&VaultEntry) -> Option<VaultValue> + Send + Sync + 'static,
    ) -> VaultSecretSource {
        VaultSecretSource {
            lookup: Arc::new(entry),
            read: Arc::new(value),
        }
    }
}

impl SecretSource for VaultSecretSource {
    fn value(&self, name: &str) -> Option<Secret> {
        (self.lookup)(name)
            .and_then(|e| (self.read)(&e))
            .map(|v| Secret::new(v.secret))
    }

    fn entry(&self, name: &str) -> Option<VaultEntry> {
        (self.lookup)(name)
    }
}
