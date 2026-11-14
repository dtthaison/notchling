use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use notchling_engine::transcript::subagents::{Origin, Status, Subagent};

use super::paging::{Counts, Entry, Tree, TreeShown};
use super::subagent_rows::{Facts, Peek, SubagentRows};
use crate::testutil::Scratch;

const TOP: &str = "claude:top";

fn row(id: &str, parent: &str, depth: u8, started: f64, running: bool) -> Subagent {
    Subagent {
        id: id.into(),
        path: String::new(),
        description: id.into(),
        agent_type: String::new(),
        model: None,
        effort: None,
        background: false,
        tool_use_id: None,
        started,
        updated: started,
        status: if running {
            Status::Running
        } else {
            Status::Completed
        },
        origin: Origin::default(),
        session: TOP.into(),
        parent: parent.into(),
        depth,
    }
}

fn top_rows(n: usize) -> Vec<Subagent> {
    (0..n)
        .map(|i| row(&format!("t{i}"), TOP, 1, i as f64, false))
        .collect()
}

fn with_children(parent: &str, n: usize) -> Vec<Subagent> {
    let mut rows = vec![row(parent, TOP, 1, 1.0, false)];
    rows.extend((0..n).map(|i| row(&format!("{parent}-{i}"), parent, 2, 10.0 + i as f64, false)));
    rows
}

fn ids(rows: &[Subagent], shown: &TreeShown) -> Vec<String> {
    shown
        .entries
        .iter()
        .map(|e| format!("{}{}", e.depth, rows[e.index].id))
        .collect()
}

fn show(tree: &mut Tree, session: &str, rows: &[Subagent], open: &[&str]) -> TreeShown {
    let open: HashSet<String> = open.iter().map(|s| s.to_string()).collect();
    tree.show(Some(session), rows, &open, &HashMap::new())
}

#[test]
fn depth_one_pages_as_before() {
    let rows = top_rows(5);
    let mut tree = Tree::default();
    let shown = show(&mut tree, TOP, &rows, &[]);
    assert_eq!(ids(&rows, &shown), ["1t4", "1t3", "1t2"]);
    assert_eq!(
        shown.counts,
        Counts {
            total: 5,
            running: 0,
            hidden: 2
        }
    );
    let mut running = top_rows(5);
    running[1].status = Status::Running;
    let shown = show(&mut Tree::default(), TOP, &running, &[]);
    assert_eq!(ids(&running, &shown), ["1t1"]);
    assert_eq!(shown.counts.hidden, 4);
    let more = HashMap::from([(String::new(), 1)]);
    let shown = Tree::default().show(Some(TOP), &rows, &HashSet::new(), &more);
    assert_eq!(shown.entries.len(), 5);
}

#[test]
fn a_row_with_children_starts_collapsed_and_counts_them() {
    let rows = with_children("a", 4);
    let shown = show(&mut Tree::default(), TOP, &rows, &[]);
    assert_eq!(
        shown.entries,
        [Entry {
            index: 0,
            depth: 1,
            children: 4,
            open: false
        }]
    );
    assert!(shown.more.is_empty());
    assert_eq!(shown.counts.total, 5, "the title counts every row");
}

#[test]
fn opening_a_row_shows_its_children_paged() {
    let rows = with_children("a", 5);
    let mut tree = Tree::default();
    let shown = show(&mut tree, TOP, &rows, &["a"]);
    assert_eq!(ids(&rows, &shown), ["1a", "2a-4", "2a-3", "2a-2"]);
    assert!(shown.entries[0].open);
    assert_eq!(shown.more.len(), 1);
    let more = &shown.more[0];
    assert_eq!((more.after, more.group.as_str(), more.next), (3, "a", 2));
    let revealed = HashMap::from([("a".to_string(), 1)]);
    let open = HashSet::from(["a".to_string()]);
    let shown = tree.show(Some(TOP), &rows, &open, &revealed);
    assert_eq!(shown.entries.len(), 6);
    assert!(shown.more.is_empty());
    let shown = show(&mut tree, TOP, &rows, &["a"]);
    assert_eq!(shown.entries.len(), 6, "shown rows stay shown");
    let shown = show(&mut tree, TOP, &rows, &[]);
    assert_eq!(shown.entries.len(), 1, "closing hides them again");
}

#[test]
fn a_nested_group_opens_under_its_row() {
    let mut rows = with_children("a", 2);
    rows.push(row("a-0-x", "a-0", 3, 20.0, false));
    let shown = show(&mut Tree::default(), TOP, &rows, &["a", "a-0"]);
    assert_eq!(ids(&rows, &shown), ["1a", "2a-1", "2a-0", "3a-0-x"]);
    let shown = show(&mut Tree::default(), TOP, &rows, &["a"]);
    assert_eq!(ids(&rows, &shown), ["1a", "2a-1", "2a-0"]);
    assert_eq!(shown.entries[2].children, 1);
}

struct Reads(Rc<RefCell<Vec<String>>>);

impl Peek for Reads {
    fn read(&self, agent: &Subagent) -> Facts {
        self.0.borrow_mut().push(agent.id.clone());
        Facts {
            context: Some(1),
            doing: None,
        }
    }
}

#[test]
fn a_collapsed_group_is_never_glanced() {
    let s = Scratch::new("tree-glance");
    let mut rows = with_children("a", 3);
    for r in &mut rows {
        r.path = s
            .write(&format!("{}.jsonl", r.id), "{}\n")
            .to_string_lossy()
            .into();
    }
    let reads = Rc::new(RefCell::new(Vec::new()));
    let mut peeked = SubagentRows::new(Reads(reads.clone()));
    let mut tree = Tree::default();
    let shown = show(&mut tree, TOP, &rows, &[]);
    let indexes: Vec<usize> = shown.entries.iter().map(|e| e.index).collect();
    peeked.rows(&rows, &indexes);
    assert_eq!(*reads.borrow(), ["a"]);
    reads.borrow_mut().clear();
    let shown = show(&mut tree, TOP, &rows, &["a"]);
    let indexes: Vec<usize> = shown.entries.iter().map(|e| e.index).collect();
    peeked.rows(&rows, &indexes);
    assert_eq!(reads.borrow().len(), 3, "the three children, a is kept");
}

#[test]
fn another_session_starts_closed() {
    let rows = with_children("a", 5);
    let mut tree = Tree::default();
    let open = HashSet::from(["a".to_string()]);
    let more = HashMap::from([("a".to_string(), 1)]);
    assert_eq!(tree.show(Some("s1"), &rows, &open, &more).entries.len(), 6);
    let shown = tree.show(Some("s2"), &rows, &open, &HashMap::new());
    assert_eq!(shown.entries.len(), 4, "the group is paged from its start");
}
