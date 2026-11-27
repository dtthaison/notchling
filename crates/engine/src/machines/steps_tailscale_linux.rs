use super::registry::Machine;
use super::runner::{Channel, OutputSink};
use super::step::{
    run, CheckResult, Detected, FixContext, FixOutcome, FixPlan, LinkOffer, SetupStep, StepFix,
    StepId, TailscaleState,
};
use super::steps_node::first_text_line;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub const INSTALL: &str = "curl -fsSL https://tailscale.com/install.sh -o /tmp/ts-install.sh && sh /tmp/ts-install.sh; rm -f /tmp/ts-install.sh";
pub const SIGN_IN: &str = "tailscale up --timeout=300s";

const LINK_PREFIX: &str = "https://login.tailscale.com/";
const HOST: &str = "login.tailscale.com";
const HIDDEN: &str = "[sign-in link]";
const LINK_MAX: usize = 300;
const LONG_LINE: usize = 4096;
pub const INSTALL_SECS: u64 = 900;
pub const SIGN_IN_SECS: u64 = 330;

pub fn is_sign_in_link(url: &str) -> bool {
    let Some(path) = url.strip_prefix(LINK_PREFIX) else {
        return false;
    };
    url.len() <= LINK_MAX
        && !path.is_empty()
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.' | '~'))
}

pub(super) fn detect(linux: bool, code: i32, backend: Option<&str>) -> Option<TailscaleState> {
    if !linux {
        return None;
    }
    match backend {
        Some("NeedsLogin") => Some(TailscaleState::LoggedOut),
        None if code == 127 => Some(TailscaleState::Missing),
        _ => None,
    }
}

pub(super) fn tagged(
    result: CheckResult,
    linux: bool,
    code: i32,
    backend: Option<&str>,
) -> CheckResult {
    result.detected(Detected {
        tailscale: detect(linux, code, backend),
        ..Detected::default()
    })
}

pub(super) fn state_of(results: &BTreeMap<StepId, CheckResult>) -> Option<TailscaleState> {
    results
        .get(&StepId::NODE_TAILSCALE)
        .filter(|r| r.state.is_fail())
        .and_then(|r| r.detected.tailscale)
}

pub(super) fn with_fix(
    mut list: Vec<SetupStep>,
    machine: &Machine,
    state: Option<TailscaleState>,
) -> Vec<SetupStep> {
    if !machine.is_linux() {
        return list;
    }
    let plan = match state {
        Some(TailscaleState::Missing) => FixPlan::new(INSTALL, true, install_fix),
        Some(TailscaleState::LoggedOut) => FixPlan::new(SIGN_IN, true, sign_in_fix),
        None => return list,
    };
    if let Some(step) = list.iter_mut().find(|s| s.id == StepId::NODE_TAILSCALE) {
        step.fix = StepFix::Run(plan);
    }
    list
}

fn install_fix(ctx: &FixContext) -> FixOutcome {
    let r = run::admin(ctx, INSTALL, INSTALL_SECS);
    if r.code != 0 {
        return FixOutcome::fail(failure(
            ctx,
            "the Tailscale install",
            &r.code,
            &r.out,
            &r.err,
        ));
    }
    let found = run::admin(ctx, "command -v tailscale >/dev/null 2>&1", 60);
    if found.code != 0 {
        return FixOutcome::fail("Tailscale didn't install; the log above says why");
    }
    FixOutcome::ok("Tailscale is installed; sign in next")
}

fn sign_in_fix(ctx: &FixContext) -> FixOutcome {
    let filter = Arc::new(LinkFilter::new(ctx.log.clone(), ctx.links.clone()));
    let feed = filter.clone();
    let safe = FixContext {
        step: ctx.step.clone(),
        admin_password: ctx.admin_password.clone(),
        log: Arc::new(move |channel, chunk| feed.feed(channel, chunk)),
        masks: ctx.masks.clone(),
        links: None,
    };
    let r = run::admin(&safe, SIGN_IN, SIGN_IN_SECS);
    filter.finish();
    if r.code != 0 {
        return FixOutcome::fail(failure(ctx, "tailscale up", &r.code, &r.out, &r.err));
    }
    FixOutcome::ok("Tailscale is signed in")
}

fn failure(ctx: &FixContext, what: &str, code: &i32, out: &str, err: &str) -> String {
    let said = scrub(if err.trim().is_empty() { out } else { err }).0;
    let line = first_text_line(&said);
    let text = if line.is_empty() {
        format!("{what} exited {code}")
    } else {
        format!("{what} exited {code}: {line}")
    };
    ctx.mask(&text)
}

fn scrub(text: &str) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut found = Vec::new();
    let mut token = String::new();
    let flush = |token: &mut String, out: &mut String, found: &mut Vec<String>| {
        if token.is_empty() {
            return;
        }
        if token.to_ascii_lowercase().contains(HOST) {
            found.push(std::mem::take(token));
            out.push_str(HIDDEN);
        } else {
            out.push_str(token);
            token.clear();
        }
    };
    for c in text.chars() {
        if c.is_whitespace() {
            flush(&mut token, &mut out, &mut found);
            out.push(c);
        } else {
            token.push(c);
        }
    }
    flush(&mut token, &mut out, &mut found);
    (out, found)
}

struct LinkFilter {
    sink: Arc<OutputSink>,
    offer: Option<LinkOffer>,
    partial: Mutex<BTreeMap<Channel, String>>,
    offered: Mutex<Vec<String>>,
}

impl LinkFilter {
    fn new(sink: Arc<OutputSink>, offer: Option<LinkOffer>) -> LinkFilter {
        LinkFilter {
            sink,
            offer,
            partial: Mutex::new(BTreeMap::new()),
            offered: Mutex::new(Vec::new()),
        }
    }

    fn feed(&self, channel: Channel, chunk: &str) {
        let mut lines = Vec::new();
        {
            let mut partial = self.partial.lock().unwrap_or_else(|e| e.into_inner());
            let buf = partial.entry(channel).or_default();
            buf.push_str(chunk);
            while let Some(at) = buf.find('\n') {
                lines.push(buf.drain(..=at).collect::<String>());
            }
            if buf.len() > LONG_LINE {
                buf.clear();
                lines.push("[output line cut]\n".into());
            }
        }
        for line in lines {
            self.emit(channel, &line, true);
        }
    }

    fn finish(&self) {
        let rest = std::mem::take(&mut *self.partial.lock().unwrap_or_else(|e| e.into_inner()));
        for (channel, text) in rest {
            if !text.is_empty() {
                self.emit(channel, &format!("{text}\n"), false);
            }
        }
    }

    fn emit(&self, channel: Channel, line: &str, complete: bool) {
        let (shown, found) = scrub(line);
        for url in found.into_iter().filter(|u| complete && is_sign_in_link(u)) {
            let new = {
                let mut offered = self.offered.lock().unwrap_or_else(|e| e.into_inner());
                let new = !offered.contains(&url);
                if new {
                    offered.push(url.clone());
                }
                new
            };
            if let (true, Some(offer)) = (new, &self.offer) {
                offer(&url);
            }
        }
        (self.sink)(channel, &shown);
    }
}
