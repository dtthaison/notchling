use super::*;
use crate::sessions::testutil::TempDir;

const ID: &str = "7f1c2d3e-4a5b-4c6d-8e9f-0a1b2c3d4e5f";

fn live(pid: u32) -> bool {
    pid == 999
}

#[test]
fn a_live_session_file_with_a_stray_closing_brace_still_names_its_session() {
    let tmp = TempDir::new("takeover-trailing");
    tmp.write(
        "sessions/999.json",
        &format!(r#"{{"pid":999,"sessionId":"{ID}","cwd":"/x"}}}}"#),
    );
    assert!(has_live_session_file(ID, &tmp.path("sessions"), &live));
}

#[test]
fn a_truncated_live_session_file_names_nothing() {
    let tmp = TempDir::new("takeover-torn");
    tmp.write(
        "sessions/999.json",
        &format!(r#"{{"pid":999,"sessionId":"{ID}","cw"#),
    );
    assert!(!has_live_session_file(ID, &tmp.path("sessions"), &live));
}

#[test]
fn a_session_file_with_a_stray_closing_brace_still_maps_its_process_to_the_session() {
    let tmp = TempDir::new("takeover-root-trailing");
    tmp.write(
        "sessions/501.json",
        &format!(r#"{{"pid":501,"sessionId":"{ID}"}}}}"#),
    );
    let parents = |pid: u32| match pid {
        501 => Some(500),
        500 => Some(300),
        _ => None,
    };
    assert_eq!(
        session_id_for_root_pid(300, &tmp.path("sessions"), &parents).as_deref(),
        Some(ID)
    );
    assert_eq!(
        session_id_for_root_pid(777, &tmp.path("sessions"), &parents),
        None
    );
}
