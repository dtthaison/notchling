use super::*;

#[derive(Default)]
struct Fake {
    present: bool,
    opened: Vec<String>,
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
        self.opened.push(url.to_owned());
    }

    fn copy(&mut self, text: &str) {
        self.copied.push(text.to_owned());
    }
}

fn encoded(text: &str) -> String {
    link(text).strip_prefix(BASE).unwrap().to_owned()
}

#[test]
fn the_link_opens_the_home_screen_with_the_text_as_the_prompt() {
    assert_eq!(link("hi"), "antigravity://open?prompt=hi");
    assert!(!link("hi").contains("project"));
}

#[test]
fn unreserved_characters_stay_and_everything_else_is_encoded() {
    assert_eq!(encoded("AZaz09-._~"), "AZaz09-._~");
    assert_eq!(encoded("fix the bug"), "fix%20the%20bug");
    assert_eq!(encoded("a&b=c?d"), "a%26b%3Dc%3Fd");
    assert_eq!(encoded("#1 /tmp/x"), "%231%20%2Ftmp%2Fx");
    assert_eq!(encoded("100% + more"), "100%25%20%2B%20more");
    assert_eq!(encoded("it's (\"q\")"), "it%27s%20%28%22q%22%29");
}

#[test]
fn newlines_tabs_and_carriage_returns_are_encoded() {
    assert_eq!(encoded("one\ntwo\r\nthree\tx"), "one%0Atwo%0D%0Athree%09x");
    assert_eq!(encoded("a\n\nb"), "a%0A%0Ab");
}

#[test]
fn emoji_and_non_ascii_become_their_utf8_bytes() {
    assert_eq!(encoded("caf\u{e9}"), "caf%C3%A9");
    assert_eq!(encoded("\u{1F600}"), "%F0%9F%98%80");
    assert_eq!(
        encoded("Xin ch\u{e0}o \u{1F44B}"),
        "Xin%20ch%C3%A0o%20%F0%9F%91%8B"
    );
    assert_eq!(encoded("\u{65e5}\u{672c}"), "%E6%97%A5%E6%9C%AC");
}

#[test]
fn nothing_but_unreserved_characters_and_escapes_is_left_in_the_link() {
    let all: String = (0u32..0x250).filter_map(char::from_u32).collect();
    let link = encoded(&all);
    assert!(link
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"-._~%".contains(&b)));
}

#[test]
fn an_empty_text_makes_an_empty_prompt() {
    assert_eq!(link(""), BASE);
}

#[test]
fn the_text_goes_to_the_opener_once_and_nowhere_else() {
    let mut fake = Fake {
        present: true,
        ..Fake::default()
    };
    assert_eq!(hand_over(&mut fake, "go & fix\nit"), Ok(()));
    assert!(fake.copied.is_empty());
    assert_eq!(
        fake.opened,
        ["antigravity://open?prompt=go%20%26%20fix%0Ait"]
    );
}

#[test]
fn without_the_app_nothing_is_opened() {
    let mut fake = Fake::default();
    assert_eq!(hand_over(&mut fake, "hello"), Err(NotInstalled));
    assert!(fake.opened.is_empty());
}

#[test]
fn the_app_is_found_in_the_system_or_the_users_applications_folder() {
    let root = std::env::temp_dir().join(format!("nl-agydraft-{}", std::process::id()));
    let system = root.join("system");
    let home = root.join("home");
    std::fs::create_dir_all(system.join("Other.app")).unwrap();
    std::fs::create_dir_all(home.join("Applications")).unwrap();
    let folders = vec![system.clone(), home.join("Applications")];
    assert!(!installed_in(&folders));
    std::fs::create_dir_all(home.join("Applications").join(APP)).unwrap();
    assert!(installed_in(&folders));
    assert!(!installed_in(&[system]));
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(
        app_folders(Some(Path::new("/Users/x"))),
        [
            PathBuf::from("/Applications"),
            PathBuf::from("/Users/x/Applications")
        ]
    );
    assert_eq!(app_folders(None), [PathBuf::from("/Applications")]);
}
