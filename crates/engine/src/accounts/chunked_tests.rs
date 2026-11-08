//! Split values against the in-memory Keychains: realistic sizes, the Swift one-item format, a
//! damaged part, and parts left over from a longer value.

use super::*;
use crate::accounts::fake::MemoryKeychain;
use crate::accounts::generic_password::FakeKeychain;
use crate::accounts::keychain::Keys;
use std::sync::Arc;

const CODEX: &str = "Notchling Codex";

/// An obviously fake value of `len` bytes that looks like a base64 login copy.
fn fake_value(len: usize) -> String {
    let pattern = "FAKE-not-a-token-0123456789abcdefABCDEF+/";
    pattern.chars().cycle().take(len).collect()
}

#[test]
fn a_long_value_is_split_and_read_back_whole() {
    for len in [4_000, 5_500, 8_192] {
        let kc = FakeKeychain::new();
        let value = fake_value(len);
        write(&Plain(&kc), CODEX, "u:w", &value).unwrap();
        let header = kc.get(CODEX, "u:w").unwrap();
        assert!(header.starts_with(HEADER), "{header}");
        let n = len.div_ceil(PART_BYTES);
        assert!(header.starts_with(&format!("{HEADER}{n}:")));
        for i in 0..n {
            assert!(kc.get(CODEX, &part_name("u:w", i)).is_some());
        }
        assert_eq!(kc.get(CODEX, &part_name("u:w", n)), None);
        assert_eq!(
            read(&Plain(&kc), CODEX, "u:w").unwrap().as_deref(),
            Some(value.as_str())
        );
    }
}

#[test]
fn every_part_is_written_before_the_header() {
    let kc = FakeKeychain::new();
    write(&Plain(&kc), CODEX, "u:w", &fake_value(5_000)).unwrap();
    let writes: Vec<String> = kc
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("write"))
        .collect();
    assert_eq!(
        writes.last().map(String::as_str),
        Some("write Notchling Codex/u:w")
    );
    assert_eq!(writes.len(), 6);
}

#[test]
fn a_short_value_stays_one_item_the_swift_way() {
    let kc = FakeKeychain::new();
    write(&Plain(&kc), CODEX, "u:w", "short").unwrap();
    assert_eq!(kc.get(CODEX, "u:w").as_deref(), Some("short"));
    assert_eq!(kc.get(CODEX, &part_name("u:w", 0)), None);
}

#[test]
fn a_one_item_value_the_swift_app_wrote_is_read_as_is() {
    let swift = fake_value(3_000);
    let kc = FakeKeychain::new().with(CODEX, "u:w", &swift);
    assert_eq!(
        read(&Plain(&kc), CODEX, "u:w").unwrap().as_deref(),
        Some(swift.as_str())
    );
}

#[test]
fn a_damaged_or_missing_part_is_an_error_never_a_wrong_value() {
    let kc = FakeKeychain::new();
    write(&Plain(&kc), CODEX, "u:w", &fake_value(5_000)).unwrap();
    let part = kc.get(CODEX, &part_name("u:w", 1)).unwrap();
    let changed = format!("QUFB{}", &part[4..]);
    kc.write(CODEX, &part_name("u:w", 1), &changed).unwrap();
    assert_eq!(read(&Plain(&kc), CODEX, "u:w"), Err(KeychainError::Damaged));
    kc.delete(CODEX, &part_name("u:w", 1));
    assert_eq!(read(&Plain(&kc), CODEX, "u:w"), Err(KeychainError::Damaged));
}

#[test]
fn a_shorter_value_removes_the_parts_a_longer_one_left() {
    let kc = FakeKeychain::new();
    write(&Plain(&kc), CODEX, "u:w", &fake_value(8_000)).unwrap();
    write(&Plain(&kc), CODEX, "u:w", &fake_value(3_000)).unwrap();
    assert!(kc.get(CODEX, &part_name("u:w", 2)).is_some());
    for i in 3..8 {
        assert_eq!(kc.get(CODEX, &part_name("u:w", i)), None);
    }
    write(&Plain(&kc), CODEX, "u:w", "short").unwrap();
    assert_eq!(kc.get(CODEX, &part_name("u:w", 0)), None);
    assert_eq!(
        read(&Plain(&kc), CODEX, "u:w").unwrap().as_deref(),
        Some("short")
    );
}

#[test]
fn delete_removes_the_header_and_every_part() {
    let kc = FakeKeychain::new();
    write(&Plain(&kc), CODEX, "u:w", &fake_value(6_000)).unwrap();
    delete(&Plain(&kc), CODEX, "u:w").unwrap();
    assert_eq!(kc.get(CODEX, "u:w"), None);
    for i in 0..8 {
        assert_eq!(kc.get(CODEX, &part_name("u:w", i)), None);
    }
}

#[test]
fn items_other_tools_read_are_never_split() {
    let kc = FakeKeychain::new();
    for (service, account) in [
        ("gemini", "antigravity"),
        ("Claude Code-credentials", "me"),
        ("Claude Code-credentials-0123abcd", "me"),
    ] {
        assert_eq!(
            write(&Plain(&kc), service, account, &fake_value(5_000)),
            Err(KeychainError::TooLong)
        );
        assert_eq!(kc.get(service, account), None);
        assert_eq!(kc.get(service, &part_name(account, 0)), None);
    }
    // A header-looking value in another tool's item is that tool's text, not parts to join.
    let odd = format!("{HEADER}1:{}", "0".repeat(64));
    let kc = FakeKeychain::new().with("gemini", "antigravity", &odd);
    assert_eq!(
        read(&Plain(&kc), "gemini", "antigravity").unwrap(),
        Some(odd)
    );
}

#[test]
fn every_line_written_stays_under_the_cap() {
    let chain = Arc::new(MemoryKeychain::new());
    let keys = Keys {
        runner: chain.clone(),
        user: "me".into(),
    };
    let value = format!("{{\"blob\":\"{}\"}}", fake_value(8_000));
    keys.write("Notchling Accounts", "a1:o1", &value).unwrap();
    assert_eq!(
        keys.read_text("Notchling Accounts", "a1:o1")
            .unwrap()
            .map(|s| s.expose().to_string()),
        Some(value.clone())
    );
    assert!(chain
        .calls()
        .iter()
        .all(|args| !args.join(" ").contains("FAKE")));
    keys.delete("Notchling Accounts", "a1:o1").unwrap();
    assert!(chain.names().is_empty());
    // Claude Code's own item keeps refusing.
    assert_eq!(
        keys.write("Claude Code-credentials", "me", &value),
        Err(KeychainError::TooLong)
    );
}

#[test]
fn the_own_services_are_the_stores_saved_copies() {
    assert!(is_own(crate::accounts::codex::store::SERVICE));
    assert!(is_own(crate::accounts::antigravity::store::SERVICE));
    assert!(is_own(crate::accounts::claude::SAVED_SERVICE));
    assert!(!is_own(crate::accounts::claude::LIVE_SERVICE));
    assert!(!is_own(crate::accounts::antigravity::store::LIVE_SERVICE));
    assert!(!is_own(crate::vault::store::SERVICE));
}
