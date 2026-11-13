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

fn show(page: &mut ShellPage, c: &Conversation, more: usize) -> Listing {
    page.show(Some("session"), c, more)
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
    let paths = probe_paths(&c, &listing);
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
        probe_paths(&c, &listing).is_empty(),
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
        probe_paths(&c, &second),
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
    assert!(probe_paths(&c, &second).is_empty());
}

#[test]
fn another_session_starts_the_first_page_again() {
    let c = conversation(30, &[]);
    let mut page = ShellPage::default();
    show(&mut page, &c, 0);
    assert_eq!(show(&mut page, &c, 1).ids.len(), 13);
    let other = page.show(Some("other"), &c, 0);
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
        probe_paths(&c, &listing),
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
    let running = |p: &str| {
        asked.borrow_mut().push(p.to_owned());
        vec![0]
    };
    let i = inspector_listed(&c, &[], Some(&listing), &running);
    assert_eq!(i.shells.len(), 2);
    assert_eq!(i.shells[0].command, "job 22", "newest first");
    assert!(i.shells.iter().all(|s| s.running));
    assert_eq!(i.shells_title(), "Shells · 2 running");
    assert_eq!(i.shell_counts.total, 30);
    assert_eq!(i.shell_counts.next_page(), 10);
    let mut asked = asked.into_inner();
    asked.sort();
    let mut probed = probe_paths(&c, &listing);
    probed.sort();
    assert_eq!(asked, probed, "what the model asks is what the probe lists");
}

#[test]
fn with_none_running_the_title_counts_all_of_them_over_three_rows() {
    use crate::chat::inspector::{inspector_listed, no_writers};

    let c = conversation(30, &[]);
    let listing = show(&mut ShellPage::default(), &c, 0);
    let i = inspector_listed(&c, &[], Some(&listing), &no_writers);
    assert_eq!(i.shells.len(), 3);
    assert_eq!(i.shells_title(), "Shells · 30");
    assert!(i.shells.iter().all(|s| s.status_word == "Completed"));
}

#[test]
fn a_shell_not_probed_yet_shows_running_and_one_the_probe_found_idle_shows_ended() {
    use crate::chat::inspector::inspector_listed;

    let c = conversation(3, &[0, 1]);
    let listing = show(&mut ShellPage::default(), &c, 0);
    let unprobed = inspector_listed(&c, &[], Some(&listing), &|_: &str| vec![0]);
    assert!(unprobed.shells.iter().all(|s| s.running));
    let probed = inspector_listed(&c, &[], Some(&listing), &|p: &str| {
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
