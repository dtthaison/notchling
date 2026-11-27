use std::sync::Arc;

use serde_json::Value;

use super::branch_list::{parse_branch_list, BranchList, PanelInput, Tab};
use super::git::Why;
use super::git_gates::SlowGate;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    Same,
    Reload,
    Close,
}

pub fn on_switch(before: Option<&str>, after: Option<&str>) -> Switch {
    match (before, after) {
        (before, after) if before == after => Switch::Same,
        (_, Some(_)) => Switch::Reload,
        (_, None) => Switch::Close,
    }
}

pub fn selection_moved(
    was: Option<&str>,
    now: Option<&str>,
    before: Option<&str>,
    after: Option<&str>,
) -> bool {
    was != now || before != after
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    Same,
    Changed,
    NotRepo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Received {
    Stale,
    Moved,
    Applied(Applied),
}

pub fn hidden_switch(before: Option<&str>, after: Option<&str>, feed: &mut Feed) -> bool {
    if on_switch(before, after) == Switch::Same {
        return false;
    }
    feed.forget();
    true
}

#[derive(Debug, Default)]
pub struct Feed {
    generation: u64,
    gate: SlowGate,
    folder: Option<String>,
    list: Option<Arc<BranchList>>,
    failed: Option<String>,
}

impl Feed {
    pub fn list(&self) -> Option<Arc<BranchList>> {
        self.list.clone()
    }

    pub fn failed(&self) -> Option<String> {
        self.failed.clone()
    }

    pub fn list_for(&self, folder: Option<&str>) -> Option<Arc<BranchList>> {
        self.for_folder(folder).and(self.list.clone())
    }

    pub fn failed_for(&self, folder: Option<&str>) -> Option<String> {
        self.for_folder(folder).and(self.failed.clone())
    }

    fn for_folder(&self, folder: Option<&str>) -> Option<()> {
        (folder.is_some() && self.folder.as_deref() == folder).then_some(())
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn receive(
        &mut self,
        generation: u64,
        folder: &str,
        selected: Option<&str>,
        result: Result<Value, String>,
        now_ms: i64,
    ) -> Received {
        if generation != self.generation {
            return Received::Stale;
        }
        if selected != Some(folder) {
            self.skip(now_ms);
            return Received::Moved;
        }
        Received::Applied(self.apply(folder, result, now_ms))
    }

    pub fn ask(&mut self, why: Why, folder: Option<&str>, visible: bool, now_ms: i64) -> bool {
        self.gate.ask(why, folder, visible, now_ms)
    }

    pub fn skip(&mut self, now_ms: i64) {
        self.gate.done(false, now_ms);
    }

    pub fn forget(&mut self) {
        self.generation += 1;
        self.folder = None;
        self.list = None;
        self.failed = None;
        self.gate = SlowGate::default();
    }

    pub fn show(&mut self, list: BranchList) {
        self.list = Some(Arc::new(list));
        self.failed = None;
    }

    pub fn apply(&mut self, folder: &str, result: Result<Value, String>, now_ms: i64) -> Applied {
        if self.folder.as_deref() != Some(folder) {
            self.list = None;
            self.failed = None;
            self.folder = Some(folder.to_owned());
        }
        match result {
            Ok(reply) => match parse_branch_list(&reply) {
                None => {
                    self.gate.done(false, now_ms);
                    self.list = None;
                    Applied::NotRepo
                }
                Some(parsed) => {
                    self.gate.done(parsed.pending > 0, now_ms);
                    let same = self.list.as_deref() == Some(&parsed) && self.failed.is_none();
                    if same {
                        return Applied::Same;
                    }
                    self.list = Some(Arc::new(parsed));
                    self.failed = None;
                    Applied::Changed
                }
            },
            Err(why) => {
                self.gate.done(false, now_ms);
                if self.failed.as_deref() == Some(why.as_str()) {
                    return Applied::Same;
                }
                self.failed = Some(why);
                Applied::Changed
            }
        }
    }
}

#[derive(Debug)]
pub struct Shown {
    input: Arc<PanelInput>,
    tab: Tab,
    query: String,
    open: Option<String>,
}

impl Default for Shown {
    fn default() -> Self {
        Shown {
            input: Arc::default(),
            tab: Tab::Working,
            query: String::new(),
            open: None,
        }
    }
}

impl Shown {
    pub fn input(&self) -> &Arc<PanelInput> {
        &self.input
    }

    pub fn tab(&self) -> Tab {
        self.tab
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn open(&self) -> Option<&str> {
        self.open.as_deref()
    }

    pub fn set_input(&mut self, input: Arc<PanelInput>) -> bool {
        let keep = input.list.is_none() && input.failed.is_some() && self.input.list.is_some();
        let next = if keep {
            Arc::new(PanelInput {
                list: self.input.list.clone(),
                ..(*input).clone()
            })
        } else {
            input
        };
        if self.input == next {
            return false;
        }
        self.input = next;
        true
    }

    pub fn set_tab(&mut self, tab: Tab) -> bool {
        if self.tab == tab {
            return false;
        }
        self.tab = tab;
        self.open = None;
        true
    }

    pub fn set_query(&mut self, query: &str) -> bool {
        if self.query == query {
            return false;
        }
        self.query = query.to_owned();
        self.open = None;
        true
    }

    pub fn toggle(&mut self, key: &str) {
        if self.open.as_deref() == Some(key) {
            self.open = None;
        } else {
            self.open = Some(key.to_owned());
        }
    }

    pub fn open_row(&mut self, key: &str) {
        self.open = Some(key.to_owned());
    }

    pub fn reset_view(&mut self) -> bool {
        let changed = self.open.is_some() || !self.query.is_empty();
        self.query.clear();
        self.open = None;
        changed
    }

    pub fn switched(&mut self) {
        self.input = Arc::default();
        self.reset_view();
    }
}
