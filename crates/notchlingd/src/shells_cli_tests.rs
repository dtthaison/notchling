use super::*;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_stop_that_left_writers_running_exits_1_with_some_left() {
    let reply = json!({"id": "bq1", "stopped": false, "state": "running", "left": [801], "reason": "some_left"});
    let failure = stop_reply("bq1", &reply).unwrap_err();
    assert_eq!(failure.exit, 1);
    assert_eq!(failure.reason, "some_left");
    let value = failure.to_value_with(reply);
    assert_eq!(value["error"]["reason"], "some_left");
    assert_eq!(value["result"]["left"], json!([801]));
    assert_eq!(
        stop_reply("bq1", &json!({"stopped": true})).unwrap(),
        "Stopped bq1."
    );
    assert_eq!(
        stop_reply("bq1", &json!({"stopped": false, "state": "ended"})).unwrap(),
        "bq1 had already ended."
    );
}

#[test]
fn the_shells_command_lists_or_stops_one() {
    assert_eq!(
        parse(&words(&["--json"])).unwrap(),
        Args {
            sub: Sub::List,
            json: true,
            socket: None
        }
    );
    assert_eq!(
        parse(&words(&["stop", "bq1", "--socket", "/tmp/s"])).unwrap(),
        Args {
            sub: Sub::Stop("bq1".into()),
            json: false,
            socket: Some(PathBuf::from("/tmp/s"))
        }
    );
    assert_eq!(
        parse(&words(&["--stuck"])).unwrap(),
        Args {
            sub: Sub::Stuck,
            json: false,
            socket: None
        }
    );
    assert!(parse(&words(&["stop", "a", "--stuck"])).is_err());
    assert!(parse(&words(&["stop"])).is_err());
    assert!(parse(&words(&["stop", "a", "b"])).is_err());
    assert!(parse(&words(&["kill", "a"])).is_err());
    assert!(parse(&words(&["--wat"])).is_err());
}

#[test]
fn a_shell_prints_as_one_line() {
    let row = json!({"id": "b1", "status": "running", "runningSecs": 2820, "stuck": true,
        "command": "while pgrep -f x; do sleep 5; done"});
    assert_eq!(
        line(&row),
        "b1  running  47m  stuck  while pgrep -f x; do sleep 5; done"
    );
    let ended = json!({"id": "b2", "status": "stopped", "runningSecs": null, "stuck": false, "command": "sleep 1"});
    assert_eq!(line(&ended), "b2  stopped  -  -  sleep 1");
    assert_eq!(age(5), "5s");
    assert_eq!(age(3 * 3600 + 120), "3h02m");
}
