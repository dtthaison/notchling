use notchling_engine::machines::addresses::Route;
use notchling_engine::machines::host_trust::{HostKey, ScanFailure};

use super::trust::*;

fn key() -> HostKey {
    HostKey {
        field: "m1.example".into(),
        kind: "ssh-ed25519".into(),
        blob: "AAAA".into(),
        fingerprint: "SHA256:abc".into(),
    }
}

fn ask() -> TrustAsk {
    TrustAsk::begin(
        TrustWhere::Tailscale("m1.example".into()),
        Route::direct("m1.example", "me"),
        7,
    )
}

#[test]
fn a_direct_host_is_scanned_then_shown_before_anything_is_saved() {
    let mut ask = ask();
    assert!(ask.needs_scan());
    assert_eq!(ask.confirm(), None, "nothing to confirm before the scan");
    ask.scanned(7, Ok(vec![key()]));
    assert_eq!(ask.flow, TrustFlow::Review(vec![key()]));
    assert_eq!(ask.confirm(), Some(vec![key()]));
    assert!(matches!(ask.flow, TrustFlow::Saving(_)));
    assert_eq!(ask.confirm(), None, "a second click saves nothing more");
    assert!(ask.saved(7, Ok(1)));
}

#[test]
fn a_late_scan_or_save_for_another_round_is_dropped() {
    let mut ask = ask();
    ask.scanned(6, Ok(vec![key()]));
    assert!(ask.needs_scan());
    ask.scanned(7, Ok(vec![key()]));
    ask.confirm();
    assert!(!ask.saved(6, Ok(1)));
    assert!(matches!(ask.flow, TrustFlow::Saving(_)));
}

#[test]
fn a_jump_route_cannot_be_trusted_from_here() {
    let route = Route {
        jump: Some(Box::new(Route::direct("gw", "hop"))),
        ..Route::direct("m1.example", "me")
    };
    let ask = TrustAsk::begin(TrustWhere::Edit("m1".into()), route, 1);
    assert_eq!(ask.flow, TrustFlow::Failed(TrustFail::Jump));
    assert!(!ask.needs_scan());
}

#[test]
fn a_failed_scan_or_write_is_kept_to_show() {
    let mut ask = ask();
    ask.scanned(7, Err(ScanFailure::Nothing));
    assert_eq!(ask.flow, TrustFlow::Failed(TrustFail::Nothing));
    let mut ask = self::ask();
    ask.scanned(7, Ok(vec![key()]));
    ask.confirm();
    assert!(!ask.saved(7, Err("read-only".into())));
    assert_eq!(
        ask.flow,
        TrustFlow::Failed(TrustFail::Write("read-only".into()))
    );
}
