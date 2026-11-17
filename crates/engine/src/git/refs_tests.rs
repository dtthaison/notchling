use super::refs::{
    parse_ahead_behind, parse_left_right, parse_refs, parse_shortstat, parse_track, plain_label,
    remote_hosts, Counts, Lines, Track, MAX_REFS, SUBJECT_CHARS,
};

const A: &str = "0823e49772e044ef8d683209cb2c931dec786b96";
const B: &str = "1111111111111111111111111111111111111111";

fn record(fields: [&str; 6]) -> String {
    format!("{}\n", fields.join("\0"))
}

#[test]
fn ref_record_fields() {
    let text = format!(
        "{}{}",
        record([
            "refs/heads/feat",
            A,
            "1794887941",
            "origin/feat",
            "ahead 1, behind 2",
            "third commit"
        ]),
        record(["refs/heads/main", B, "1794887000", "", "", "first"]),
    );
    let (rows, cut) = parse_refs(&text);
    assert!(!cut);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].refname, "refs/heads/feat");
    assert_eq!(rows[0].commit, A);
    assert_eq!(rows[0].committed_ms, Some(1_794_887_941_000));
    assert_eq!(rows[0].upstream.as_deref(), Some("origin/feat"));
    assert_eq!(
        rows[0].track,
        Track {
            ahead: 1,
            behind: 2,
            gone: false
        }
    );
    assert_eq!(rows[0].subject, "third commit");
    assert_eq!(rows[1].upstream, None);
    assert_eq!(rows[1].track, Track::default());
}

#[test]
fn track_forms() {
    assert_eq!(
        parse_track("ahead 2, behind 1"),
        Track {
            ahead: 2,
            behind: 1,
            gone: false
        }
    );
    assert_eq!(
        parse_track("ahead 2"),
        Track {
            ahead: 2,
            behind: 0,
            gone: false
        }
    );
    assert_eq!(
        parse_track("behind 3"),
        Track {
            ahead: 0,
            behind: 3,
            gone: false
        }
    );
    assert_eq!(
        parse_track("gone"),
        Track {
            ahead: 0,
            behind: 0,
            gone: true
        }
    );
    assert_eq!(parse_track(""), Track::default());
    assert_eq!(parse_track("ahead x"), Track::default());
}

#[test]
fn origin_head_skipped() {
    let text = format!(
        "{}{}{}",
        record(["refs/remotes/origin/HEAD", A, "1", "", "", "s"]),
        record(["refs/remotes/origin/main", A, "1", "", "", "s"]),
        record(["refs/remotes/origin/agent/ask", B, "1", "", "", "s"]),
    );
    let (rows, _) = parse_refs(&text);
    let names: Vec<&str> = rows.iter().map(|row| row.refname.as_str()).collect();
    assert_eq!(
        names,
        ["refs/remotes/origin/main", "refs/remotes/origin/agent/ask"]
    );
}

#[test]
fn refs_cut_at_300() {
    let mut text = record(["refs/remotes/origin/HEAD", A, "1", "", "", "s"]);
    for n in 0..MAX_REFS + 1 {
        text.push_str(&record(["refs/heads/b", A, &n.to_string(), "", "", "s"]));
    }
    let (rows, cut) = parse_refs(&text);
    assert!(cut);
    assert_eq!(rows.len(), MAX_REFS);

    let mut exact = String::new();
    for _ in 0..MAX_REFS {
        exact.push_str(&record(["refs/heads/b", A, "1", "", "", "s"]));
    }
    let (rows, cut) = parse_refs(&exact);
    assert!(!cut);
    assert_eq!(rows.len(), MAX_REFS);
}

#[test]
fn ahead_behind_atom_lines() {
    let text = "refs/heads/feat\x002 0\nrefs/heads/main\x000 0\nrefs/heads/orph\x001 1\nbad line\nrefs/heads/x\0a b\n";
    let counts = parse_ahead_behind(text);
    assert_eq!(counts.len(), 3);
    assert_eq!(
        counts["refs/heads/feat"],
        Counts {
            ahead: 2,
            behind: 0
        }
    );
    assert_eq!(counts["refs/heads/main"], Counts::default());
    assert_eq!(
        counts["refs/heads/orph"],
        Counts {
            ahead: 1,
            behind: 1
        }
    );
}

#[test]
fn left_right_prints_behind_first() {
    assert_eq!(
        parse_left_right("3\t2\n"),
        Some(Counts {
            ahead: 2,
            behind: 3
        })
    );
    assert_eq!(parse_left_right(""), None);
    assert_eq!(parse_left_right("3\n"), None);
    assert_eq!(parse_left_right("a\tb\n"), None);
}

