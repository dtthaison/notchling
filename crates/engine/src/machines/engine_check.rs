use super::engine::applying;
use super::engine::{
    lock, Batch, Evaluation, SetupEngine, CHEAP_TIMEOUT, CONNECT_ERRORS, LOCAL_PARALLEL, PROBE_CAP,
    SLOW_TIMEOUT,
};
use super::masks::Masks;
use super::process::CUT_NOTICE;
use super::registry::Machine;
use super::runner::Call;
use super::shell;
use super::step::{
    Cadence, CheckResult, CheckState, Detected, Probe, RemoteOutput, SetupStep, StepContext, StepId,
};
use crate::lanes::quote;
use std::collections::{BTreeMap, BTreeSet};

fn nonce() -> Option<String> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut b = [0u8; 8];
    SystemRandom::new().fill(&mut b).ok()?;
    Some(b.iter().map(|x| format!("{x:02x}")).collect())
}

impl SetupEngine {
    pub(super) fn evaluate(
        &self,
        m: &Machine,
        slow: bool,
        use_prior: bool,
        masks: &Masks,
    ) -> Evaluation {
        let first = self.evaluate_once(m, slow, use_prior, masks);
        let build = Detected {
            os: first.detected.os,
            home: first.detected.home.clone(),
            python: first.detected.python.clone(),
            sd_path: first.detected.sd_path.clone(),
            token_expires: first.detected.token_expires.clone(),
            ..Detected::default()
        };
        let learned = applying(&build, m);
        if first.unreachable || learned == *m {
            return first;
        }
        let mut second = self.evaluate_once(&learned, slow, use_prior, masks);
        second.slow_ran |= first.slow_ran;
        second
    }

