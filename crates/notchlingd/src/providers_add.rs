use super::*;

pub(super) fn enlist(sh: &Shared, provider: &str) -> bool {
    let mut adding = lock(&sh.adding);
    if sh.closing.load(Ordering::SeqCst) {
        return false;
    }
    adding.insert(provider.to_string());
    true
}

pub(super) fn add_flow(
    sh: Arc<Shared>,
    p: Arc<dyn AccountProvider>,
    confirmed: bool,
    slot: Slot,
    first: std::sync::mpsc::Sender<First>,
) {
    let provider = p.id().as_str().to_string();
    let event = |state: &str, extra: Value| {
        let mut data = json!({"provider": provider, "reason": "add", "state": state});
        if let (Some(map), Some(more)) = (data.as_object_mut(), extra.as_object()) {
            map.extend(more.clone());
        }
        ProvidersOps::changed(&sh, data);
    };
    if !confirmed {
        if let Some(c) = p.add_confirmation() {
            let _ = first.send(First::Confirm(c));
            return;
        }
    }
    let started = p.begin_add();
    if started.is_ok() && !enlist(&sh, &provider) && sh.accounts.is_on() {
        p.cancel_add();
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
    let _slot = slot;
    let started_at = Instant::now();
    let limit = Duration::from_millis(sh.add_limit_ms.load(Ordering::SeqCst));
    loop {
        sh.nap(Duration::from_millis(sh.poll_ms.load(Ordering::SeqCst)));
        if sh.closing.load(Ordering::SeqCst) {
            return;
        }
        if !sh.accounts.is_on() {
            let note = p.abandon_add();
            return event("cancelled", json!({"error": ACCOUNTS_OFF, "note": note}));
        }
        match p.poll_add() {
            AddProgress::Waiting { sign_in_url } => {
                if sign_in_url != url {
                    url = sign_in_url;
                    event("waiting", json!({"sign_in_url": url}));
                }
            }
            AddProgress::Done => return event("done", json!({})),
            AddProgress::Cancelled | AddProgress::Idle => return event("cancelled", json!({})),
            AddProgress::Failed(e) => return event("failed", json!({"error": e.message()})),
        }
        if started_at.elapsed() > limit {
            p.cancel_add();
            return event("failed", json!({"error": "The sign-in took too long."}));
        }
    }
}
