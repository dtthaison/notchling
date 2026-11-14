use crate::log_audit_tests::{called, rig, with_history, T0};
use crate::log_checks_tests::wire;
use crate::log_ops::LogOps;
use crate::op_group::OpGroup;
use notchling_local_api::{ops, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::json;

fn caller(tier: Tier) -> Caller {
    Caller {
        tier,
        client: match tier {
            Tier::App => ClientKind::App,
            Tier::Agent => ClientKind::Cli,
        },
        peer: None,
        pid: None,
    }
}

#[test]
fn specs_follow_the_tiers() {
    let wired = wire(rig("logops-spec"));
    let log = LogOps::new(wired.audit.clone(), wired.checker.clone());
    assert_eq!(log.spec(ops::LOG_VERIFY), Some(OpSpec::AGENT));
    assert_eq!(log.spec(ops::LOG_CHAIN_START), Some(OpSpec::APP));
    assert_eq!(log.spec("log.other"), None);
}

#[test]
fn verify_reply_shape() {
    let wired = wire(rig("logops-verify"));
    wired.write(vec![called(T0)]);
    let log = LogOps::new(wired.audit.clone(), wired.checker.clone());
    let reply = log
        .call(
            &caller(Tier::Agent),
            ops::LOG_VERIFY,
            &json!({"full": true}),
        )
        .unwrap();
    for key in [
        "result",
        "sealing",
        "from_n",
        "to_n",
        "first_bad_n",
        "checkpoint_n",
        "chain_started",
        "at",
    ] {
        assert!(reply.get(key).is_some(), "{key} in {reply}");
    }
    assert_eq!(reply["result"], "intact");
    assert_eq!(reply["sealing"], "sealed");
}

#[test]
fn verify_one_line_is_the_apps() {
    let wired = wire(rig("logops-line"));
    wired.write(vec![called(T0)]);
    let log = LogOps::new(wired.audit.clone(), wired.checker.clone());
    let refused = log
        .call(&caller(Tier::Agent), ops::LOG_VERIFY, &json!({"n": 3}))
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Forbidden);
    let reply = log
        .call(&caller(Tier::App), ops::LOG_VERIFY, &json!({"n": 3}))
        .unwrap();
    assert_eq!(reply["line"]["n"], 3);
    assert_eq!(reply["line"]["sealed"], true);
    assert_eq!(reply["line"]["seal_ok"], true);
    assert_eq!(reply["line"]["f"]["op"], "session.reply");
    let missing = log
        .call(&caller(Tier::App), ops::LOG_VERIFY, &json!({"n": 999}))
        .unwrap_err();
    assert_eq!(missing.code, ErrorCode::BadRequest);
}

#[test]
fn chain_start_needs_confirm() {
    let wired = wire(rig("logops-confirm"));
    let log = LogOps::new(wired.audit.clone(), wired.checker.clone());
    for args in [
        json!({}),
        json!({"confirm": false}),
        json!({"confirm": "yes"}),
    ] {
        let err = log
            .call(&caller(Tier::App), ops::LOG_CHAIN_START, &args)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }
}

#[test]
fn chain_start_writes_chain_started_and_seals_again() {
    let rig = rig("logops-chain");
    with_history(&rig);
    let wired = wire(rig);
    wired.write(vec![called(T0 + 1)]);
    assert_eq!(wired.checker.run(true).sealing, "unsealed");
    let log = LogOps::new(wired.audit.clone(), wired.checker.clone());
    let reply = log
        .call(
            &caller(Tier::App),
            ops::LOG_CHAIN_START,
            &json!({"confirm": true}),
        )
        .unwrap();
    let n = reply["chain_started_n"].as_u64().unwrap();
    let lines = wired.rig.lines();
    let line = &lines[n as usize - 1];
    assert_eq!(line["kind"], "audit.chain_started");
    assert!(line["prev"].is_null());
    assert!(line["seal"].is_string());
    let report = wired.checker.run(true);
    assert_eq!(report.sealing, "sealed");
    assert_eq!(report.result, "intact");
    assert_eq!(report.chain_started, Some(n));
}
