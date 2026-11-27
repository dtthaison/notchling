use super::*;

#[test]
fn fourteen_days_before_the_expiry_each_worker_gets_a_login_needed_notice() {
    let rig = rig();
    rig.only_worker();
    rig.deliverer.reconcile();
    assert!(rig.fake.with(|i| i.raised.is_empty()));
    rig.clock.store(noon("2026-12-17") + 1, Ordering::SeqCst);
    rig.deliverer.reconcile();
    rig.deliverer.reconcile();
    let raised = rig.fake.with(|i| i.raised.clone());
    assert_eq!(raised.len(), 1);
    let item = &raised[0];
    assert_eq!(item.kind, Kind::LoginNeeded);
    assert_eq!(
        item.title,
        format!("Claude Code's token for worker expires on {DAY}")
    );
    assert_eq!(item.source.node.as_deref(), Some(id("worker").as_str()));
    assert_eq!(item.actions.len(), 1);
    assert_eq!(item.actions[0].id, ActionId::Done);
    assert_eq!(item.state, State::Open);
    assert!(!format!("{item:?}").contains("NL-MARKER"));
}

#[test]
fn a_renewed_expiry_withdraws_the_old_notice_and_the_notice_is_not_early() {
    let rig = rig();
    rig.only_worker();
    rig.clock.store(noon("2026-12-18") + 1, Ordering::SeqCst);
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.with(|i| i.raised.len()), 1);
    rig.fake.edit_entry(|e| {
        e.expires = Some("2027-06-30".to_string());
        e.updated = "t2".to_string();
    });
    rig.deliverer.reconcile();
    let withdrawn = rig.fake.with(|i| i.withdrawn.clone());
    assert_eq!(withdrawn.len(), 1);
    assert!(withdrawn[0].contains(DAY));
    assert_eq!(rig.fake.with(|i| i.raised.len()), 1);
}

#[test]
fn the_notice_is_raised_even_while_the_worker_is_offline() {
    let rig = rig();
    rig.only_worker();
    rig.fake.with(|i| i.online.clear());
    rig.clock.store(noon("2026-12-20"), Ordering::SeqCst);
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.with(|i| i.raised.len()), 1);
    assert_eq!(rig.fake.with(|i| i.releases), 0);
}

#[test]
fn sent_stamps_survive_a_restart_so_nothing_is_released_twice() {
    let dir = Dir::new("login-stamps");
    let first = rig_with(Stamps::load(dir.path()));
    first.only_worker();
    first.deliverer.reconcile();
    assert_eq!(first.fake.sends().len(), 1);
    let text = std::fs::read_to_string(dir.path().join("login-sent.json")).expect("stamps");
    assert!(!text.contains("NL-MARKER"), "the stamps held the value");
    let second = rig_with(Stamps::load(dir.path()));
    second.only_worker();
    second.deliverer.reconcile();
    assert_eq!(second.fake.with(|i| i.releases), 0);
}

