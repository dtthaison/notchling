use super::*;

fn scanned(list: &[&str]) -> Scanned {
    let raw: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    scan(&raw, USAGE, &[]).unwrap()
}

#[test]
fn a_download_asks_embed_request_with_the_model() {
    let (op, body) = request(&scanned(&[
        "granite-97m-r2@835ad14/p1",
        "--reason",
        "search",
        "--key",
        "k",
    ]))
    .unwrap();
    assert_eq!(op, ops::EMBED_REQUEST);
    assert_eq!(
        body,
        json!({"model": "granite-97m-r2@835ad14/p1", "consent": {"reason": "search", "key": "k"}})
    );
    let (_, dry) = request(&scanned(&["m", "--dry-run"])).unwrap();
    assert_eq!(dry["dryRun"], true);
}

#[test]
fn a_download_needs_exactly_one_model() {
    assert!(request(&scanned(&[])).is_err());
    assert!(request(&scanned(&["a", "b"])).is_err());
}

#[test]
fn the_done_line_names_the_model() {
    assert_eq!(
        done_line(&json!({"result": {"model": "m1"}})),
        "Done: m1 is downloading (the user said yes). It finishes in the background."
    );
}
