use std::collections::BTreeMap;

use super::fence::{
    after_wait, decide, decide_bytes, parse, Fence, FenceFile, Gate, FENCE_VERSION, MAX_FENCE_BYTES,
};
use super::members::Term;
use super::testkit::id;

const HOUR_MS: u64 = 3_600_000;

fn file(gates: &[(&str, Gate)], boot: &str, valid_until: u64) -> FenceFile {
    let mut sessions = BTreeMap::new();
    for (s, g) in gates {
        sessions.insert((*s).to_string(), g.clone());
    }
    FenceFile {
        v: FENCE_VERSION,
        term: Term {
            n: 3,
            node: id("m5"),
        },
        node: id("m5"),
        boot_id: boot.to_string(),
        valid_until,
        sessions,
    }
}

fn json(f: &FenceFile) -> Vec<u8> {
    serde_json::to_vec(f).expect("json")
}

#[test]
fn no_file_passes() {
    assert_eq!(decide(None, "s1", "b1", 10), Fence::Pass);
    assert_eq!(decide_bytes(None, "s1", "b1", 10), Fence::Pass);
}

#[test]
fn unlisted_session_passes() {
    let f = file(&[("s1", Gate::Lease)], "b1", 0);
    assert_eq!(decide(Some(&f), "other", "b1", 10), Fence::Pass);
}

#[test]
fn free_passes_after_any_sleep() {
    let f = file(&[("s1", Gate::Free)], "b0", 0);
    assert_eq!(decide(Some(&f), "s1", "b1", 100 * HOUR_MS), Fence::Pass);
}

#[test]
fn valid_lease_passes() {
    let f = file(&[("s1", Gate::Lease)], "b1", 1_000);
    assert_eq!(decide(Some(&f), "s1", "b1", 999), Fence::Pass);
    assert_eq!(decide(Some(&f), "s1", "b1", 1_000), Fence::Wait);
}

#[test]
fn expired_after_sleep_waits_then_denies() {
    let f = file(&[("s1", Gate::Lease)], "b1", 10 * HOUR_MS);
    assert_eq!(decide(Some(&f), "s1", "b1", 13 * HOUR_MS), Fence::Wait);
    let Fence::Deny(text) = after_wait() else {
        panic!("after_wait must deny");
    };
    assert_eq!(
        text,
        "Notchling can't reach the Standby that would take over this session, so it's paused to keep it from running on two machines. Check the cluster in Notchling."
    );
}

#[test]
fn other_boot_id_never_passes() {
    let f = file(&[("s1", Gate::Lease)], "b1", u64::MAX);
    assert_eq!(decide(Some(&f), "s1", "b2", 0), Fence::Wait);
    assert_eq!(decide(Some(&f), "s1", "b1", 0), Fence::Pass);
}

#[test]
fn moved_denies_naming_the_machine() {
    let f = file(
        &[(
            "s1",
            Gate::Moved {
                to: "m1".to_string(),
            },
        )],
        "b1",
        u64::MAX,
    );
    assert_eq!(
        decide(Some(&f), "s1", "b1", 0),
        Fence::Deny("This session continues on m1. Open it there.".to_string())
    );
}

#[test]
fn waiting_denies_with_the_choice() {
    let f = file(
        &[(
            "s1",
            Gate::Waiting {
                for_node: "m5".to_string(),
            },
        )],
        "b1",
        u64::MAX,
    );
    assert_eq!(
        decide(Some(&f), "s1", "b1", 0),
        Fence::Deny(
            "This session waits for m5, which has newer work in it. To go on here, choose Continue From This Copy in Notchling."
                .to_string()
        )
    );
}

#[test]
fn garbage_file_waits_never_passes() {
    assert_eq!(decide_bytes(Some(b"not json"), "s1", "b1", 0), Fence::Wait);
    assert_eq!(decide_bytes(Some(b""), "s1", "b1", 0), Fence::Wait);
    assert_eq!(decide_bytes(Some(b"{}"), "s1", "b1", 0), Fence::Wait);
    let well_formed = json(&file(&[("s1", Gate::Lease)], "b1", HOUR_MS));
    let bogus = String::from_utf8(well_formed)
        .expect("utf8")
        .replace("\"lease\"", "\"bogus\"");
    assert!(bogus.contains("\"bogus\""));
    assert_eq!(parse(bogus.as_bytes()), None);
    assert_eq!(
        decide_bytes(Some(bogus.as_bytes()), "s1", "b1", 0),
        Fence::Wait
    );
}

#[test]
fn empty_boot_id_never_passes_a_lease() {
    let f = file(&[("s1", Gate::Lease)], "", HOUR_MS);
    assert_eq!(decide(Some(&f), "s1", "", 10), Fence::Wait);
    assert_eq!(decide_bytes(Some(&json(&f)), "s1", "", 10), Fence::Wait);
    let free = file(&[("s1", Gate::Free)], "", HOUR_MS);
    assert_eq!(decide(Some(&free), "s1", "", 10), Fence::Pass);
}

#[test]
fn oversized_file_waits() {
    let mut f = file(&[("s1", Gate::Free)], "b1", 0);
    f.boot_id = "x".repeat(MAX_FENCE_BYTES);
    let bytes = json(&f);
    assert!(bytes.len() > MAX_FENCE_BYTES);
    assert_eq!(parse(&bytes), None);
    assert_eq!(decide_bytes(Some(&bytes), "s1", "b1", 0), Fence::Wait);
}

#[test]
fn version_zero_waits() {
    let mut f = file(&[("s1", Gate::Free)], "b1", 0);
    f.v = 0;
    let bytes = json(&f);
    assert_eq!(parse(&bytes), None);
    assert_eq!(decide_bytes(Some(&bytes), "s1", "b1", 0), Fence::Wait);
}

#[test]
fn newer_version_waits() {
    let mut f = file(&[("s1", Gate::Free)], "b1", 0);
    f.v = FENCE_VERSION + 1;
    let bytes = json(&f);
    assert_eq!(parse(&bytes), None);
    assert_eq!(decide_bytes(Some(&bytes), "s1", "b1", 0), Fence::Wait);
}

#[test]
fn bytes_round_trip_and_decide() {
    let f = file(
        &[
            ("s1", Gate::Lease),
            ("s2", Gate::Free),
            (
                "s3",
                Gate::Moved {
                    to: "m1".to_string(),
                },
            ),
            (
                "s4",
                Gate::Waiting {
                    for_node: "m5".to_string(),
                },
            ),
        ],
        "b1",
        500,
    );
    let bytes = json(&f);
    assert_eq!(parse(&bytes), Some(f));
    assert_eq!(decide_bytes(Some(&bytes), "s1", "b1", 100), Fence::Pass);
    assert_eq!(decide_bytes(Some(&bytes), "s1", "b1", 500), Fence::Wait);
}

#[test]
fn gates_use_the_documented_wire_shape() {
    let lease: Gate = serde_json::from_str(r#"{"kind":"lease"}"#).expect("lease");
    assert_eq!(lease, Gate::Lease);
    let moved: Gate = serde_json::from_str(r#"{"kind":"moved","to":"m1"}"#).expect("moved");
    assert_eq!(
        moved,
        Gate::Moved {
            to: "m1".to_string()
        }
    );
    let waiting: Gate =
        serde_json::from_str(r#"{"kind":"waiting","for_node":"m5"}"#).expect("waiting");
    assert_eq!(
        waiting,
        Gate::Waiting {
            for_node: "m5".to_string()
        }
    );
    assert_eq!(
        serde_json::to_string(&Gate::Free).expect("free"),
        r#"{"kind":"free"}"#
    );
}
