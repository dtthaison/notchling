use super::*;

fn rig() -> (Arc<Fake>, Member, Job) {
    let fake = Arc::new(Fake::default());
    fake.expect("api-key", true);
    fake.expect("db-pass", false);
    let m = worker(&["api-key", "db-pass"]);
    let job = job_on(&m, &["api-key"], JobState::Running);
    live(&fake, &m, &job);
    (fake, m, job)
}

fn run(fake: &Arc<Fake>, m: &Member, job: &Job, list: &[&str]) -> Result<Grant, ApiError> {
    broker(fake).grant(job, &facts(m, true), &names(list), &who())
}

fn nothing_left(fake: &Fake, reads: usize) {
    assert_eq!(fake.with(|i| i.value_reads), reads);
    assert!(fake.with(|i| i.sends.is_empty()));
    assert!(!fake.log().iter().any(|l| l.starts_with("audit:")));
}

#[test]
fn a_failover_during_the_approval_reads_no_value_and_sends_nothing() {
    let (fake, m, job) = rig();
    fake.with(|i| {
        i.during_approval = Some(Box::new(|f| f.with(|i| i.me = Some(id("home")))));
    });
    let r = run(&fake, &m, &job, &["api-key"]);
    assert_eq!(code(r), ErrorCode::Forbidden);
    nothing_left(&fake, 0);
}

#[test]
fn another_home_after_the_approval_is_a_failover_too() {
    let (fake, m, job) = rig();
    fake.with(|i| {
        i.during_approval = Some(Box::new(|f| {
            f.with(|i| {
                if let Some(state) = i.state.as_mut() {
                    state.home = id("home");
                }
            })
        }));
    });
    let r = run(&fake, &m, &job, &["api-key"]);
    assert_eq!(code(r), ErrorCode::Forbidden);
    nothing_left(&fake, 0);
}

#[test]
fn a_job_that_ends_during_the_approval_gets_nothing() {
    let (fake, m, job) = rig();
    fake.with(|i| {
        i.during_approval = Some(Box::new(|f| {
            f.with(|i| {
                if let Some(state) = i.state.as_mut() {
                    if let Some(job) = state.jobs.get_mut("j-1") {
                        job.state = JobState::Done;
                    }
                }
            })
        }));
    });
    let r = run(&fake, &m, &job, &["api-key"]);
    assert_eq!(code(r), ErrorCode::Forbidden);
    nothing_left(&fake, 0);
}

#[test]
fn a_name_taken_off_the_allowlist_during_the_approval_gets_nothing() {
    let (fake, m, job) = rig();
    fake.with(|i| {
        i.during_approval = Some(Box::new(|f| {
            f.with(|i| {
                if let Some(state) = i.state.as_mut() {
                    for member in state.members.values_mut() {
                        member.secrets = SecretAllow::default();
                    }
                }
            })
        }));
    });
    let r = run(&fake, &m, &job, &["api-key"]);
    assert_eq!(code(r), ErrorCode::Forbidden);
    nothing_left(&fake, 0);
}

#[test]
fn a_member_removed_during_the_approval_gets_nothing() {
    let (fake, m, job) = rig();
    fake.with(|i| {
        i.during_approval = Some(Box::new(|f| {
            f.with(|i| {
                if let Some(state) = i.state.as_mut() {
                    for member in state.members.values_mut() {
                        member.state = MemberState::Removed;
                    }
                }
            })
        }));
    });
    let r = run(&fake, &m, &job, &["api-key"]);
    assert_eq!(code(r), ErrorCode::Forbidden);
    nothing_left(&fake, 0);
}

#[test]
fn a_change_after_the_values_are_read_stops_before_the_audit_and_the_sends() {
    let (fake, m, job) = rig();
    fake.with(|i| {
        i.after_read = Some(Box::new(|f| f.with(|i| i.me = Some(id("home")))));
    });
    let r = run(&fake, &m, &job, &["api-key", "db-pass"]);
    assert_eq!(code(r), ErrorCode::Forbidden);
    nothing_left(&fake, 1);
    assert!(fake.log().contains(&"refused:api-key".to_string()));
}

#[test]
fn a_change_between_two_sends_stops_the_second_and_revokes_the_first() {
    let (fake, m, job) = rig();
    fake.with(|i| {
        i.after_send = Some(Box::new(|f| f.with(|i| i.me = Some(id("home")))));
    });
    let r = run(&fake, &m, &job, &["api-key", "db-pass"]);
    assert_eq!(code(r), ErrorCode::Forbidden);
    let sends = fake.with(|i| i.sends.clone());
    assert_eq!(sends.len(), 2);
    assert!(sends[0].0.ends_with(":grant.deliver"));
    assert!(sends[1].0.ends_with(":grant.revoke"));
    assert!(!sends[1].1.contains("NL-MARKER"));
}

#[test]
fn a_state_that_cannot_be_read_gives_nothing() {
    let (fake, m, job) = rig();
    fake.with(|i| i.state = None);
    let r = run(&fake, &m, &job, &["api-key"]);
    assert_eq!(code(r), ErrorCode::Forbidden);
    assert!(fake.with(|i| i.sends.is_empty()));
}

#[test]
fn a_refusal_audits_only_names_that_are_names() {
    let (fake, m, job) = rig();
    let r = run(&fake, &m, &job, &["bad name"]);
    assert_eq!(code(r), ErrorCode::Forbidden);
    let log = fake.log();
    assert!(
        log.contains(&"refused:<invalid name>".to_string()),
        "{log:?}"
    );
    assert!(!log.iter().any(|l| l.contains("bad name")));
}
