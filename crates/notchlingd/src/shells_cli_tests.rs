use super::*;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
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
