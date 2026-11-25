use super::*;

const UUID: &str = "0a1b2c3d-1111-4222-8333-444455556666";
const ID: &str = "f00dfeed-aaaa-4bbb-8ccc-ddddeeeeffff";
const EMAIL: &str = "jo.doe+test@example.com";

#[derive(Default)]
struct Fake {
    opened: Vec<String>,
    paneled: Vec<String>,
    copied: Vec<String>,
}

impl Opener for Fake {
    fn installed(&self) -> bool {
        false
    }

    fn open(&mut self, url: &str) {
        self.opened.push(url.to_owned());
    }

    fn panel(&mut self, url: &str) {
        self.paneled.push(url.to_owned());
    }

    fn copy(&mut self, text: &str) {
        self.copied.push(text.to_owned());
    }
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-agyremote-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_link_with_an_account_and_a_conversation_encodes_the_target_twice() {
    assert_eq!(
        link(UUID, Some(EMAIL), Some(ID)),
        "https://accounts.google.com/AccountChooser?Email=jo.doe%2Btest%40example.com&continue=https%3A%2F%2Fantigravity.google.com%2Fr%2F0a1b2c3d-1111-4222-8333-444455556666-v2%3Fp%3Dc%252Ff00dfeed-aaaa-4bbb-8ccc-ddddeeeeffff"
    );
}

#[test]
fn the_conversation_is_encoded_once_in_the_page_and_again_inside_continue() {
    assert_eq!(
        page(UUID, Some(ID)),
        "https://antigravity.google.com/r/0a1b2c3d-1111-4222-8333-444455556666-v2?p=c%2Ff00dfeed-aaaa-4bbb-8ccc-ddddeeeeffff"
    );
    assert!(link(UUID, Some(EMAIL), Some(ID)).contains("%3Fp%3Dc%252F"));
}

#[test]
fn without_a_conversation_the_device_page_opens() {
    assert_eq!(
        page(UUID, None),
        "https://antigravity.google.com/r/0a1b2c3d-1111-4222-8333-444455556666-v2"
    );
    assert_eq!(
        link(UUID, Some(EMAIL), None),
        "https://accounts.google.com/AccountChooser?Email=jo.doe%2Btest%40example.com&continue=https%3A%2F%2Fantigravity.google.com%2Fr%2F0a1b2c3d-1111-4222-8333-444455556666-v2"
    );
}

#[test]
fn without_an_email_the_page_opens_directly() {
    assert_eq!(link(UUID, None, Some(ID)), page(UUID, Some(ID)));
    assert_eq!(link(UUID, Some("  "), Some(ID)), page(UUID, Some(ID)));
}

#[test]
fn special_characters_in_the_email_and_the_conversation_are_encoded() {
    let url = link(UUID, Some("a b&c#d@example.com"), Some("x y/z&1"));
    assert!(url.contains("Email=a%20b%26c%23d%40example.com&"));
    assert!(url.contains("%3Fp%3Dc%252Fx%2520y%252Fz%25261"));
    assert_eq!(url.matches("&continue=").count(), 1);
}

#[test]
fn only_the_installation_uuid_field_is_read() {
    let state =
        "name: \"x\"\ninstallation_uuid: \"0a1b2c3d-1111-4222-8333-444455556666\"\nother: 1\n";
    assert_eq!(installation_uuid(state).as_deref(), Some(UUID));
    let nested = "settings {\n  installation_uuid: \"0a1b2c3d-1111-4222-8333-444455556666\"\n}\n";
    assert_eq!(installation_uuid(nested).as_deref(), Some(UUID));
}

#[test]
fn a_missing_or_malformed_field_reads_as_nothing() {
    assert_eq!(installation_uuid(""), None);
    assert_eq!(installation_uuid("other: \"x\"\n"), None);
    assert_eq!(installation_uuid("installation_uuid: \"nope\"\n"), None);
    assert_eq!(installation_uuid("installation_uuid: 5\n"), None);
    assert_eq!(
        installation_uuid("installation_uuid: \"0a1b2c3d-1111-4222-8333-44445555666\"\n"),
        None
    );
    assert_eq!(
        installation_uuid("installation_uuid: \"0a1b2c3d-1111-4222-8333-44445555666z\"\n"),
        None
    );
    assert_eq!(
        installation_uuid("x_installation_uuid: \"0a1b2c3d-1111-4222-8333-444455556666\"\n"),
        None
    );
}

#[test]
fn the_state_file_is_read_from_the_given_root_only() {
    let root = scratch("read");
    assert_eq!(read_installation(&root), None);
    std::fs::write(
        root.join(STATE_FILE),
        "installation_uuid: \"0a1b2c3d-1111-4222-8333-444455556666\"\n",
    )
    .unwrap();
    assert_eq!(read_installation(&root).as_deref(), Some(UUID));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_session_row_names_its_conversation_and_a_draft_names_none() {
    assert_eq!(session_of("agy:abc-1"), Some("abc-1"));
    assert_eq!(session_of("antigravity:abc-1"), Some("abc-1"));
    assert_eq!(session_of("agy-new:u1"), None);
    assert_eq!(session_of("agy:"), None);
    assert_eq!(session_of("agy:a/b"), None);
    assert_eq!(session_of("codex:abc"), None);
}

#[test]
fn handing_over_copies_the_text_and_opens_the_link_once() {
    let root = scratch("hand");
    std::fs::write(
        root.join(STATE_FILE),
        "installation_uuid: \"0a1b2c3d-1111-4222-8333-444455556666\"\n",
    )
    .unwrap();
    let mut fake = Fake::default();
    assert_eq!(
        hand_over(
            &mut fake,
            &root,
            Some(EMAIL),
            "agy:f00dfeed-aaaa-4bbb-8ccc-ddddeeeeffff",
            "hello\nthere",
            false
        ),
        Ok(())
    );
    assert_eq!(fake.copied, ["hello\nthere"]);
    assert_eq!(fake.opened, [link(UUID, Some(EMAIL), Some(ID))]);
    assert!(fake.paneled.is_empty());
    let mut inside = Fake::default();
    assert_eq!(
        hand_over(
            &mut inside,
            &root,
            Some(EMAIL),
            "agy:f00dfeed-aaaa-4bbb-8ccc-ddddeeeeffff",
            "hello",
            true
        ),
        Ok(())
    );
    assert_eq!(inside.paneled, [link(UUID, Some(EMAIL), Some(ID))]);
    assert!(inside.opened.is_empty());
    assert_eq!(inside.copied, ["hello"]);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn without_an_installation_nothing_is_copied_or_opened() {
    let root = scratch("none");
    let mut fake = Fake::default();
    assert_eq!(
        hand_over(&mut fake, &root, Some(EMAIL), "agy:abc", "hello", true),
        Err(NoInstallation)
    );
    assert!(fake.opened.is_empty() && fake.copied.is_empty() && fake.paneled.is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}