#[cfg(unix)]
#[test]
fn the_stamp_file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Dir::new("login-stamp-mode");
    let rig = rig_with(Stamps::load(dir.path()));
    rig.only_worker();
    rig.deliverer.reconcile();
    let mode = std::fs::metadata(dir.path().join("login-sent.json"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn a_damaged_stamp_file_means_no_stamps() {
    let dir = Dir::new("login-stamp-bad");
    std::fs::write(dir.path().join("login-sent.json"), "{not json").unwrap();
    let stamps = Stamps::load(dir.path());
    assert!(stamps.get(&id("worker")).is_none());
}

#[test]
fn the_expiry_day_is_the_start_of_that_utc_day() {
    let at = expires_of(&entry(Some("2026-12-31"), "t")).expect("a day");
    assert_eq!(at, day_number("2026-12-31").unwrap() * DAY_MS);
    assert_eq!(at % DAY_MS, 0);
    assert!(expires_of(&entry(None, "t")).is_none());
    assert!(expires_of(&entry(Some("soon"), "t")).is_none());
    assert!(expires_of(&entry(Some("2026-02-30"), "t")).is_none());
}

#[test]
fn the_online_flag_reads_the_status() {
    let status = json!({"members": [
        {"id": id("worker").as_str(), "online": true},
        {"id": id("other").as_str(), "online": false},
    ]});
    assert!(flag(&status, &id("worker")));
    assert!(!flag(&status, &id("other")));
    assert!(!flag(&status, &id("vault")));
    assert!(!flag(&json!({}), &id("worker")));
}

#[test]
fn every_eligible_node_gets_the_login_op_and_nothing_else() {
    let rig = rig();
    rig.deliverer.reconcile();
    for send in rig.fake.sends() {
        assert_eq!(send.1, LOGIN_DELIVER);
    }
    let nodes: BTreeSet<NodeId> = rig.fake.sends().into_iter().map(|s| s.0).collect();
    assert_eq!(nodes, [id("worker"), id("other")].into_iter().collect());
}

fn stamped(rig: &Rig) {
    rig.only_worker();
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.sends().len(), 1);
}

fn revokes(rig: &Rig) -> usize {
    rig.fake
        .sends()
        .iter()
        .filter(|s| s.1 == LOGIN_REVOKE)
        .count()
}

#[test]
fn taking_the_entry_off_the_allowlist_revokes_the_login_once() {
    let rig = rig();
    stamped(&rig);
    rig.fake.edit_state(|s| {
        s.members.get_mut(&id("worker")).unwrap().secrets =
            SecretAllow::new(["other".to_string()], false);
    });
    rig.deliverer.reconcile();
    assert_eq!(revokes(&rig), 1);
    rig.deliverer.reconcile();
    assert_eq!(revokes(&rig), 1);
}

#[test]
fn deleting_the_entry_or_letting_it_expire_revokes_the_login() {
    let rig = rig();
    stamped(&rig);
    rig.fake.with(|i| i.entry = None);
    rig.deliverer.reconcile();
    assert_eq!(revokes(&rig), 1);
    let rig = self::rig();
    stamped(&rig);
    rig.pass(60 * DAY_MS);
    rig.deliverer.reconcile();
    assert_eq!(revokes(&rig), 1);
}

#[test]
fn a_revoke_waits_for_the_node_and_a_removed_node_is_forgotten_without_one() {
    let rig = rig();
    stamped(&rig);
    rig.fake.with(|i| i.entry = None);
    rig.fake.with(|i| i.online.clear());
    rig.deliverer.reconcile();
    assert_eq!(revokes(&rig), 0);
    rig.fake.with(|i| i.online.insert(id("worker")));
    rig.deliverer.reconcile();
    assert_eq!(revokes(&rig), 1);
    let removed = self::rig();
    stamped(&removed);
    removed.fake.edit_state(|s| {
        s.members.get_mut(&id("worker")).unwrap().state = MemberState::Removed;
    });
    removed.deliverer.reconcile();
    removed.deliverer.reconcile();
    assert_eq!(revokes(&removed), 0);
}

#[test]
fn a_node_that_stays_eligible_is_never_revoked() {
    let rig = rig();
    stamped(&rig);
    rig.deliverer.reconcile();
    rig.pass(RESEND_MS * 3);
    rig.fake.edit_state(|s| {
        s.members.get_mut(&id("worker")).unwrap().versions = Some(versions(true, true));
    });
    rig.deliverer.reconcile();
    assert_eq!(revokes(&rig), 0);
}

#[test]
fn a_brief_flap_of_eligibility_keeps_the_notice_and_does_not_double_it() {
    let rig = rig();
    rig.only_worker();
    rig.clock.store(noon("2026-12-20"), Ordering::SeqCst);
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.with(|i| i.raised.len()), 1);
    rig.fake.edit_state(|s| {
        s.members.get_mut(&id("worker")).unwrap().versions = None;
    });
    rig.deliverer.reconcile();
    assert!(rig.fake.with(|i| i.withdrawn.is_empty()));
    rig.fake.edit_state(|s| {
        s.members.get_mut(&id("worker")).unwrap().versions = Some(versions(true, false));
    });
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.with(|i| i.raised.len()), 1);
    assert_eq!(rig.fake.open_ids().len(), 1);
}

#[test]
fn deleting_the_entry_withdraws_its_open_notices() {
    let rig = rig();
    rig.only_worker();
    rig.clock.store(noon("2026-12-20"), Ordering::SeqCst);
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.open_ids().len(), 1);
    rig.fake.with(|i| i.entry = None);
    rig.deliverer.reconcile();
    assert!(rig.fake.open_ids().is_empty());
    assert_eq!(rig.fake.with(|i| i.withdrawn.len()), 1);
}

#[test]
fn no_expiry_notice_needs_an_eligible_worker() {
    let rig = rig();
    rig.fake.edit_entry(|e| e.expires = None);
    rig.fake.edit_state(|s| {
        s.members.remove(&id("worker"));
        s.members.remove(&id("other"));
    });
    rig.deliverer.reconcile();
    assert!(rig.fake.with(|i| i.raised.is_empty()));
}