    fn evaluate_once(&self, m: &Machine, slow: bool, use_prior: bool, masks: &Masks) -> Evaluation {
        let list = self.step_list(m);
        let ctx = self.context(m, masks);
        let prior = if use_prior {
            lock(&self.state)
                .results
                .get(&m.name)
                .cloned()
                .unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        let mut e = Evaluation::default();
        let mut done: BTreeMap<StepId, CheckResult> = BTreeMap::new();
        let mut detections = Detected::default();

        let cheap: Vec<&SetupStep> = list
            .iter()
            .filter(|s| {
                matches!(s.probe, Probe::Remote { .. }) && s.cadence == Cadence::EveryRefresh
            })
            .collect();
        let mut raw = BTreeMap::new();
        if !cheap.is_empty() {
            e.tried = true;
            match self.run_batch(&cheap, &ctx, masks, CHEAP_TIMEOUT) {
                Batch::Unreachable(line, connect) => {
                    e.unreachable = true;
                    e.connect_error = connect;
                    for s in &list {
                        let r = if s.id == StepId::REACHABLE {
                            CheckResult::fail(line.clone())
                        } else {
                            CheckResult::cant_tell(format!("{} is not reachable", m.name))
                        };
                        e.results.insert(s.id.clone(), r);
                    }
                    return e;
                }
                Batch::Answers(r) => raw = r,
            }
        }

        let ids: BTreeSet<StepId> = list.iter().map(|s| s.id.clone()).collect();
        let title = |id: &StepId| {
            list.iter()
                .find(|s| &s.id == id)
                .map(|s| s.title.clone())
                .unwrap_or_else(|| id.raw().to_string())
        };
        let mut pending: Vec<&SetupStep> = list.iter().collect();
        while !pending.is_empty() {
            let ready: Vec<&SetupStep> = pending
                .iter()
                .copied()
                .filter(|s| {
                    s.needs
                        .iter()
                        .all(|n| done.contains_key(n) || !ids.contains(n))
                })
                .collect();
            if ready.is_empty() {
                for s in &pending {
                    let need = s.needs.first().map(title).unwrap_or("another step".into());
                    done.insert(
                        s.id.clone(),
                        CheckResult::cant_tell(format!("waits for {need}")),
                    );
                }
                break;
            }
            let mut slow_remote = Vec::new();
            let mut locals = Vec::new();
            for s in ready {
                let blocking = s
                    .needs
                    .iter()
                    .find(|n| !done.get(*n).is_some_and(|r| r.state.is_pass()));
                if let Some(need) = blocking {
                    done.insert(
                        s.id.clone(),
                        CheckResult::cant_tell(format!("waits for {}", title(need))),
                    );
                    continue;
                }
                if let Some(why) = s.hold_when.as_ref().and_then(|h| h(&detections)) {
                    done.insert(s.id.clone(), CheckResult::cant_tell(why));
                    continue;
                }
                match (&s.probe, s.cadence) {
                    (Probe::Remote { .. }, Cadence::EveryRefresh) => {
                        let r = raw.get(&s.id).cloned().unwrap_or_else(|| {
                            CheckResult::cant_tell("no answer from the machine")
                        });
                        detections = detections.merged(&r.detected);
                        done.insert(s.id.clone(), r);
                    }
                    (_, Cadence::Slow) if !slow => {
                        let r = prior
                            .get(&s.id)
                            .cloned()
                            .unwrap_or_else(|| CheckResult::cant_tell("not checked yet"));
                        done.insert(s.id.clone(), r);
                        e.carried.insert(s.id.clone());
                    }
                    (Probe::Remote { .. }, Cadence::Slow) => slow_remote.push(s),
                    (Probe::Local(_), _) => locals.push(s),
                }
            }
            if !slow_remote.is_empty() {
                e.slow_ran = true;
                match self.run_batch(&slow_remote, &ctx, masks, SLOW_TIMEOUT) {
                    Batch::Unreachable(..) => {
                        for s in &slow_remote {
                            done.insert(
                                s.id.clone(),
                                CheckResult::cant_tell(format!("{} is not reachable", m.name)),
                            );
                        }
                    }
                    Batch::Answers(r) => {
                        for s in &slow_remote {
                            let result = r.get(&s.id).cloned().unwrap_or_else(|| {
                                CheckResult::cant_tell("no answer from the machine")
                            });
                            detections = detections.merged(&result.detected);
                            done.insert(s.id.clone(), result);
                        }
                    }
                }
            }
            if !locals.is_empty() {
                if locals.iter().any(|s| s.cadence == Cadence::Slow) {
                    e.slow_ran = true;
                }
                for (id, r) in run_locals(&locals, &ctx) {
                    detections = detections.merged(&r.detected);
                    done.insert(id, r);
                }
            }
            pending.retain(|s| !done.contains_key(&s.id));
        }
        e.detected = detections;
        e.results = done;
        e
    }

    fn run_batch(
        &self,
        steps: &[&SetupStep],
        ctx: &StepContext,
        masks: &Masks,
        secs: u64,
    ) -> Batch {
        let Some(nonce) = nonce() else {
            return Batch::Unreachable("no randomness for the probe markers".into(), false);
        };
        let mut wrapped = Vec::new();
        let mut probes = Vec::new();
        for s in steps {
            if let Probe::Remote { script, parse } = &s.probe {
                let index = probes.len();
                probes.push((s.id.clone(), parse.clone()));
                wrapped.push(format!(
                    "out=$( /bin/sh -c {} </dev/null 2>&1 ); c=$?; printf '%s' \"$out\" | head -c {PROBE_CAP}; printf '\\n@@{nonce} {index} %s@@\\n' \"$c\"",
                    quote(script)
                ));
            }
        }
        let r = self.deps.runner.run(&Call::secs(
            shell::ssh_script(&ctx.machine, &wrapped.join("\n")),
            secs,
        ));
        let answers = split_markers(&r.out, &nonce, probes.len());
        if r.code == 255 || r.code == 124 || answers.is_empty() {
            let first = r.err.lines().next().unwrap_or("").trim().to_string();
            let line = if !first.is_empty() {
                masks.apply(&first)
            } else {
                match r.code {
                    124 => "timed out".into(),
                    127 => "ssh not found".into(),
                    0 => "no answer from the machine".into(),
                    c => format!("ssh exited {c}"),
                }
            };
            let connect = r.code == 124 || CONNECT_ERRORS.iter().any(|e| r.err.contains(e));
            return Batch::Unreachable(line, connect);
        }
        let mut parsed = BTreeMap::new();
        for (i, (id, parse)) in probes.iter().enumerate() {
            let result = match answers.get(&i) {
                Some(out) => {
                    let head = out.text.starts_with(CUT_NOTICE);
                    let tail = out.text.len() >= PROBE_CAP;
                    let out = RemoteOutput {
                        text: masks.apply_to_cut(&out.text, head, tail),
                        code: out.code,
                    };
                    parse(&out, ctx)
                }
                None => CheckResult::cant_tell("no answer from the machine"),
            };
            parsed.insert(id.clone(), result);
        }
        Batch::Answers(parsed)
    }
}

pub fn split_markers(out: &str, nonce: &str, count: usize) -> BTreeMap<usize, RemoteOutput> {
    let prefix = format!("@@{nonce} ");
    let mut answers = BTreeMap::new();
    let mut current: Vec<&str> = Vec::new();
    for line in out.split('\n') {
        if let Some(body) = line
            .strip_prefix(&prefix)
            .and_then(|l| l.strip_suffix("@@"))
        {
            let parts: Vec<&str> = body.split(' ').collect();
            if let [index, code] = parts.as_slice() {
                if let (Ok(index), Ok(code)) = (index.parse::<usize>(), code.parse::<i32>()) {
                    if index < count && !answers.contains_key(&index) {
                        answers.insert(
                            index,
                            RemoteOutput {
                                text: current.join("\n"),
                                code,
                            },
                        );
                        current.clear();
                        continue;
                    }
                }
            }
        }
        current.push(line);
    }
    answers
}

fn run_locals(steps: &[&SetupStep], ctx: &StepContext) -> Vec<(StepId, CheckResult)> {
    let mut out = Vec::new();
    for chunk in steps.chunks(LOCAL_PARALLEL) {
        let got: Vec<(StepId, CheckResult)> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .filter_map(|s| match &s.probe {
                    Probe::Local(p) => {
                        let p = p.clone();
                        let id = s.id.clone();
                        Some(scope.spawn(move || (id, p(ctx))))
                    }
                    Probe::Remote { .. } => None,
                })
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        out.extend(got);
    }
    out
}

impl CheckState {
    pub fn is_waiting(&self) -> bool {
        matches!(self, CheckState::CantTell(t) if t.starts_with("waits for"))
    }
}
