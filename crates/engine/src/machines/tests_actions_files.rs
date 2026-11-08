//! Ported from `MachinesMorningTests.swift`: screenshots copied from a machine, and the names a
//! machine may and may not give them (C20).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::actions::{destination, is_plain_image_name};
use super::fakes_jobs::{bad, ok, AnswerRunner};
use super::morning::MorningKind;
use super::tests_actions::{actions_over, item, m1, ready, ready_item, temp_cache};

fn shots_runner() -> Arc<AnswerRunner> {
    Arc::new(AnswerRunner::new(|argv| {
        if argv[0] == "/usr/bin/ssh" {
            ok("a.png\nnotes.txt\nB.JPG\nc d.png\n")
        } else if argv.last().is_some_and(|l| l.ends_with("B.JPG")) {
            bad("")
        } else {
            ok("")
        }
    }))
}

struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn screenshots_copy_only_images_with_quoted_sources() {
    let cache = temp_cache("shots");
    let _clean = Cleanup(cache.clone());
    let runner = shots_runner();
    let urls =
        actions_over(&runner, vec![m1()], cache.clone()).screenshots(&item(MorningKind::Ready, 7));

    let dir = cache.join("m1/7");
    assert_eq!(
        urls,
        [dir.join("a.png"), dir.join("c d.png")],
        "B.JPG failed to copy and is left out"
    );
    assert!(dir.exists(), "the cache folder was created");
    let scps: Vec<Vec<String>> = runner
        .calls()
        .into_iter()
        .filter(|c| c[0] == "/usr/bin/scp")
        .collect();
    assert_eq!(
        scps.len(),
        3,
        "the text file is not copied; the failed copy was still tried"
    );
    assert!(scps
        .iter()
        .any(|c| c.contains(&"agent@m1.tail:'/wt/7/evidence/c d.png'".to_string())));
    for scp in &scps {
        let dashes = scp.iter().position(|a| a == "--").expect("--");
        let host = scp
            .iter()
            .position(|a| a.starts_with("agent@"))
            .expect("a host");
        assert!(
            dashes < host,
            "-- ends the options before the host: {scp:?}"
        );
    }
}

// C20: names from the machine, repos from the machine

/// The names come from the machine's `ls`: none of them may leave the item's cache folder.
#[test]
fn screenshot_names_cannot_leave_the_cache_folder() {
    let cache = temp_cache("shots");
    let _clean = Cleanup(cache.clone());
    let names = [
        "../escaped.png",
        "../../../../escaped.png",
        "a/b.png",
        "/abs/path.png",
        "..",
        ".",
        "-o.png",
        "..png/x.png",
        "notes.txt",
        "ok.png",
        "UPPER.JPEG",
        "shot (1).jpg",
    ];
    let listing = format!("{}\n", names.join("\n"));
    let runner = Arc::new(AnswerRunner::new(move |argv| {
        if argv[0] == "/usr/bin/ssh" {
            ok(&listing)
        } else {
            ok("")
        }
    }));
    let urls =
        actions_over(&runner, vec![m1()], cache.clone()).screenshots(&item(MorningKind::Ready, 7));

    let dir = cache.join("m1/7");
    assert_eq!(
        urls,
        [
            dir.join("UPPER.JPEG"),
            dir.join("ok.png"),
            dir.join("shot (1).jpg")
        ]
    );
    let scps: Vec<Vec<String>> = runner
        .calls()
        .into_iter()
        .filter(|c| c[0] == "/usr/bin/scp")
        .collect();
    assert_eq!(scps.len(), 3, "only plain names were fetched");
    let prefix = format!("{}/", dir.display());
    for scp in &scps {
        let last = scp.last().expect("a target");
        assert!(
            last.starts_with(&prefix),
            "a copy was aimed outside the cache folder: {scp:?}"
        );
        assert!(!last.contains(".."));
        assert!(!scp
            .iter()
            .any(|a| a.contains("escaped") || a.contains("a/b.png") || a.contains("-o.png")));
    }
}

#[test]
fn a_hostile_evidence_folder_is_never_listed() {
    for dir in [
        "/wt/7/ev; touch /tmp/x",
        "/wt/../etc",
        "relative/ev",
        "/wt/$(id)",
        "-oProxyCommand=x",
        "/wt/a b",
    ] {
        let cache = temp_cache("shots");
        let _clean = Cleanup(cache.clone());
        let runner = shots_runner();
        let mut entry = ready(7);
        entry.evidence_dir = dir.into();
        let urls = actions_over(&runner, vec![m1()], cache.clone()).screenshots(&ready_item(entry));
        assert!(urls.is_empty(), "{dir}");
        assert!(runner.calls().is_empty(), "{dir} reached a command");
        assert!(!cache.exists());
    }
}

/// The cache folder is named after the registered machine; a name with a slash would leave it.
#[test]
fn a_machine_name_that_would_leave_the_cache_folder_gets_no_screenshots() {
    let cache = temp_cache("shots");
    let _clean = Cleanup(cache.clone());
    let runner = shots_runner();
    let mut odd = m1();
    odd.name = "../up".into();
    let mut it = item(MorningKind::Ready, 7);
    it.machine = "../up".into();
    assert!(actions_over(&runner, vec![odd], cache)
        .screenshots(&it)
        .is_empty());
    assert!(runner.calls().is_empty());
}

#[test]
fn only_a_plain_image_name_is_accepted() {
    for good in [
        "a.png",
        "A.PNG",
        "shot (1).jpg",
        "x.JPEG",
        "a..b.png",
        "é.png",
    ] {
        assert!(is_plain_image_name(good), "{good}");
    }
    let bad = [
        "",
        ".",
        "..",
        "../x.png",
        "a/b.png",
        "/x.png",
        "x.png/",
        "-o.png",
        "--help.jpg",
        ".png",
        "x.txt",
        "x",
        "bad\nname.png",
        "bad\rname.png",
        "bad\0name.png",
        "bad\tname.png",
        "x.png\n",
        "\u{7f}.png",
    ];
    for name in bad {
        assert!(!is_plain_image_name(name), "{name:?} was accepted");
    }
}

#[test]
fn the_destination_must_stay_inside_the_cache_folder() {
    let dir = Path::new("/tmp/cache/m1/7");
    assert_eq!(
        destination("ok.png", dir),
        Some(PathBuf::from("/tmp/cache/m1/7/ok.png"))
    );
    for bad in [
        "../escaped.png",
        "../../x.png",
        "a/../../x.png",
        "/etc/x.png",
        "..",
        ".",
        "",
    ] {
        assert_eq!(destination(bad, dir), None, "{bad}");
    }
}
