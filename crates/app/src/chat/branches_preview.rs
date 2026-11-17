use std::collections::HashMap;
use std::sync::Arc;

use super::branch_list::{Branch, BranchList, Lines, PanelInput, Tab, WorktreeOf};
use super::worktrees::{Occupancy, Open, Work, PREVIEW_NOW_MS};

const MINUTE_MS: i64 = 60_000;
const ROOT: &str = "/Users/me/work/notchling";
const BASE: &str = "agent-run-oss";

pub const STATES: [&str; 7] = [
    "working",
    "not-merged",
    "merged",
    "open-row",
    "loading",
    "error",
    "empty",
];

pub const OPEN_KEY: &str = "refs/heads/agent/ask";

fn ago(minutes: i64) -> Option<i64> {
    Some(PREVIEW_NOW_MS - minutes * MINUTE_MS)
}

fn lane_path(name: &str) -> String {
    format!("{ROOT}/.claude/worktrees/{name}")
}

fn work(added: u64, removed: u64, files: u64, uncommitted: u64) -> Option<Work> {
    Some(Work {
        added,
        removed,
        files,
        uncommitted_files: uncommitted,
        changed_at: None,
        read_at: PREVIEW_NOW_MS,
    })
}

fn local(name: &str) -> Branch {
    Branch {
        name: Some(name.to_owned()),
        reference: Some(format!("refs/heads/{name}")),
        local: true,
        base: Some(BASE.to_owned()),
        ..Branch::default()
    }
}

fn remote(name: &str) -> Branch {
    Branch {
        name: Some(name.to_owned()),
        reference: Some(format!("refs/remotes/origin/{name}")),
        remote: Some("origin".to_owned()),
        remote_only: true,
        on_github: true,
        base: Some(BASE.to_owned()),
        ..Branch::default()
    }
}

fn lane(name: &str, branch: &str, ahead: u64, work: Option<Work>, minutes: i64) -> Branch {
    let path = lane_path(name);
    Branch {
        worktree: Some(WorktreeOf {
            real: path.clone(),
            path,
            work,
            ..WorktreeOf::default()
        }),
        committed_at: ago(minutes),
        ahead: Some(ahead),
        behind: Some(0),
        ..local(branch)
    }
}

fn with_lines(mut b: Branch, added: u64, removed: u64, files: u64) -> Branch {
    b.lines = Some(Lines {
        added,
        removed,
        files,
    });
    b
}

fn merged(mut b: Branch, minutes: i64, subject: &str) -> Branch {
    b.merged_into = Some(BASE.to_owned());
    b.ahead = Some(0);
    b.committed_at = ago(minutes);
    b.subject = Some(subject.to_owned());
    b
}

fn list() -> BranchList {
    let mut current = lane("", "agent/details-git", 3, work(501, 91, 40, 5), 2);
    if let Some(w) = current.worktree.as_mut() {
        w.path = ROOT.to_owned();
        w.real = ROOT.to_owned();
        w.main = true;
        w.current = true;
    }
    current.upstream = Some("origin/agent/details-git".to_owned());
    current.subject = Some("details: one line per worktree".to_owned());
    let mut ask = lane("ask", "agent/ask", 2, work(1240, 38, 23, 0), 6);
    ask.subject = Some("ask: store and wait".to_owned());
    ask.upstream = Some("origin/agent/ask".to_owned());
    let mut ssh = with_lines(lane("ssh-join", "agent/ssh-join", 4, None, 3), 860, 12, 17);
    ssh.subject = Some("ssh join: invite code".to_owned());
    let mut old = lane("old", "agent/old-lane", 0, None, 0);
    if let Some(w) = old.worktree.as_mut() {
        w.missing = true;
    }
    old.committed_at = None;
    let mut main = local("main");
    main.is_base = true;
    main.upstream = Some("origin/main".to_owned());
    let mut integration = local(BASE);
    integration.is_base = true;
    integration.upstream = Some("origin/agent-run-oss".to_owned());
    let mut held = with_lines(remote("agent/held-reply-fix"), 42, 7, 3);
    held.committed_at = ago(60 * 24 * 9);
    held.subject = Some("held replies keep their text".to_owned());
    let mut outbox = with_lines(local("agent/outbox-ui"), 310, 24, 9);
    outbox.committed_at = ago(60 * 5);
    outbox.subject = Some("outbox: show each state".to_owned());
    outbox.ahead = Some(5);
    let mut others = vec![
        merged(
            local("agent/fix-busy-unsure"),
            60 * 30,
            "busy: unsure after a stall",
        ),
        merged(
            remote("agent/copy-link-menu"),
            60 * 50,
            "chat: Copy Link in the menu",
        ),
        merged(
            remote("agent/fix-sending-echo"),
            60 * 70,
            "sending: no echo",
        ),
    ];
    let mut branches = vec![current, ask, ssh, old, main, integration, held, outbox];
    branches.append(&mut others);
    BranchList {
        name: "notchling".to_owned(),
        default: Some("origin/main".to_owned()),
        integration: Some(BASE.to_owned()),
        fetched_at: ago(120),
        truncated: false,
        pending: 0,
        branches,
    }
}

fn occupancy() -> Vec<(String, Option<Occupancy>)> {
    let running = |minutes: i64, id: &str| {
        Some(Occupancy::Running {
            since_ms: PREVIEW_NOW_MS - minutes * MINUTE_MS,
            open: Open::Session(id.to_owned()),
            more: 0,
        })
    };
    vec![
        (lane_path("ask"), running(41, "preview-ask")),
        (lane_path("ssh-join"), running(44, "preview-ssh")),
    ]
}

fn names() -> HashMap<String, String> {
    HashMap::from([
        ("preview-ask".to_owned(), "Lane: build Ask".to_owned()),
        ("preview-ssh".to_owned(), "Lane: build SSH join".to_owned()),
    ])
}

pub fn preview(state: &str) -> PanelInput {
    let base = PanelInput {
        minute_ms: PREVIEW_NOW_MS,
        ..PanelInput::default()
    };
    match state {
        "loading" => base,
        "error" => PanelInput {
            failed: Some("git took too long to answer".to_owned()),
            ..base
        },
        "empty" => {
            let mut empty = list();
            empty.branches.clear();
            PanelInput {
                list: Some(Arc::new(empty)),
                ..base
            }
        }
        _ => PanelInput {
            list: Some(Arc::new(list())),
            occupancy: occupancy(),
            session_names: names(),
            ..base
        },
    }
}

pub fn preview_tab(state: &str) -> Tab {
    match state {
        "not-merged" => Tab::NotMerged,
        "merged" => Tab::Merged,
        _ => Tab::Working,
    }
}

pub fn preview_open(state: &str) -> Option<&'static str> {
    (state == "open-row").then_some(OPEN_KEY)
}
