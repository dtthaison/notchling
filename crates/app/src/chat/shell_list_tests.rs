use notchling_engine::transcript::model::BackgroundShell;

use super::*;

fn conversation(total: usize, running: &[usize]) -> Conversation {
    let mut c = Conversation::default();
    for n in 0..total {
        c.shells.push(BackgroundShell {
            id: format!("b{n}"),
            command: format!("job {n}"),
            title: String::new(),
            output_path: format!("/tmp/s/b{n}.output"),
            started: None,
        });
        if !running.contains(&n) {
            c.finished_agents
                .insert(format!("b{n}"), "completed".into());
        }
    }
    c
}

fn running(_: &str) -> Option<bool> {
    Some(true)
}

fn show(page: &mut ShellPage, c: &Conversation, more: usize) -> Listing {
    page.show(Some("session"), c, more, &running)
}

#[test]
fn with_thirty_and_two_running_the_first_page_shows_and_probes_only_those_two() {
    let c = conversation(30, &[4, 22]);
    let listing = show(&mut ShellPage::default(), &c, 0);
    assert_eq!(listing.ids, ["b22", "b4"], "newest first");
    assert_eq!(
        (
            listing.counts.total,
            listing.counts.running,
            listing.counts.hidden
        ),
        (30, 2, 28)
    );
    let paths = probe_paths(&c, &listing, &running);
    assert_eq!(paths, ["/tmp/s/b22.output", "/tmp/s/b4.output"]);
    let calls = lsof_calls(&paths);
    assert_eq!(calls.len(), 1, "one lsof for all of them");
    assert_eq!(
        calls[0],
        [
            "-w",
            "-F",
            "pcan",
            "--",
            "/tmp/s/b22.output",
            "/tmp/s/b4.output"
        ]
    );
}

#[test]
fn with_none_running_the_first_page_shows_three_and_probes_nothing() {
    let c = conversation(30, &[]);
    let listing = show(&mut ShellPage::default(), &c, 0);
    assert_eq!(listing.ids, ["b29", "b28", "b27"]);
    assert!(
        probe_paths(&c, &listing, &running).is_empty(),
        "ended ones need no lsof"
    );
    assert!(lsof_calls(&[]).is_empty());
}

#[test]
fn each_load_more_adds_ten_and_the_probe_follows_the_shown_unfinished_ones() {
    let c = conversation(30, &[]);
    let mut page = ShellPage::default();
    assert_eq!(show(&mut page, &c, 0).ids.len(), 3);
    let more = show(&mut page, &c, 1);
    assert_eq!(more.ids.len(), 13);
    assert_eq!(more.counts.hidden, 17);
    assert_eq!(more.counts.next_page(), 10);
    assert_eq!(show(&mut page, &c, 0).ids.len(), 13, "it stays");
    for _ in 0..2 {
        show(&mut page, &c, 1);
    }
    let all = show(&mut page, &c, 0);
    assert_eq!((all.ids.len(), all.counts.hidden), (30, 0));
    assert_eq!(all.counts.next_page(), 0, "the control goes away");
}

#[test]
fn a_new_shell_goes_to_the_top_and_the_shown_ones_stay() {
    let mut c = conversation(12, &[]);
    let mut page = ShellPage::default();
    let first = show(&mut page, &c, 0);
    assert_eq!(first.ids, ["b11", "b10", "b9"]);
    c.shells.push(BackgroundShell {
        id: "b12".into(),
        command: "new".into(),
        title: String::new(),
        output_path: "/tmp/s/b12.output".into(),
        started: None,
    });
    let second = show(&mut page, &c, 0);
    assert_eq!(second.ids, ["b12", "b11", "b10", "b9"]);
    assert_eq!(second.counts.total, 13);
    assert_eq!(
        probe_paths(&c, &second, &running),
        ["/tmp/s/b12.output"],
        "the new one is unfinished; the three done need none"
    );
}

#[test]
fn a_running_shell_that_ends_stays_shown_and_leaves_the_probe() {
    let mut c = conversation(10, &[7]);
    let mut page = ShellPage::default();
    let first = show(&mut page, &c, 0);
    assert_eq!(first.ids, ["b7"]);
    c.finished_agents.insert("b7".into(), "completed".into());
    let second = show(&mut page, &c, 0);
    assert_eq!(second.ids, ["b7"]);
    assert!(probe_paths(&c, &second, &running).is_empty());
}

#[test]
fn another_session_starts_the_first_page_again() {
    let c = conversation(30, &[]);
    let mut page = ShellPage::default();
    show(&mut page, &c, 0);
    assert_eq!(show(&mut page, &c, 1).ids.len(), 13);
    let other = page.show(Some("other"), &c, 0, &running);
    assert_eq!(other.ids.len(), 3);
}

