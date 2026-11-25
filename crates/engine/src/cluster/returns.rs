use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::claims::{winner, Claim, SideCopy};
use super::id::NodeId;
use super::members::{Member, Term};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalCopy {
    pub len: u64,
    pub folder_changed: bool,
    pub path: Option<String>,
    pub at_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReturnAction {
    TakeBack,
    Keep,
    ContinueHere,
    WaitFor,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct NoticeAction {
    pub id: ReturnAction,
    pub label: String,
    pub primary: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub kind: String,
    pub text: String,
    pub actions: Vec<NoticeAction>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StepDown {
    pub moved: Vec<(String, String)>,
    pub staying: Vec<String>,
    pub side_copies: Vec<SideCopy>,
    pub notice: Notice,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReturnChoice {
    Continue,
    Wait { for_node: String },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Meet {
    Keep,
    Yield {
        owner: NodeId,
        side_copy: Option<SideCopy>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoverRequest {
    pub via: NodeId,
    pub to: NodeId,
    pub aside: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TakeBackStep {
    MoveAside {
        record: SideCopy,
        from: String,
        to: String,
    },
    Handover(HandoverRequest),
}

fn newest<'a>(claims: &'a [Claim], session: &str) -> Option<&'a Claim> {
    claims
        .iter()
        .filter(|c| c.session == session)
        .reduce(winner)
}

fn primary_back(newer: &Member) -> Notice {
    Notice {
        kind: "primary_back".to_string(),
        text: format!("{} is primary", newer.name),
        actions: vec![
            NoticeAction {
                id: ReturnAction::TakeBack,
                label: "Take Back".to_string(),
                primary: true,
            },
            NoticeAction {
                id: ReturnAction::Keep,
                label: format!("Keep {}", newer.name),
                primary: false,
            },
        ],
    }
}

pub fn on_step_down(
    me: &NodeId,
    newer: &Member,
    mine: &[String],
    winner_claims: &[Claim],
    local: &dyn Fn(&str) -> LocalCopy,
) -> StepDown {
    let mut moved = Vec::new();
    let mut staying = Vec::new();
    let mut side_copies = Vec::new();
    for session in mine {
        let owner = newest(winner_claims, session).filter(|c| c.node != *me);
        let Some(claim) = owner else {
            staying.push(session.clone());
            continue;
        };
        moved.push((session.clone(), claim.node.as_str().to_string()));
        let copy = local(session);
        if copy.len > claim.base_len || copy.folder_changed {
            side_copies.push(SideCopy {
                session: session.clone(),
                node: me.clone(),
                base_set: claim.base_set.clone(),
                extra_bytes: copy.len.saturating_sub(claim.base_len),
                repo_changed: copy.folder_changed,
                path: copy.path,
                at_ms: copy.at_ms,
                hidden: false,
            });
        }
    }
    StepDown {
        moved,
        staying,
        side_copies,
        notice: primary_back(newer),
    }
}

pub fn on_return_with_primary_off(
    me: &NodeId,
    learned: &[Claim],
    mine: &[String],
) -> Vec<(String, ReturnChoice)> {
    let newer = learned
        .iter()
        .map(|c| &c.term)
        .max()
        .map(|t| t.node.clone())
        .filter(|node| node != me);
    let wait_for = |node: Option<&NodeId>| ReturnChoice::Wait {
        for_node: node.map(|n| n.as_str().to_string()).unwrap_or_default(),
    };
    mine.iter()
        .map(|session| {
            let choice = match newest(learned, session) {
                Some(c) if newer.as_ref() == Some(&c.node) && !c.touched => ReturnChoice::Continue,
                Some(c) if c.node != *me => wait_for(Some(&c.node)),
                _ => wait_for(newer.as_ref()),
            };
            (session.clone(), choice)
        })
        .collect()
}

pub fn continue_here(
    session: &str,
    me: &NodeId,
    term: &Term,
    previous: Option<&Claim>,
    base_set: &str,
    base_len: u64,
) -> Claim {
    Claim {
        session: session.to_string(),
        node: me.clone(),
        n: previous.map_or(1, |c| c.n.saturating_add(1)),
        term: term.clone(),
        base_set: base_set.to_string(),
        base_len,
        touched: false,
    }
}

pub fn on_meet(mine: &Claim, theirs: &Claim, my_len: u64) -> Meet {
    if mine.node == theirs.node || winner(mine, theirs) == mine {
        return Meet::Keep;
    }
    let grew = my_len > theirs.base_len;
    let side_copy = (grew || mine.touched).then(|| SideCopy {
        session: mine.session.clone(),
        node: mine.node.clone(),
        base_set: theirs.base_set.clone(),
        extra_bytes: my_len.saturating_sub(theirs.base_len),
        repo_changed: false,
        path: None,
        at_ms: 0,
        hidden: false,
    });
    Meet::Yield {
        owner: theirs.node.clone(),
        side_copy,
    }
}

fn safe_part(part: &str) -> bool {
    !part.is_empty() && part != "." && part != ".." && !part.contains(['/', '\\', '\0'])
}

pub fn copies_dir(data: &Path, session: &str, set: &str) -> Result<PathBuf, String> {
    if !safe_part(session) || !safe_part(set) {
        return Err(format!("{session}/{set} is not a name for a copies folder"));
    }
    Ok(data.join("cluster").join("copies").join(session).join(set))
}

pub fn take_back(
    me: &NodeId,
    primary: &NodeId,
    copies: &[SideCopy],
    data: &Path,
) -> Result<Vec<TakeBackStep>, String> {
    if primary == me {
        return Err("this machine is already primary".to_string());
    }
    let mut steps = Vec::new();
    for copy in copies.iter().filter(|c| c.node == *me) {
        let Some(from) = copy.path.as_deref() else {
            continue;
        };
        let dir = copies_dir(data, &copy.session, &copy.base_set)?;
        let from_path = Path::new(from);
        if from_path.starts_with(&dir) {
            continue;
        }
        let name = from_path
            .file_name()
            .ok_or_else(|| format!("{from} has no file name"))?;
        let to = dir
            .join(name)
            .to_str()
            .ok_or_else(|| "the copies folder is not valid text".to_string())?
            .to_string();
        let mut record = copy.clone();
        record.path = Some(to.clone());
        steps.push(TakeBackStep::MoveAside {
            record,
            from: from.to_string(),
            to,
        });
    }
    steps.push(TakeBackStep::Handover(HandoverRequest {
        via: primary.clone(),
        to: me.clone(),
        aside: true,
    }));
    Ok(steps)
}
