use super::*;
use notchling_engine::contract::channel::chosen;
use notchling_engine::contract::channel_remote::STATE_FILE;
use notchling_engine::contract::Tool;

#[derive(Default)]
struct Fake {
    present: bool,
    opened: Vec<String>,
    paneled: Vec<String>,
    copied: Vec<String>,
}

impl Opener for Fake {
    fn installed(&self) -> bool {
        self.present
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

fn state(tag: &str, with_uuid: bool) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-handoff-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    if with_uuid {
        std::fs::write(
            dir.join(STATE_FILE),
            "installation_uuid: \"0a1b2c3d-1111-4222-8333-444455556666\"\n",
        )
        .unwrap();
    }
    dir
}

fn target<'a>(root: &'a Path, email: Option<&'a str>) -> Target<'a> {
    Target {
        in_panel: false,
        remote_root: root,
        email,
        row_id: "agy:f00dfeed-aaaa-4bbb-8ccc-ddddeeeeffff",
    }
}

#[test]
fn the_default_channel_hands_nothing_over() {
    let root = state("acp", true);
    for setting in [None, Some("acp"), Some("junk")] {
        let channel = chosen(Tool::Antigravity, setting);
        let mut fake = Fake {
            present: true,
            ..Fake::default()
        };
        assert!(hand_over(channel, &mut fake, &target(&root, None), "hi").is_none());
        assert!(fake.opened.is_empty() && fake.copied.is_empty());
        assert!(words(channel).is_none());
    }
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_app_channel_opens_the_draft_link_and_copies_nothing() {
    let root = state("app", false);
    let channel = chosen(Tool::Antigravity, Some("app"));
    let mut fake = Fake {
        present: true,
        ..Fake::default()
    };
    let done = hand_over(channel, &mut fake, &target(&root, None), "a b");
    assert_eq!(done, Some(Ok(t::chat::antigravity_draft_note())));
    assert_eq!(fake.opened, ["antigravity://open?prompt=a%20b"]);
    assert!(fake.copied.is_empty());
    assert_eq!(words(channel).unwrap().tip, t::chat::antigravity_send_tip());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_app_channel_says_so_when_the_app_is_missing() {
    let root = state("appmissing", false);
    let channel = chosen(Tool::Antigravity, Some("app"));
    let mut fake = Fake::default();
    let done = hand_over(channel, &mut fake, &target(&root, None), "x");
    assert_eq!(done, Some(Err(t::chat::antigravity_missing())));
    assert!(fake.opened.is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_remote_channel_copies_the_text_and_opens_the_conversation_for_the_account() {
    let root = state("remote", true);
    let channel = chosen(Tool::Antigravity, Some("remote"));
    let mut fake = Fake::default();
    let done = hand_over(
        channel,
        &mut fake,
        &target(&root, Some("jo@example.com")),
        "hello",
    );
    assert_eq!(done, Some(Ok(t::chat::antigravity_remote_note())));
    assert_eq!(fake.copied, ["hello"]);
    assert_eq!(
        fake.opened,
        ["https://accounts.google.com/AccountChooser?Email=jo%40example.com&continue=https%3A%2F%2Fantigravity.google.com%2Fr%2F0a1b2c3d-1111-4222-8333-444455556666-v2%3Fp%3Dc%252Ff00dfeed-aaaa-4bbb-8ccc-ddddeeeeffff"]
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_remote_channel_follows_the_active_account_and_skips_the_chooser_without_one() {
    let root = state("remoteacct", true);
    let channel = chosen(Tool::Antigravity, Some("remote"));
    let mut first = Fake::default();
    hand_over(
        channel,
        &mut first,
        &target(&root, Some("a@example.com")),
        "x",
    );
    let mut second = Fake::default();
    hand_over(
        channel,
        &mut second,
        &target(&root, Some("b@example.com")),
        "x",
    );
    assert!(first.opened[0].contains("Email=a%40example.com"));
    assert!(second.opened[0].contains("Email=b%40example.com"));
    let mut none = Fake::default();
    hand_over(channel, &mut none, &target(&root, None), "x");
    assert!(none.opened[0].starts_with("https://antigravity.google.com/r/"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_remote_channel_without_an_installation_says_to_turn_remote_control_on() {
    let root = state("remotemissing", false);
    let channel = chosen(Tool::Antigravity, Some("remote"));
    let mut fake = Fake::default();
    let done = hand_over(
        channel,
        &mut fake,
        &target(&root, Some("a@example.com")),
        "x",
    );
    assert_eq!(done, Some(Err(t::chat::antigravity_remote_missing())));
    assert!(fake.opened.is_empty() && fake.copied.is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_remote_state_lives_beside_the_cli_folder() {
    assert_eq!(
        remote_root(Path::new("/h/.gemini/antigravity-cli")),
        Path::new("/h/.gemini/antigravity")
    );
}

#[test]
fn the_remote_channel_opens_in_the_panel_or_the_browser_as_asked() {
    let root = state("remotepanel", true);
    let channel = chosen(Tool::Antigravity, Some("remote"));
    let mut inside = Fake::default();
    let mut aimed = target(&root, Some("jo@example.com"));
    aimed.in_panel = true;
    let done = hand_over(channel, &mut inside, &aimed, "hello");
    assert_eq!(done, Some(Ok(t::chat::antigravity_remote_note())));
    assert_eq!(inside.paneled.len(), 1);
    assert!(inside.opened.is_empty());
    assert_eq!(inside.copied, ["hello"]);
    let mut outside = Fake::default();
    aimed.in_panel = false;
    hand_over(channel, &mut outside, &aimed, "hello");
    assert_eq!(outside.opened, inside.paneled);
    assert!(outside.paneled.is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_draft_link_never_goes_to_the_panel() {
    let root = state("apppanel", false);
    let channel = chosen(Tool::Antigravity, Some("app"));
    let mut fake = Fake {
        present: true,
        ..Fake::default()
    };
    let mut aimed = target(&root, None);
    aimed.in_panel = true;
    hand_over(channel, &mut fake, &aimed, "x");
    assert!(fake.paneled.is_empty());
    assert_eq!(fake.opened.len(), 1);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_panel_is_the_default_place_and_browser_is_the_only_other() {
    assert!(in_panel(""));
    assert!(in_panel("panel"));
    assert!(in_panel("junk"));
    assert!(!in_panel("browser"));
}
