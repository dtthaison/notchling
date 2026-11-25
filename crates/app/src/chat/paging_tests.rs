use super::*;

struct Job {
    id: String,
    running: bool,
}

impl Item for Job {
    fn id(&self) -> &str {
        &self.id
    }

    fn running(&self) -> bool {
        self.running
    }
}

fn jobs(total: usize, running: &[usize]) -> Vec<Job> {
    (0..total)
        .map(|n| Job {
            id: format!("j{n}"),
            running: running.contains(&n),
        })
        .collect()
}

fn ids(items: &[Job], shown: &Shown) -> Vec<String> {
    shown.indexes.iter().map(|&i| items[i].id.clone()).collect()
}

#[test]
fn with_forty_and_two_running_only_those_two_show() {
    let items = jobs(40, &[5, 31]);
    let shown = Page::default().show(Some("s"), &items, 0);
    assert_eq!(shown.indexes, [31, 5], "newest first");
    assert_eq!(
        shown.counts,
        Counts {
            total: 40,
            running: 2,
            hidden: 38
        }
    );
}

#[test]
fn with_none_running_the_three_newest_show() {
    let items = jobs(40, &[]);
    let shown = Page::default().show(Some("s"), &items, 0);
    assert_eq!(shown.indexes, [39, 38, 37]);
    assert_eq!(shown.counts.hidden, 37);
    let few = jobs(2, &[]);
    let shown = Page::default().show(Some("s"), &few, 0);
    assert_eq!((shown.indexes, shown.counts.hidden), (vec![1, 0], 0));
}

#[test]
fn each_load_more_adds_the_next_ten_newest_first_until_none_are_left() {
    let items = jobs(40, &[]);
    let mut page = Page::default();
    let first = page.show(Some("s"), &items, 0);
    assert_eq!(first.counts.next_page(), 10);
    let second = page.show(Some("s"), &items, 1);
    assert_eq!(second.indexes, (27..40).rev().collect::<Vec<_>>());
    assert_eq!(second.counts.hidden, 27);
    let third = page.show(Some("s"), &items, 1);
    assert_eq!(third.indexes.len(), 23);
    let fourth = page.show(Some("s"), &items, 1);
    assert_eq!(fourth.indexes.len(), 33);
    let last = page.show(Some("s"), &items, 1);
    assert_eq!(last.indexes, (0..40).rev().collect::<Vec<_>>());
    assert_eq!(last.counts.hidden, 0);
    assert_eq!(last.counts.next_page(), 0, "the row goes away");
}

#[test]
fn the_last_page_is_only_what_is_left() {
    let items = jobs(16, &[]);
    let mut page = Page::default();
    let first = page.show(Some("s"), &items, 0);
    assert_eq!(first.counts.next_page(), 10);
    let second = page.show(Some("s"), &items, 1);
    assert_eq!(second.counts.hidden, 3);
    assert_eq!(second.counts.next_page(), 3);
}

#[test]
fn a_second_look_at_an_unchanged_list_changes_nothing() {
    let items = jobs(40, &[3]);
    let mut page = Page::default();
    let first = page.show(Some("s"), &items, 0);
    assert_eq!(page.show(Some("s"), &items, 0), first);
}

#[test]
fn a_running_one_that_finishes_while_shown_stays_shown() {
    let mut items = jobs(40, &[2, 20]);
    let mut page = Page::default();
    assert_eq!(page.show(Some("s"), &items, 0).indexes, [20, 2]);
    items[20].running = false;
    let shown = page.show(Some("s"), &items, 0);
    assert_eq!(shown.indexes, [20, 2]);
    assert_eq!(shown.counts.running, 1);
    items[2].running = false;
    assert_eq!(page.show(Some("s"), &items, 0).indexes, [20, 2]);
}

#[test]
fn a_new_one_goes_to_the_top_and_the_three_done_ones_stay() {
    let mut items = jobs(40, &[]);
    let mut page = Page::default();
    assert_eq!(page.show(Some("s"), &items, 0).indexes, [39, 38, 37]);
    items.push(Job {
        id: "fresh".into(),
        running: true,
    });
    let shown = page.show(Some("s"), &items, 0);
    assert_eq!(ids(&items, &shown), ["fresh", "j39", "j38", "j37"]);
    assert_eq!(shown.counts.total, 41);
    items[40].running = false;
    let again = page.show(Some("s"), &items, 0);
    assert_eq!(ids(&items, &again), ["fresh", "j39", "j38", "j37"]);
}

#[test]
fn a_new_one_that_is_already_done_is_shown_too() {
    let mut items = jobs(10, &[]);
    let mut page = Page::default();
    page.show(Some("s"), &items, 0);
    items.push(Job {
        id: "quick".into(),
        running: false,
    });
    let shown = page.show(Some("s"), &items, 0);
    assert_eq!(ids(&items, &shown)[0], "quick");
    assert_eq!(shown.indexes.len(), 4);
}

#[test]
fn load_more_goes_on_from_where_it_was_after_a_new_one() {
    let mut items = jobs(40, &[]);
    let mut page = Page::default();
    page.show(Some("s"), &items, 0);
    items.push(Job {
        id: "fresh".into(),
        running: true,
    });
    page.show(Some("s"), &items, 0);
    let shown = page.show(Some("s"), &items, 1);
    let got = ids(&items, &shown);
    assert_eq!(got.len(), 14);
    assert_eq!(got[0], "fresh");
    assert_eq!(got[1], "j39");
    assert_eq!(got[13], "j27", "ten more below the three: j36 down to j27");
    assert_eq!(shown.counts.hidden, 27);
}

#[test]
fn one_that_runs_again_after_being_hidden_is_shown() {
    let mut items = jobs(40, &[]);
    let mut page = Page::default();
    page.show(Some("s"), &items, 0);
    items[4].running = true;
    let shown = page.show(Some("s"), &items, 0);
    assert_eq!(shown.indexes, [39, 38, 37, 4]);
}

#[test]
fn a_new_session_starts_on_the_first_page_again() {
    let items = jobs(40, &[]);
    let mut page = Page::default();
    page.show(Some("a"), &items, 2);
    let other = jobs(12, &[7]);
    assert_eq!(page.show(Some("b"), &other, 0).indexes, [7]);
    assert_eq!(page.show(Some("a"), &items, 0).indexes, [39, 38, 37]);
    assert_eq!(page.show(None, &[] as &[Job], 0), Shown::default());
}

#[test]
fn a_session_that_starts_empty_pages_when_its_first_arrive() {
    let mut page = Page::default();
    assert_eq!(page.show(Some("s"), &[] as &[Job], 0), Shown::default());
    let items = jobs(40, &[]);
    assert_eq!(page.show(Some("s"), &items, 0).indexes, [39, 38, 37]);
}