#[test]
fn the_probe_lists_each_path_once_and_skips_shells_without_one() {
    let mut c = conversation(4, &[0, 1, 2, 3]);
    c.shells[1].output_path = c.shells[0].output_path.clone();
    c.shells[2].output_path.clear();
    let listing = show(&mut ShellPage::default(), &c, 0);
    assert_eq!(listing.ids.len(), 4);
    assert_eq!(
        probe_paths(&c, &listing, &running),
        ["/tmp/s/b3.output", "/tmp/s/b0.output"]
    );
}

#[test]
fn a_long_list_goes_in_calls_of_a_hundred() {
    let paths: Vec<String> = (0..250).map(|n| format!("/tmp/s/b{n}.output")).collect();
    let calls = lsof_calls(&paths);
    assert_eq!(
        calls.iter().map(Vec::len).collect::<Vec<_>>(),
        [104, 104, 54],
        "four options, then the paths"
    );
    let listed: Vec<&String> = calls.iter().flat_map(|c| c.iter().skip(4)).collect();
    assert_eq!(listed.len(), 250);
    assert_eq!(listed[0], &paths[0]);
    assert_eq!(listed[249], &paths[249]);
}

#[test]
fn the_model_draws_only_the_shown_rows_and_asks_writers_only_about_them() {
    use crate::chat::inspector::inspector_listed;
    use std::cell::RefCell;

    let c = conversation(30, &[4, 22]);
    let listing = show(&mut ShellPage::default(), &c, 0);
    let asked = RefCell::new(Vec::new());
    let asking = |p: &str| {
        asked.borrow_mut().push(p.to_owned());
        vec![0]
    };
    let i = inspector_listed(&c, &[], Counts::default(), Some(&listing), &asking);
    assert_eq!(i.shells.len(), 2);
    assert_eq!(i.shells[0].command, "job 22", "newest first");
    assert!(i.shells.iter().all(|s| s.running));
    assert_eq!(i.shells_title(), "Shells · 2 running");
    assert_eq!(i.shell_counts.total, 30);
    assert_eq!(i.shell_counts.next_page(), 10);
    let mut asked = asked.into_inner();
    asked.sort();
    let mut probed = probe_paths(&c, &listing, &running);
    probed.sort();
    assert_eq!(asked, probed, "what the model asks is what the probe lists");
}

#[test]
fn with_none_running_the_title_counts_all_of_them_over_three_rows() {
    use crate::chat::inspector::{inspector_listed, no_writers};

    let c = conversation(30, &[]);
    let listing = show(&mut ShellPage::default(), &c, 0);
    let i = inspector_listed(&c, &[], Counts::default(), Some(&listing), &no_writers);
    assert_eq!(i.shells.len(), 3);
    assert_eq!(i.shells_title(), "Shells · 30");
    assert!(i.shells.iter().all(|s| s.status_word == "Completed"));
}

#[test]
fn a_shell_not_probed_yet_shows_running_and_one_the_probe_found_idle_shows_ended() {
    use crate::chat::inspector::inspector_listed;

    let c = conversation(3, &[0, 1]);
    let listing = show(&mut ShellPage::default(), &c, 0);
    let unprobed = inspector_listed(&c, &[], Counts::default(), Some(&listing), &|_: &str| {
        vec![0]
    });
    assert!(unprobed.shells.iter().all(|s| s.running));
    let probed = inspector_listed(&c, &[], Counts::default(), Some(&listing), &|p: &str| {
        if p.ends_with("b1.output") {
            vec![4242]
        } else {
            Vec::new()
        }
    });
    let words: Vec<(&str, bool)> = probed
        .shells
        .iter()
        .map(|s| (s.command.as_str(), s.running))
        .collect();
    assert_eq!(words, [("job 1", true), ("job 0", false)]);
}

fn unknown(_: &str) -> Option<bool> {
    None
}

fn writing_only(names: &'static [&'static str]) -> impl Fn(&str) -> Option<bool> {
    move |path| {
        Some(
            names
                .iter()
                .any(|n| path.ends_with(&format!("/{n}.output"))),
        )
    }
}

fn every_shell_unfinished(total: usize) -> Conversation {
    conversation(total, &(0..total).collect::<Vec<_>>())
}

#[test]
fn shells_the_transcript_never_ended_wait_for_the_probe_and_then_page_by_it() {
    let c = every_shell_unfinished(30);
    let mut page = ShellPage::default();
    let waiting = page.show(Some("session"), &c, 0, &unknown);
    assert!(waiting.waiting);
    assert_eq!(waiting.ids.len(), 10, "a first look is one page, not all");
    assert_eq!(waiting.ids[0], "b29");
    assert_eq!(waiting.counts.hidden, 0, "no control to press yet");
    assert_eq!(waiting.counts.total, 30);
    let asked = probe_paths(&c, &waiting, &unknown);
    assert_eq!(asked.len(), 30, "every unfinished one is asked about");
    assert_eq!(lsof_calls(&asked).len(), 1);

    let probed = writing_only(&["b3", "b17"]);
    let settled = page.show(Some("session"), &c, 0, &probed);
    assert!(!settled.waiting);
    assert_eq!(settled.ids, ["b17", "b3"], "the two with a writer");
    assert_eq!(
        (
            settled.counts.total,
            settled.counts.running,
            settled.counts.hidden
        ),
        (30, 2, 28)
    );
    assert_eq!(
        probe_paths(&c, &settled, &probed),
        ["/tmp/s/b17.output", "/tmp/s/b3.output"],
        "an idle one is not asked about again"
    );

    let more = page.show(Some("session"), &c, 1, &probed);
    assert_eq!(more.ids.len(), 12);
    assert_eq!(more.counts.hidden, 18);
    assert_eq!(probe_paths(&c, &more, &probed).len(), 2);
}