#[test]
fn both_count_forms_agree_for_one_pair() {
    let atom = parse_ahead_behind("refs/heads/feat\x002 1\n");
    let left_right = parse_left_right("1\t2\n");
    assert_eq!(Some(atom["refs/heads/feat"]), left_right);
}

#[test]
fn shortstat_insertions_only() {
    assert_eq!(
        parse_shortstat(" 3 files changed, 3 insertions(+)\n"),
        Lines {
            added: 3,
            removed: 0,
            files: 3
        }
    );
}

#[test]
fn shortstat_deletions_only() {
    assert_eq!(
        parse_shortstat(" 2 files changed, 7 deletions(-)\n"),
        Lines {
            added: 0,
            removed: 7,
            files: 2
        }
    );
}

#[test]
fn shortstat_both() {
    assert_eq!(
        parse_shortstat(" 4 files changed, 120 insertions(+), 33 deletions(-)\n"),
        Lines {
            added: 120,
            removed: 33,
            files: 4
        }
    );
    assert_eq!(parse_shortstat(""), Lines::default());
}

#[test]
fn shortstat_one_file() {
    assert_eq!(
        parse_shortstat(" 1 file changed, 1 insertion(+)\n"),
        Lines {
            added: 1,
            removed: 0,
            files: 1
        }
    );
    assert_eq!(
        parse_shortstat(" 1 file changed, 1 deletion(-)\n"),
        Lines {
            added: 0,
            removed: 1,
            files: 1
        }
    );
}

#[test]
fn shortstat_binary_only() {
    let expected = Lines {
        added: 0,
        removed: 0,
        files: 1,
    };
    assert_eq!(parse_shortstat(" 1 file changed\n"), expected);
    assert_eq!(
        parse_shortstat(" 1 file changed, 0 insertions(+), 0 deletions(-)\n"),
        expected
    );
}

fn config(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(name, url)| format!("remote.{name}.url\n{url}\0"))
        .collect()
}

#[test]
fn remote_host_https_ssh_and_scp() {
    let text = config(&[
        ("origin", "https://github.com/o/r.git"),
        ("up", "git@Github.com:o/r.git"),
        ("s", "ssh://u@H.example:2222/p/q"),
        ("v6", "ssh://u@[::1]:22/p"),
        ("local", "/srv/repo.git"),
        ("rel", "../repo"),
        ("file", "file:///srv/repo.git"),
    ]);
    assert_eq!(
        remote_hosts(&text),
        vec![
            ("origin".to_string(), Some("github.com".to_string())),
            ("up".to_string(), Some("github.com".to_string())),
            ("s".to_string(), Some("h.example".to_string())),
            ("v6".to_string(), Some("[::1]".to_string())),
            ("local".to_string(), None),
            ("rel".to_string(), None),
            ("file".to_string(), None),
        ]
    );
}

#[test]
fn remote_host_drops_user_and_token() {
    let text = config(&[
        ("origin", "https://me:ghp_x@github.com/o/r.git"),
        ("odd", "https://me:p@ss@Example.com:8443/o/r"),
        ("tok", "https://ghp_x@github.com/o/r.git"),
    ]);
    let hosts = remote_hosts(&text);
    let shown = format!("{hosts:?}").replace("Some", "");
    assert!(!shown.contains("me"), "{shown}");
    assert!(!shown.contains("ghp_x"), "{shown}");
    assert!(!shown.contains("p@ss"), "{shown}");
    assert!(!shown.contains('@'), "{shown}");
    assert_eq!(hosts[0].1.as_deref(), Some("github.com"));
    assert_eq!(hosts[1].1.as_deref(), Some("example.com"));
    assert_eq!(hosts[2].1.as_deref(), Some("github.com"));
}

#[test]
fn plain_label_drops_controls_and_cuts_120_on_char() {
    let mut subject = "a".repeat(SUBJECT_CHARS - 2);
    subject.push('\u{e9}');
    subject.push('\u{202e}');
    subject.push('\t');
    subject.push('\u{1b}');
    subject.push('\u{f1}');
    subject.push_str(&"\u{f1}".repeat(400));
    assert!(subject.chars().count() > 500);
    let label = plain_label(&subject, SUBJECT_CHARS);
    assert_eq!(label.chars().count(), SUBJECT_CHARS);
    assert!(label.chars().all(|c| !c.is_control() && c != '\u{202e}'));
    assert!(label.ends_with("\u{e9}\u{f1}"));
    assert_eq!(plain_label("a\nb\u{7}c", 10), "abc");
    assert_eq!(plain_label("  spaced  ", 10), "spaced");
    assert_eq!(plain_label("short", 0), "");
}