#[test]
fn the_notices_survive_a_restart_that_withdraws_open_items() {
    use crate::acting_ops_tests::FakeRoom;
    use crate::inbox_ops::InboxOps;
    let dir = Dir::new("login-notes-restart");
    let open = |at: i64| {
        let ops = InboxOps::with_clock(
            dir.path().to_path_buf(),
            Arc::new(FakeRoom::default()),
            Arc::new(move || at),
        );
        ops.load();
        ops
    };
    let first = open(1_000);
    let rig = rig_notes(Stamps::in_memory(), Some(first.clone()));
    rig.only_worker();
    rig.clock.store(noon("2026-12-20"), Ordering::SeqCst);
    rig.deliverer.reconcile();
    assert_eq!(first.open_ids().len(), 1);
    first.flush();
    let second = open(2_000);
    assert!(
        second.open_ids().is_empty(),
        "the load withdraws open items"
    );
    let rig = rig_notes(Stamps::in_memory(), Some(second.clone()));
    rig.only_worker();
    rig.clock.store(noon("2026-12-20"), Ordering::SeqCst);
    rig.deliverer.reconcile();
    rig.deliverer.reconcile();
    let open_now = second.open_ids();
    assert_eq!(open_now.len(), 1, "{open_now:?}");
    assert!(open_now[0].ends_with("#2"), "{open_now:?}");
}

#[test]
fn a_notice_the_user_marked_done_is_not_raised_again_after_a_restart() {
    use crate::acting_ops_tests::FakeRoom;
    use crate::inbox_ops::InboxOps;
    let dir = Dir::new("login-notes-done");
    let ops = InboxOps::with_clock(
        dir.path().to_path_buf(),
        Arc::new(FakeRoom::default()),
        Arc::new(|| 1_000),
    );
    let rig = rig_notes(Stamps::in_memory(), Some(ops.clone()));
    rig.only_worker();
    rig.clock.store(noon("2026-12-20"), Ordering::SeqCst);
    rig.deliverer.reconcile();
    let id_open = ops.open_ids().remove(0);
    ops.resolve(&id_open, State::Answered, Some(ActionId::Done), Some("app"));
    let again = rig_notes(Stamps::in_memory(), Some(ops.clone()));
    again.only_worker();
    again.clock.store(noon("2026-12-20"), Ordering::SeqCst);
    again.deliverer.reconcile();
    assert!(ops.open_ids().is_empty());
}

#[test]
fn a_deliver_that_failed_on_the_link_may_have_stored_so_a_later_revoke_reaches_it() {
    let rig = rig();
    rig.only_worker();
    rig.fake.with(|i| i.link_down = true);
    rig.deliverer.reconcile();
    assert_eq!(rig.fake.sends().len(), 1);
    rig.fake.with(|i| i.link_down = false);
    rig.fake.with(|i| i.entry = None);
    rig.deliverer.reconcile();
    assert_eq!(revokes(&rig), 1);
}

#[test]
fn a_deliver_that_was_refused_before_the_call_leaves_nothing_to_revoke() {
    let rig = rig();
    rig.only_worker();
    rig.fake
        .with(|i| i.release_err = Some(ApiError::new(ErrorCode::Failed, "busy")));
    rig.deliverer.reconcile();
    rig.fake.with(|i| i.entry = None);
    rig.deliverer.reconcile();
    assert!(rig.fake.sends().is_empty());
}

#[test]
fn a_worker_that_rejects_the_revoke_is_asked_again_only_every_ten_minutes() {
    let rig = rig();
    stamped(&rig);
    rig.fake.with(|i| i.entry = None);
    rig.fake.with(|i| i.link_down = true);
    rig.deliverer.reconcile();
    rig.deliverer.reconcile();
    rig.pass(HOLD_MS - 1);
    rig.deliverer.reconcile();
    assert_eq!(revokes(&rig), 1);
    rig.pass(2);
    rig.deliverer.reconcile();
    assert_eq!(revokes(&rig), 2);
    rig.fake.with(|i| i.link_down = false);
    rig.pass(HOLD_MS + 1);
    rig.deliverer.reconcile();
    assert_eq!(revokes(&rig), 3);
    rig.pass(HOLD_MS + 1);
    rig.deliverer.reconcile();
    assert_eq!(revokes(&rig), 3);
}