#[test]
fn a_probe_that_finds_every_shell_idle_shows_the_three_newest_like_ended_ones() {
    let c = every_shell_unfinished(20);
    let mut page = ShellPage::default();
    assert!(page.show(Some("session"), &c, 0, &unknown).waiting);
    let none_writing = |_: &str| Some(false);
    let settled = page.show(Some("session"), &c, 0, &none_writing);
    assert_eq!(settled.ids, ["b19", "b18", "b17"]);
    assert_eq!(settled.counts.running, 0);
    assert_eq!(settled.counts.hidden, 17);
    assert!(probe_paths(&c, &settled, &none_writing).is_empty());
}

#[test]
fn only_the_unprobed_ones_are_asked_about_while_waiting() {
    let c = every_shell_unfinished(4);
    let mut page = ShellPage::default();
    let some = |p: &str| p.ends_with("/b0.output").then_some(true);
    let listing = page.show(Some("session"), &c, 0, &some);
    assert!(listing.waiting);
    assert_eq!(
        probe_paths(&c, &listing, &some),
        ["/tmp/s/b3.output", "/tmp/s/b2.output", "/tmp/s/b1.output"]
    );
}

#[test]
fn ended_shells_and_shells_with_no_path_do_not_wait_for_a_probe() {
    let c = conversation(5, &[]);
    let listing = page_of(&c);
    assert!(!listing.waiting);
    assert_eq!(listing.ids.len(), 3);

    let mut pathless = every_shell_unfinished(2);
    for shell in &mut pathless.shells {
        shell.output_path.clear();
    }
    let listing = page_of(&pathless);
    assert!(!listing.waiting, "nothing to ask about");
    assert_eq!(listing.ids, ["b1", "b0"], "shown as running, as before");
    assert!(probe_paths(&pathless, &listing, &unknown).is_empty());
}

fn page_of(c: &Conversation) -> Listing {
    ShellPage::default().show(Some("session"), c, 0, &unknown)
}

#[test]
fn once_settled_a_shown_one_that_goes_idle_stays_shown_and_is_asked_about_no_more() {
    let c = every_shell_unfinished(6);
    let mut page = ShellPage::default();
    page.show(Some("session"), &c, 0, &unknown);
    let both = writing_only(&["b4", "b5"]);
    let first = page.show(Some("session"), &c, 0, &both);
    assert_eq!(first.ids, ["b5", "b4"]);
    let one_left = writing_only(&["b5"]);
    let second = page.show(Some("session"), &c, 0, &one_left);
    assert_eq!(second.ids, ["b5", "b4"], "it only grows");
    assert_eq!(second.counts.running, 1);
    assert_eq!(
        probe_paths(&c, &second, &one_left),
        ["/tmp/s/b5.output"],
        "the idle one left the probe"
    );
}

#[test]
fn a_shell_that_starts_after_the_first_page_goes_on_top_and_is_probed() {
    let mut c = every_shell_unfinished(5);
    let mut page = ShellPage::default();
    page.show(Some("session"), &c, 0, &unknown);
    let probed = writing_only(&["b1"]);
    assert_eq!(page.show(Some("session"), &c, 0, &probed).ids, ["b1"]);
    c.shells.push(BackgroundShell {
        id: "b5".into(),
        command: "new".into(),
        title: String::new(),
        output_path: "/tmp/s/b5.output".into(),
        started: None,
    });
    let after = |p: &str| (!p.ends_with("/b5.output")).then(|| p.ends_with("/b1.output"));
    let listing = page.show(Some("session"), &c, 0, &after);
    assert!(!listing.waiting, "the page was made once");
    assert_eq!(listing.ids, ["b5", "b1"]);
    assert_eq!(
        probe_paths(&c, &listing, &after),
        ["/tmp/s/b5.output", "/tmp/s/b1.output"],
        "the new one is asked about"
    );
}

#[test]
fn another_session_waits_for_its_own_probe() {
    let c = every_shell_unfinished(5);
    let mut page = ShellPage::default();
    page.show(Some("session"), &c, 0, &unknown);
    assert!(
        !page
            .show(Some("session"), &c, 0, &writing_only(&["b1"]))
            .waiting
    );
    assert!(page.show(Some("other"), &c, 0, &unknown).waiting);
}
