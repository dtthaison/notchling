use notchling_engine::machines::ssh_key::KeyIssue;
use notchling_engine::machines::ssh_test::{SshTest, Why};
use notchling_kit::Ink;

use super::ssh_text::{address_test_text, key_note, key_title, test_ink, test_text};

#[test]
fn each_result_has_its_own_words() {
    let cases = [
        (
            SshTest::Connected {
                user: "ubuntu".into(),
            },
            "Connected as ubuntu",
        ),
        (SshTest::Denied, "Wrong user or key (permission denied)"),
        (SshTest::HostKeyChanged, "Host key changed"),
        (
            SshTest::Unreachable(Why::TimedOut),
            "Can't reach: timed out",
        ),
        (
            SshTest::Unreachable(Why::Refused),
            "Can't reach: connection refused",
        ),
        (
            SshTest::Unreachable(Why::NoName),
            "Can't reach: name not found",
        ),
        (SshTest::Unreachable(Why::NoRoute), "Can't reach: no route"),
        (SshTest::Unreachable(Why::Other), "Can't reach: no answer"),
        (
            SshTest::WrongNetwork {
                wanted: "work.example".into(),
                active: "person@example.com".into(),
            },
            "Not on this Tailscale network",
        ),
        (SshTest::Key(KeyIssue::Missing), "The key file isn't there."),
    ];
    for (result, text) in cases {
        assert_eq!(test_text(&result), text);
    }
}

#[test]
fn a_new_host_says_nothing_was_saved() {
    let text = test_text(&SshTest::NewHost {
        user: "ubuntu".into(),
    });
    assert!(text.starts_with("Connected as ubuntu"));
    assert!(text.contains("known_hosts") && text.contains("nothing was saved"));
}

#[test]
fn an_address_line_names_both_networks() {
    let text = address_test_text(&SshTest::WrongNetwork {
        wanted: "work.example".into(),
        active: "person@example.com".into(),
    });
    assert!(text.contains("work.example") && text.contains("person@example.com"));
}

#[test]
fn a_pass_is_green_and_anything_else_needs_a_look() {
    assert_eq!(
        test_ink(&SshTest::Connected { user: "u".into() }),
        Ink::Done
    );
    assert_eq!(test_ink(&SshTest::NewHost { user: "u".into() }), Ink::Done);
    assert_eq!(test_ink(&SshTest::Denied), Ink::Needs);
}

#[test]
fn the_key_button_shows_the_file_name_or_default() {
    assert_eq!(key_title(None), "Key: Default");
    assert_eq!(key_title(Some("~/.ssh/id_work")), "Key: id_work");
    assert_eq!(key_title(Some("/Users/x/keys/ci deploy")), "Key: ci deploy");
}

#[test]
fn a_problem_beats_a_warning_and_a_public_key_warns() {
    let (text, ink) = key_note(Some("~/.ssh/k"), Some(KeyIssue::TooOpen)).unwrap();
    assert!(text.contains("chmod 600"));
    assert_eq!(ink, Ink::Needs);
    let (text, ink) = key_note(Some("~/.ssh/k.pub"), None).unwrap();
    assert!(text.contains(".pub"));
    assert_eq!(ink, Ink::TextMuted);
    assert!(key_note(Some("~/.ssh/k"), None).is_none());
    assert!(key_note(None, None).is_none());
}
