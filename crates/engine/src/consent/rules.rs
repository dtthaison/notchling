use super::model::{
    Consent, New, Refusal, State, COOLDOWN_MS, HOUR_MS, LONG_ALL, LONG_EXPIRY_MS, LONG_PER_SESSION,
    OPENED_PER_HOUR, ROOT_MAX_EXPIRY_MS, ROOT_PER_HOUR, ROOT_WAITING, UNVERIFIED_PER_HOUR,
    UNVERIFIED_WAITING, WAITING_ALL, WAITING_PER_SESSION,
};

pub const BAD_EXPIRY: &str = "expiresIn takes 1 minute to 12 hours.";

fn waiting(row: &Consent) -> bool {
    row.state.open()
}

fn long(row: &Consent) -> bool {
    row.expires_ms.saturating_sub(row.created_ms) > LONG_EXPIRY_MS
}

fn busy(after: i64) -> Refusal {
    Refusal::Busy {
        retry_after_ms: after.max(1_000),
    }
}

pub fn check(new: &New, now: i64, rows: &[Consent]) -> Result<(), Refusal> {
    let bucket = new.bucket();
    let session_bucket = new.verified && new.session.is_some();
    let long_end = new.expires_in_ms > LONG_EXPIRY_MS;
    if long_end && !session_bucket {
        return Err(Refusal::LongExpiry);
    }
    if !session_bucket && new.expires_in_ms > ROOT_MAX_EXPIRY_MS {
        return Err(Refusal::LongExpiry);
    }
    let mine: Vec<&Consent> = rows.iter().filter(|r| r.bucket() == bucket).collect();
    let same_ask = |r: &&Consent| {
        r.state == State::Denied
            && r.kind == new.kind
            && r.target == new.target
            && (!session_bucket || !r.in_session() || r.bucket() == bucket)
    };
    if let Some(after) = rows
        .iter()
        .filter(same_ask)
        .filter_map(|r| r.answered_ms.or(r.closed_ms))
        .map(|at| at + COOLDOWN_MS - now)
        .filter(|left| *left > 0)
        .max()
    {
        return Err(busy(after));
    }
    let open: Vec<&&Consent> = mine.iter().filter(|r| waiting(r)).collect();
    let cap = if session_bucket {
        WAITING_PER_SESSION
    } else {
        ROOT_WAITING
    };
    if open.len() >= cap {
        let soonest = open.iter().map(|r| r.expires_ms).min().unwrap_or(now);
        return Err(busy(soonest - now));
    }
    if rows.iter().filter(|r| waiting(r)).count() >= WAITING_ALL {
        let soonest = rows
            .iter()
            .filter(|r| waiting(r))
            .map(|r| r.expires_ms)
            .min()
            .unwrap_or(now);
        return Err(busy(soonest - now));
    }
    if !session_bucket {
        let unverified: Vec<&Consent> = rows.iter().filter(|r| !r.in_session()).collect();
        let open: Vec<&&Consent> = unverified.iter().filter(|r| waiting(r)).collect();
        if open.len() >= UNVERIFIED_WAITING {
            let soonest = open.iter().map(|r| r.expires_ms).min().unwrap_or(now);
            return Err(busy(soonest - now));
        }
        let recent: Vec<i64> = unverified
            .iter()
            .map(|r| r.created_ms)
            .filter(|at| *at > now - HOUR_MS)
            .collect();
        if recent.len() >= UNVERIFIED_PER_HOUR {
            let oldest = recent.iter().copied().min().unwrap_or(now);
            return Err(busy(oldest + HOUR_MS - now));
        }
    }
    let hourly = if session_bucket {
        OPENED_PER_HOUR
    } else {
        ROOT_PER_HOUR
    };
    let recent: Vec<i64> = mine
        .iter()
        .map(|r| r.created_ms)
        .filter(|at| *at > now - HOUR_MS)
        .collect();
    if recent.len() >= hourly {
        let oldest = recent.iter().copied().min().unwrap_or(now);
        return Err(busy(oldest + HOUR_MS - now));
    }
    if long_end {
        let longs: Vec<&Consent> = rows.iter().filter(|r| waiting(r) && long(r)).collect();
        let mine_long = longs.iter().filter(|r| r.bucket() == bucket).count();
        if mine_long >= LONG_PER_SESSION || longs.len() >= LONG_ALL {
            return Err(busy(60_000));
        }
    }
    Ok(())
}
