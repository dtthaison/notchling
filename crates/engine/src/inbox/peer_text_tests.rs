use super::peer_text::{clean_note, clean_place};

const TOKEN: &str = "ghp_abcdefghijklmnopqrstuvwxyz0123";

#[test]
fn a_note_loses_a_token_however_it_is_wrapped_or_split() {
    for text in [
        format!("use {TOKEN} now"),
        format!("use ({TOKEN}), now"),
        format!("use token:{TOKEN} now"),
        "use ghp_abcdefghij\tklmnopqrstuvwxyz0123 now".to_string(),
        "use ghp_abcdefghij\nklmnopqrstuvwxyz0123 now".to_string(),
        "use ghp_abcdefghij\u{200b}klmnopqrstuvwxyz0123 now".to_string(),
        "use ghp_abcdefghij\u{202e}klmnopqrstuvwxyz0123 now".to_string(),
    ] {
        let got = clean_note(&text, 300).unwrap();
        assert!(!got.contains("abcdefghij"), "{text:?} -> {got}");
        assert!(!got.contains("klmnop"), "{text:?} -> {got}");
        assert!(got.starts_with("use "), "{got}");
    }
}

#[test]
fn a_note_loses_every_word_that_names_a_path() {
    for word in [
        "path=/etc/passwd",
        "file:///etc/hosts",
        "../../x",
        "C:/Windows",
        "(~/x)",
        "~/x",
        "/etc",
        "a\\b",
        "src/lib/x.rs",
    ] {
        let got = clean_note(&format!("see {word} ok"), 300).unwrap();
        assert_eq!(got, "see … ok", "{word}");
    }
}

#[test]
fn a_plain_note_passes_with_its_whitespace_normalized_and_is_cut() {
    assert_eq!(
        clean_note("use  the\u{a0}staging   one.", 300),
        Some("use the staging one.".to_string())
    );
    assert_eq!(clean_note("/a/b", 300), None);
    assert_eq!(clean_note("  \u{200b} ", 300), None);
    let long = "word ".repeat(200);
    assert_eq!(clean_note(&long, 300).unwrap().chars().count(), 300);
}

#[test]
fn a_place_label_keeps_a_name_and_one_branch_slash() {
    assert_eq!(
        clean_place("fix/inbox-1", 200, true, false),
        Some("fix/inbox-1".to_string())
    );
    assert_eq!(
        clean_place("my  repo #2", 200, false, false),
        Some("my repo #2".to_string())
    );
    let id = "0123abcd-4567-89ab-cdef-0123456789ab";
    assert_eq!(clean_place(id, 200, false, true), Some(id.to_string()));
    assert_eq!(clean_place(id, 200, false, false), None);
}

#[test]
fn a_place_label_is_dropped_whole_when_anything_in_it_is_risky() {
    for text in [
        TOKEN,
        "branch-ghp_abcdefghijklmnopqrstuvwxyz0123",
        "path=/etc/passwd",
        "file:///etc/hosts",
        "../../.ssh",
        "C:/Windows",
        "(~/x)",
        "~/x",
        "/etc",
        "a\\b",
        "a/b/c",
        "see (x)",
        "ghp_abcdefghij\u{200b}klmnopqrstuvwxyz0123",
        "ghp_abcdefghij\tklmnopqrstuvwxyz0123",
        "fix /etc",
        "ghp_abcdefghijklmn:opqrstuvwxyz0123",
        "ghp_abcdefghijklm\u{43d}opqrstuvwxyz0123",
        "ghp_abcdefghijklmn\u{34f}opqrstuvwxyz0123",
        "D\u{1ef1} \u{e1}n",
        "\u{43f}ath",
    ] {
        assert_eq!(clean_place(text, 200, true, false), None, "{text:?}");
    }
    assert_eq!(clean_place("a/b", 200, false, false), None);
}

#[test]
fn a_note_loses_a_long_word_whatever_it_holds_and_a_tilde_or_slash_after_punctuation() {
    for (text, want) in [
        (
            "use ghp_abcdefghijkln\u{34f}opqrstuvwxyz0123 now",
            "use … now",
        ),
        ("use ghp_abcdefghijklmn:opqrstuvwxyz0123 now", "use … now"),
        ("see (~alice) ok", "see … ok"),
        ("see \"~/ ok", "see … ok"),
        ("see (/etc ok", "see … ok"),
        ("see [/etc] ok", "see … ok"),
        ("take and/or leave", "take … leave"),
    ] {
        assert_eq!(clean_note(text, 300).as_deref(), Some(want), "{text:?}");
    }
}
