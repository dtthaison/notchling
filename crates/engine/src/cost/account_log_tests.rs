use super::account_log::*;
use crate::sessions::testutil::TempDir;

const NOW: i64 = 1_800_000_000_000;
const AT: f64 = 821_692_800.0;
const UNIX: f64 = 1_800_000_000.0;

fn written(plan: Plan) -> String {
    match plan {
        Plan::Write(text) => text,
        other => panic!("expected a write, got {other:?}"),
    }
}

#[test]
fn a_missing_or_blank_file_starts_a_list_in_the_swift_shape() {
    let want = r#"[{"at":821692800.0,"id":"a1:01"}]"#;
    assert_eq!(written(plan_record(None, "a1:01", NOW)), want);
    assert_eq!(written(plan_record(Some(b" \n"), "a1:01", NOW)), want);
    assert_eq!(written(plan_record(Some(b"[]"), "a1:01", NOW)), want);
}

#[test]
fn an_entry_goes_after_the_old_bytes_without_rewriting_them() {
    let old =
        "[{\"at\":814681048,\"id\":\"a3:03\"},\n {\"at\":815978594.1160798,\"id\":\"a4:04\"}\n]\n";
    let text = written(plan_record(Some(old.as_bytes()), "a1:01", NOW));
    assert_eq!(
        text,
        "[{\"at\":814681048,\"id\":\"a3:03\"},\n {\"at\":815978594.1160798,\"id\":\"a4:04\"},{\"at\":821692800.0,\"id\":\"a1:01\"}]"
    );
    let entries = parse(text.as_bytes());
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[2].at, UNIX);
}

#[test]
fn the_account_already_last_adds_nothing_even_when_an_older_entry_names_it() {
    let log = br#"[{"at":1,"id":"a1:01"},{"at":2,"id":"a2:02"}]"#;
    assert_eq!(plan_record(Some(log), "a2:02", NOW), Plan::Unchanged);
    assert!(matches!(
        plan_record(Some(log), "a1:01", NOW),
        Plan::Write(_)
    ));
}

#[test]
fn the_last_entry_is_the_latest_in_time_not_in_the_file() {
    let log = br#"[{"at":9,"id":"a2:02"},{"at":1,"id":"a1:01"}]"#;
    assert_eq!(plan_record(Some(log), "a2:02", NOW), Plan::Unchanged);
}

#[test]
fn anything_but_a_list_is_unreadable_and_never_planned_over() {
    for bad in [&b"{\"at\":1}"[..], b"not json", b"[{\"at\":1,", b"\xff\xfe"] {
        assert_eq!(plan_record(Some(bad), "a1:01", NOW), Plan::Unreadable);
    }
}

#[test]
fn an_id_with_quotes_stays_one_json_string() {
    let text = written(plan_record(None, "a\"b\\c", NOW));
    assert_eq!(parse(text.as_bytes())[0].id, "a\"b\\c");
}

#[test]
fn the_time_is_seconds_since_2001() {
    let text = written(plan_record(None, "a1:01", NOW + 500));
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value[0]["at"].as_f64(), Some(AT + 0.5));
}

#[test]
fn record_writes_privately_appends_once_and_refuses_a_link() {
    let tmp = TempDir::new("acct-log-record");
    let file = tmp.path("new-folder/account-log.json");
    assert!(record(&file, "a1:01", NOW).unwrap());
    assert!(!record(&file, "a1:01", NOW + 5).unwrap());
    assert!(record(&file, "a2:02", NOW + 10_000).unwrap());
    let entries = load(&file);
    let seen: Vec<_> = entries.iter().map(|e| (e.id.as_str(), e.at)).collect();
    assert_eq!(seen, [("a1:01", UNIX), ("a2:02", UNIX + 10.0)]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0);
        let target = tmp.path("elsewhere.json");
        std::fs::write(&target, "[]").unwrap();
        let link = tmp.path("link/account-log.json");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(record(&link, "a1:01", NOW).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "[]");
    }
}

#[test]
fn record_leaves_an_unreadable_file_as_it_is() {
    let tmp = TempDir::new("acct-log-bad");
    let file = tmp.write("account-log.json", "{broken");
    assert!(record(&file, "a1:01", NOW).is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "{broken");
}
