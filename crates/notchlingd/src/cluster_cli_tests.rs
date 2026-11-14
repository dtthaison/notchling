use super::{parse, read_token, run_with, status_lines, Args, Sub, TOKEN_ON_STDIN, TROUBLE};
use serde_json::json;
use std::io::Cursor;
use std::path::PathBuf;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|w| w.to_string()).collect()
}

#[test]
fn cli_join_reads_token_from_stdin_only() {
    for argv in [
        &["join", "nlj1.abc"][..],
        &["join", "--token", "nlj1.abc"],
        &["join", "--token=nlj1.abc"],
        &["join"],
        &["join", "--token-stdin", "nlj1.abc"],
    ] {
        assert_eq!(parse(&words(argv)).unwrap_err(), TOKEN_ON_STDIN, "{argv:?}");
    }
    assert_eq!(
        parse(&words(&["join", "--token-stdin", "--json"])).unwrap(),
        Args {
            sub: Sub::Join,
            json: true,
            socket: None
        }
    );
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut input = Cursor::new(b"".to_vec());
    let code = run_with(
        &words(&["join", "nlj1.secret"]),
        &mut input,
        &mut out,
        &mut err,
    );
    assert_eq!(code, TROUBLE);
    assert!(out.is_empty());
    let said = String::from_utf8(err).unwrap();
    assert!(said.contains("pass the token on stdin"), "{said}");
    assert!(!said.contains("nlj1.secret"), "{said}");
}

#[test]
fn the_other_commands_parse() {
    assert_eq!(parse(&words(&["status"])).unwrap().sub, Sub::Status);
    assert_eq!(parse(&words(&["token"])).unwrap().sub, Sub::Token);
    let with_socket = parse(&words(&["status", "--json", "--socket", "/tmp/s"])).unwrap();
    assert!(with_socket.json);
    assert_eq!(with_socket.socket, Some(PathBuf::from("/tmp/s")));
    assert!(parse(&words(&["status", "--token-stdin"])).is_err());
    assert!(parse(&words(&[])).is_err());
    assert!(parse(&words(&["leave"])).is_err());
}

#[test]
fn the_token_is_the_first_line_of_stdin() {
    let mut input = Cursor::new(b"  nlj1.abc  \nmore\n".to_vec());
    assert_eq!(read_token(&mut input).unwrap(), "nlj1.abc");
    let mut empty = Cursor::new(b"\n".to_vec());
    assert!(read_token(&mut empty).is_err());
    let mut long = Cursor::new(vec![b'a'; 5000]);
    assert_eq!(read_token(&mut long).unwrap_err(), "the token is too long");
}

#[test]
fn status_reads_as_lines() {
    let status = json!({
        "cluster": {"id": "c1", "name": "Studio", "term": 3, "home": "aaaaaaaaaaaaaaaa"},
        "me": "aaaaaaaaaaaaaaaa",
        "members": [
            {"id": "aaaaaaaaaaaaaaaa", "name": "m5", "role": "home", "state": "active", "online": true},
            {"id": "bbbbbbbbbbbbbbbb", "name": "box", "role": "worker", "state": "pending", "online": false},
        ],
    });
    assert_eq!(
        status_lines(&status),
        vec![
            "Cluster Studio (c1), term 3",
            "This node: aaaaaaaaaaaaaaaa",
            "  m5  home  active  online  aaaaaaaaaaaaaaaa",
            "  box  worker  pending  offline  bbbbbbbbbbbbbbbb",
        ]
    );
    let alone =
        json!({"cluster": null, "me": "aaaaaaaaaaaaaaaa", "members": [], "error": "no identity"});
    assert_eq!(
        status_lines(&alone),
        vec![
            "Not in a cluster.",
            "This node: aaaaaaaaaaaaaaaa",
            "Trouble: no identity"
        ]
    );
}
