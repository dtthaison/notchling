use super::json_view::{pretty, read, Shown};

#[test]
fn json_is_indented_and_keeps_its_key_order() {
    let got = pretty("{\"b\":1,\"a\":[1,2,{\"c\":null}],\"s\":\"x,y:{z}\"}").unwrap();
    assert_eq!(
        got,
        "{\n  \"b\": 1,\n  \"a\": [\n    1,\n    2,\n    {\n      \"c\": null\n    }\n  ],\n  \"s\": \"x,y:{z}\"\n}"
    );
}

#[test]
fn empty_containers_and_escapes_stay_whole() {
    assert_eq!(
        pretty("{\"a\":{},\"b\":[]}").unwrap(),
        "{\n  \"a\": {},\n  \"b\": []\n}"
    );
    assert_eq!(pretty("\"a\\\"b\"").unwrap(), "\"a\\\"b\"");
    assert_eq!(pretty("  42 ").unwrap(), "42");
}

#[test]
fn invalid_json_says_where() {
    assert_eq!(pretty("{\"a\":"), None);
    match read("{\n  \"a\": ,\n}") {
        Shown::Invalid { line, column } => {
            assert_eq!(line, 2);
            assert!(column > 0);
        }
        Shown::Pretty(_) => panic!("valid?"),
    }
    assert!(matches!(read("[1, 2]"), Shown::Pretty(_)));
}
