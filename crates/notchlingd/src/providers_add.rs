use super::*;

#[derive(Default)]
pub(super) struct Run {
    pub(super) attempt: u64,
    asked: AtomicBool,
    over: Mutex<bool>,
    gone: Condvar,
}

impl Run {
    pub(super) fn new(attempt: u64) -> Run {
        Run {
            attempt,
            ..Run::default()
        }
    }

    pub(super) fn cancel(&self) {
        self.asked.store(true, Ordering::SeqCst);
    }

    pub(super) fn cancelled(&self) -> bool {
        self.asked.load(Ordering::SeqCst)
    }

    pub(super) fn ended(&self) {
        *lock(&self.over) = true;
        self.gone.notify_all();
    }

    pub(super) fn wait_end(&self, wait: Duration) -> bool {
        let over = lock(&self.over);
        let (over, _) = self
            .gone
            .wait_timeout_while(over, wait, |over| !*over)
            .unwrap_or_else(|e| e.into_inner());
        *over
    }
}

pub(super) fn enlist(sh: &Shared, provider: &str) -> bool {
    let mut adding = lock(&sh.adding);
    if sh.closing.load(Ordering::SeqCst) {
        return false;
    }
    adding.insert(provider.to_string());
    true
}

fn known(state: &ProviderState) -> Vec<String> {
    state
        .accounts
        .iter()
        .map(|a| a.id.clone())
        .chain(state.active_id.clone())
        .collect()
}

fn stopping() -> ApiError {
    ApiError::new(ErrorCode::Failed, "Notchling is stopping.")
}

fn halted(sh: &Shared) -> Option<ApiError> {
    if sh.closing.load(Ordering::SeqCst) {
        return Some(stopping());
    }
    if !sh.accounts.is_on() {
        return Some(sh.accounts.refusal());
    }
    None
}

fn asked(sh: &Shared, run: &Run) -> Option<ApiError> {
    halted(sh).or_else(|| {
        run.cancelled()
            .then(|| ApiError::new(ErrorCode::Failed, "The sign-in was cancelled."))
    })
}

fn end(sh: &Shared, p: &dyn AccountProvider) -> Option<String> {
    if sh.accounts.is_on() {
        p.cancel_add();
    }
    p.abandon_add()
}

fn keep(sh: &Shared, attempt: u64, before: &[String], p: &dyn AccountProvider) {
    let ended_on: Vec<String> = match p.added_account() {
        Some(named) => named.into_iter().collect(),
        None => p.state().accounts.into_iter().map(|a| a.id).collect(),
    };
    let stored: Vec<String> = ended_on
        .into_iter()
        .filter(|id| !before.contains(id))
        .collect();
    let mut added = lock(&sh.added);
    while added.len() >= ADDS_KEPT {
        added.pop_front();
    }
    added.push_back((attempt, stored));
}

pub(super) fn add_flow(
    sh: Arc<Shared>,
    p: Arc<dyn AccountProvider>,
    confirmed: bool,
    slot: Slot,
    attempt: u64,
    first: std::sync::mpsc::Sender<First>,
) {
    let provider = p.id().as_str().to_string();
    let event = |state: &str, extra: Value| {
        let mut data = json!({
            "provider": provider, "reason": "add", "state": state, "attempt": attempt,
        });
        if let (Some(map), Some(more)) = (data.as_object_mut(), extra.as_object()) {
            map.extend(more.clone());
        }
        ProvidersOps::changed(&sh, data);
    };
    let refuse = |refusal: ApiError, note: Option<String>| {
        let mut extra = json!({"error": refusal.message});
        if let Some(note) = note {
            extra["note"] = json!(note);
        }
        let _ = first.send(First::Refused(refusal));
        event("cancelled", extra);
    };
    if let Some(refusal) = halted(&sh) {
        return refuse(refusal, None);
    }
    if !confirmed {
        if let Some(c) = p.add_confirmation() {
            let _ = first.send(First::Confirm(c));
            return;
        }
    }
    p.sync(false);
    if let Some(refusal) = asked(&sh, &slot.run) {
        return refuse(refusal, None);
    }
    let started = p.begin_add();
    let before = known(&p.state());
    let listed = started.is_ok() && enlist(&sh, &provider);
    let stop = asked(&sh, &slot.run).or_else(|| (started.is_ok() && !listed).then(stopping));
    if let Some(refusal) = stop {
        let note = started.is_ok().then(|| end(&sh, p.as_ref())).flatten();
        return refuse(refusal, note);
    }
    let _ = first.send(First::Started(started.clone()));
    let mut url = match started {
        Ok(started) => {
            event("started", json!({"sign_in_url": started.sign_in_url}));
            started.sign_in_url
        }
        Err(e) => {
            event("failed", json!({"error": e.message()}));
            return;
        }
    };
    let started_at = Instant::now();
    let limit = Duration::from_millis(sh.add_limit_ms.load(Ordering::SeqCst));
    loop {
        sh.nap(
            Duration::from_millis(sh.poll_ms.load(Ordering::SeqCst)),
            &slot.run,
        );
        if let Some(refusal) = halted(&sh) {
            let note = end(&sh, p.as_ref());
            return event("cancelled", json!({"error": refusal.message, "note": note}));
        }
        if slot.run.cancelled() {
            let note = end(&sh, p.as_ref());
            return event("cancelled", json!({"note": note}));
        }
        match p.poll_add() {
            AddProgress::Waiting { sign_in_url } => {
                if sign_in_url != url {
                    url = sign_in_url;
                    event("waiting", json!({"sign_in_url": url}));
                }
            }
            AddProgress::Done => {
                keep(&sh, attempt, &before, p.as_ref());
                return event("done", json!({}));
            }
            AddProgress::Cancelled | AddProgress::Idle => return event("cancelled", json!({})),
            AddProgress::Failed(e) => return event("failed", json!({"error": e.message()})),
        }
        if started_at.elapsed() > limit {
            let note = end(&sh, p.as_ref());
            return event(
                "failed",
                json!({"error": "The sign-in took too long.", "note": note}),
            );
        }
    }
}
