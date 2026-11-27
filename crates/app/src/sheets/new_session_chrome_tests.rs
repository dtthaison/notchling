use super::new_session::{accounts, NewSession};
use serde_json::json;

fn sheet() -> NewSession {
    NewSession::new(
        accounts(&[], &[], &[]),
        vec!["/h/work/api".to_owned()],
        "/h",
    )
}

#[test]
fn chrome_is_off_and_sends_no_flag_unless_asked() {
    let mut sheet = sheet();
    assert!(sheet.chrome_offered());
    assert!(!sheet.chrome);
    let request = sheet.start().expect("a request");
    assert_eq!(request.args, json!({"folder": "/h/work/api"}));
}

#[test]
fn the_switch_in_the_sheet_sends_chrome() {
    let mut sheet = sheet();
    sheet.toggle_chrome();
    let request = sheet.start().expect("a request");
    assert_eq!(
        request.args,
        json!({"folder": "/h/work/api", "flags": ["--chrome"]})
    );
}

#[test]
fn the_setting_makes_the_sheet_start_with_chrome_on() {
    let mut sheet = sheet().with_chrome(true);
    assert!(sheet.chrome);
    let request = sheet.start().expect("a request");
    assert_eq!(
        request.args,
        json!({"folder": "/h/work/api", "flags": ["--chrome"]})
    );
}

#[test]
fn turning_it_off_against_the_setting_sends_no_chrome() {
    let mut sheet = sheet().with_chrome(true);
    sheet.toggle_chrome();
    assert!(!sheet.chrome);
    let request = sheet.start().expect("a request");
    assert_eq!(
        request.args,
        json!({"folder": "/h/work/api", "flags": ["--no-chrome"]})
    );
}

#[test]
fn chrome_is_not_offered_where_the_session_is_not_claude_code_here() {
    let none = NewSession::new(Vec::new(), vec!["/h/work".to_owned()], "/h");
    assert!(!none.chrome_offered());
}
