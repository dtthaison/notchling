use super::*;

struct Rig {
    root: PathBuf,
    claude: PathBuf,
    store: PathBuf,
    other: PathBuf,
}

impl Rig {
    fn new(tag: &str, account: Option<&str>, live: &[u8]) -> Rig {
        let name = format!("nl-desktop-land-{tag}-{}", std::process::id());
        let root = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&root);
        let (claude, other) = (root.join("Claude"), root.join("saved.cookies"));
        std::fs::create_dir_all(&claude).unwrap();
        if let Some(id) = account {
            std::fs::write(config(&claude), names(id)).unwrap();
        }
        std::fs::write(cookies(&claude), live).unwrap();
        std::fs::write(&other, b"saved-b").unwrap();
        Rig {
            store: root.join("ours"),
            root,
            claude,
            other,
        }
    }

    fn dirs(&self) -> Dirs<'_> {
        Dirs {
            claude: &self.claude,
            store: &self.store,
        }
    }

    fn number(&self, n: u64) -> PathBuf {
        let name = format!("{n:012}.cookies");
        self.store.join(UNNAMED_DIR).join(name)
    }

    fn fails_at_the_last_step(&self) -> Result<Option<Kept>, Refused> {
        let dirs = self.dirs();
        let ready = ready(&dirs, Some((self.other.as_path(), Some("u-b")))).unwrap();
        std::fs::remove_file(temp(&self.claude, "Cookies", "notchling.tmp")).unwrap();
        ready.land()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn names(id: &str) -> String {
    format!(r#"{{"lastKnownAccountUuid":"{id}"}}"#)
}

#[test]
fn a_change_that_is_refused_writes_no_copy_and_no_backup() {
    for wanted in ["own", "other", "none"] {
        for step in ["moved-on", "not-ready"] {
            let rig = Rig::new(&format!("refused-{wanted}-{step}"), Some("u-a"), b"live");
            let (dirs, own) = (rig.dirs(), saved(&rig.store, "u-a"));
            assert!(save(&own, b"good-a"));
            let to = match wanted {
                "own" => Some((own.as_path(), Some("u-a"))),
                "other" => Some((rig.other.as_path(), Some("u-b"))),
                _ => None,
            };
            let refused = if step == "moved-on" {
                let ready = ready(&dirs, to).unwrap();
                std::fs::write(config(&rig.claude), names("u-b")).unwrap();
                (ready.land().err(), Refused::Unsettled)
            } else {
                let blocked = temp(&rig.claude, "config.json", "tmp");
                std::fs::create_dir_all(blocked.join("inner")).unwrap();
                (change(&dirs, to).err(), Refused::Failed)
            };
            assert_eq!(refused.0, Some(refused.1), "{wanted} {step}");
            assert_eq!(std::fs::read(&own).unwrap(), b"good-a", "{wanted} {step}");
            assert!(!backup(&rig.claude).exists(), "{wanted} {step}");
            assert_eq!(std::fs::read(cookies(&rig.claude)).unwrap(), b"live");
        }
    }
}

#[test]
fn a_change_that_is_refused_or_fails_leaves_no_new_unnamed_copy() {
    let rig = Rig::new("unnamed-refused", None, b"u1");
    let dirs = rig.dirs();
    let ready = ready(&dirs, Some((rig.other.as_path(), Some("u-b")))).unwrap();
    std::fs::write(cookies(&rig.claude), b"u2-longer").unwrap();
    assert_eq!(ready.land(), Err(Refused::Unsettled));
    assert!(!rig.number(1).exists() && !backup(&rig.claude).exists());

    let rig = Rig::new("unnamed-failed", None, b"u1");
    assert_eq!(rig.fails_at_the_last_step(), Err(Refused::Failed));
    assert!(!rig.number(1).exists(), "the copy it made is taken back");
    assert!(!backup(&rig.claude).exists(), "and the first backup");
    assert_eq!(std::fs::read(cookies(&rig.claude)).unwrap(), b"u1");
    assert!(!config(&rig.claude).exists(), "config.json was undone");

    std::fs::create_dir_all(rig.store.join(UNNAMED_DIR)).unwrap();
    std::fs::write(rig.number(1), b"u1").unwrap();
    assert_eq!(rig.fails_at_the_last_step(), Err(Refused::Failed));
    assert_eq!(
        std::fs::read(rig.number(1)).unwrap(),
        b"u1",
        "not its to take"
    );
    assert!(!rig.number(2).exists());
}

#[test]
fn a_change_that_fails_at_the_last_step_has_kept_the_live_login_under_its_own_account() {
    let rig = Rig::new("named-failed", Some("u-a"), b"live-a");
    let own = saved(&rig.store, "u-a");
    assert!(save(&own, b"old-a"));
    assert_eq!(rig.fails_at_the_last_step(), Err(Refused::Failed));
    assert_eq!(std::fs::read(&own).unwrap(), b"live-a");
    assert!(!saved(&rig.store, "u-b").exists());
    assert!(!backup(&rig.claude).exists(), "nothing was replaced");
    assert_eq!(std::fs::read(cookies(&rig.claude)).unwrap(), b"live-a");
    assert_eq!(
        std::fs::read_to_string(config(&rig.claude)).unwrap(),
        names("u-a")
    );

    let done = change(&rig.dirs(), Some((rig.other.as_path(), Some("u-b")))).unwrap();
    assert_eq!(done.map(|kept| kept.copy), Some(own));
    assert_eq!(std::fs::read(backup(&rig.claude)).unwrap(), b"live-a");
    assert_eq!(std::fs::read(cookies(&rig.claude)).unwrap(), b"saved-b");
}

#[test]
fn a_login_that_did_not_read_the_same_twice_is_unsettled_not_unkept() {
    let rig = Rig::new("unread", Some("u-a"), b"live");
    let being_written = steady(|| false, || found(&rig.claude)).err().unwrap();
    assert_eq!(refusal(&being_written), Refused::Unsettled);
    let reads = std::cell::Cell::new(0u8);
    let differs = steady(
        || true,
        || {
            reads.set(reads.get() + 1);
            Ok(reads.get())
        },
    );
    assert_eq!(refusal(&differs.err().unwrap()), Refused::Unsettled);
    std::fs::write(config(&rig.claude), "not settings").unwrap();
    let unreadable = whole(&rig.claude).err().unwrap();
    assert_eq!(refusal(&unreadable), Refused::Failed);
    std::fs::write(config(&rig.claude), names("u-a")).unwrap();
    std::fs::remove_file(cookies(&rig.claude)).unwrap();
    std::fs::create_dir(cookies(&rig.claude)).unwrap();
    let no_file = whole(&rig.claude).err().unwrap();
    assert_eq!(refusal(&no_file), Refused::Unkept);
}
