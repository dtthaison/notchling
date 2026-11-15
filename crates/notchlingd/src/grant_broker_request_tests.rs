use super::*;

fn request_rig(home_is_vault: bool) -> (Arc<Fake>, Member, Job) {
    let fake = Arc::new(Fake::default());
    fake.expect("api-key", true);
    let vault = member("vault-mac");
    let other = member("home");
    let w = worker(&["api-key"]);
    let job = job_on(&w, &["api-key"], JobState::Running);
    let home = if home_is_vault { &vault } else { &other };
    let state = cluster(home, &vault, &w, job.clone());
    fake.with(|i| {
        i.state = Some(state);
        i.me = Some(vault.id.clone());
    });
    (fake, w, job)
}

fn origin(member: &Member) -> Origin {
    Origin {
        node: member.id.clone(),
        tier: OriginTier::Agent,
    }
}

fn request_args(job: &Job) -> Value {
    json!({
        "job": job.id,
        "names": ["api-key"],
        "command": "make deploy",
        "folder": "/work",
    })
}

#[test]
fn a_request_from_the_jobs_own_node_is_released_when_the_vault_is_primary() {
    let (fake, w, job) = request_rig(true);
    let out = broker(&fake)
        .request(&origin(&w), &request_args(&job))
        .expect("granted");
    assert_eq!(out["delivered"], json!(["api-key"]));
    assert_eq!(fake.with(|i| i.releases), 1);
}

#[test]
fn a_request_waits_while_another_node_is_primary() {
    let (fake, w, job) = request_rig(false);
    let out = broker(&fake)
        .request(&origin(&w), &request_args(&job))
        .expect("waits");
    assert!(out["waiting"].as_str().is_some());
    assert_eq!(
        fake.with(|i| (i.lookups, i.releases, i.sends.len())),
        (0, 0, 0)
    );
}

#[test]
fn a_request_for_another_nodes_job_reads_nothing() {
    let (fake, _w, job) = request_rig(true);
    let stranger = member("stranger");
    let e = broker(&fake)
        .request(&origin(&stranger), &request_args(&job))
        .expect_err("refused");
    assert_eq!(e.code, ErrorCode::Forbidden);
    assert_eq!(
        fake.with(|i| (i.lookups, i.releases, i.sends.len())),
        (0, 0, 0)
    );
}

#[test]
fn a_node_that_is_not_the_vault_node_refuses_requests() {
    let (fake, w, job) = request_rig(true);
    fake.with(|i| i.me = Some(id("home")));
    let e = broker(&fake)
        .request(&origin(&w), &request_args(&job))
        .expect_err("refused");
    assert_eq!(e.code, ErrorCode::Forbidden);
    assert_eq!(fake.with(|i| (i.lookups, i.releases)), (0, 0));
}

#[test]
fn a_malformed_request_is_a_bad_request() {
    let (fake, w, job) = request_rig(true);
    for args in [
        json!({"names": ["api-key"]}),
        json!({"job": job.id}),
        json!({"job": job.id, "names": [1]}),
        json!({"job": "j-missing", "names": ["api-key"]}),
    ] {
        let e = broker(&fake)
            .request(&origin(&w), &args)
            .expect_err("refused");
        assert!(matches!(e.code, ErrorCode::BadRequest | ErrorCode::Failed));
    }
    assert_eq!(fake.with(|i| i.releases), 0);
}

fn attach_value(m: &Member, job: &Job, primary: bool) -> Value {
    json!({
        "job": job,
        "member": m,
        "vaultName": "vault-mac",
        "vaultPrimary": primary,
    })
}

#[test]
fn an_attach_delivers_the_jobs_secrets_and_clears_its_notice() {
    let fake = Arc::new(Fake::default());
    fake.expect("api-key", false);
    let m = worker(&["api-key"]);
    let job = job_on(&m, &["api-key"], JobState::Dispatching);
    live(&fake, &m, &job);
    let out = broker(&fake)
        .attach(&attach_value(&m, &job, true))
        .expect("attached");
    assert_eq!(out["delivered"], json!(["api-key"]));
    assert_eq!(fake.with(|i| i.cleared.clone()), ["j-1"]);
    assert!(fake.with(|i| i.waiting.is_empty()));
}

#[test]
fn an_attach_while_another_node_is_primary_raises_a_notice_and_reads_nothing() {
    let fake = Arc::new(Fake::default());
    fake.expect("api-key", true);
    let m = worker(&["api-key"]);
    let job = job_on(&m, &["api-key"], JobState::Dispatching);
    live(&fake, &m, &job);
    let out = broker(&fake)
        .attach(&attach_value(&m, &job, false))
        .expect("waits");
    assert!(out["waiting"].as_str().is_some());
    assert_eq!(fake.with(|i| i.waiting.clone()), ["j-1"]);
    assert_eq!(
        fake.with(|i| (i.lookups, i.releases, i.sends.len())),
        (0, 0, 0)
    );
}

#[test]
fn an_attach_that_is_refused_fails_and_carries_no_value() {
    let fake = Arc::new(Fake::default());
    fake.expect("api-key", false);
    let m = worker(&[]);
    let job = job_on(&m, &["api-key"], JobState::Dispatching);
    live(&fake, &m, &job);
    let e = broker(&fake)
        .attach(&attach_value(&m, &job, true))
        .expect_err("refused");
    assert_eq!(e.code, ErrorCode::Forbidden);
    assert!(!e.message.contains("NL-MARKER"));
    assert_eq!(fake.with(|i| (i.releases, i.sends.len())), (0, 0));
}

#[test]
fn a_garbled_attach_is_a_bad_request() {
    let fake = Arc::new(Fake::default());
    let e = broker(&fake)
        .attach(&json!({"job": "j-1"}))
        .expect_err("refused");
    assert_eq!(e.code, ErrorCode::BadRequest);
}

#[test]
fn a_revoke_clears_the_notice_and_tells_the_node() {
    let fake = Arc::new(Fake::default());
    let m = worker(&[]);
    let out = broker(&fake).revoke(&json!({"job": "j-1", "node": m.id.as_str()}));
    assert!(out.is_ok());
    assert_eq!(fake.with(|i| i.cleared.clone()), ["j-1"]);
    let sends = fake.with(|i| i.sends.clone());
    assert_eq!(sends.len(), 1);
    assert!(sends[0].0.ends_with(":grant.revoke"));
    let e = broker(&fake)
        .revoke(&json!({"job": "j-1"}))
        .expect_err("bad");
    assert_eq!(e.code, ErrorCode::BadRequest);
}
