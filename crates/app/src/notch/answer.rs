use notchling_engine::transcript::snapshot::PendingQuestion;
use notchling_locale as t;

use crate::acting::{Answer, Decision};

pub fn permission_choices(can_always_allow: bool) -> Vec<(Decision, &'static str, &'static str)> {
    let mut choices = vec![(
        Decision::Allow,
        t::consent::notch_allow(),
        t::consent::notch_allow_detail(),
    )];
    if can_always_allow {
        choices.push((
            Decision::Always,
            t::consent::notch_always(),
            t::consent::notch_always_detail(),
        ));
    }
    choices.push((
        Decision::Deny,
        t::consent::notch_deny(),
        t::consent::notch_deny_detail(),
    ));
    choices
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Pick {
    #[default]
    None,
    Options(Vec<usize>),
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    pub title: String,
    pub answered: bool,
    pub selected: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuestionDraft {
    tool_use_id: String,
    pub tab: usize,
    picks: Vec<Pick>,
    typed: Vec<String>,
}

impl QuestionDraft {
    pub fn new(q: &PendingQuestion) -> QuestionDraft {
        QuestionDraft {
            tool_use_id: q.tool_use_id.clone(),
            tab: 0,
            picks: vec![Pick::None; q.items.len()],
            typed: vec![String::new(); q.items.len()],
        }
    }

    pub fn follow(&mut self, q: &PendingQuestion) {
        if self.tool_use_id != q.tool_use_id || self.picks.len() != q.items.len() {
            *self = QuestionDraft::new(q);
        }
    }

    pub fn is_for(&self, q: &PendingQuestion) -> bool {
        self.tool_use_id == q.tool_use_id
    }

    pub fn tabs(&self, q: &PendingQuestion) -> Vec<Tab> {
        q.items
            .iter()
            .enumerate()
            .map(|(i, item)| Tab {
                title: if item.header.trim().is_empty() {
                    t::notch::question_tab(i + 1)
                } else {
                    item.header.clone()
                },
                answered: self.answered(i),
                selected: i == self.tab,
            })
            .collect()
    }

    pub fn select_tab(&mut self, tab: usize) {
        if tab < self.picks.len() {
            self.tab = tab;
        }
    }

    pub fn answered(&self, tab: usize) -> bool {
        match self.picks.get(tab) {
            Some(Pick::Options(picked)) => !picked.is_empty(),
            Some(Pick::Text(_)) => true,
            Some(Pick::None) | None => false,
        }
    }

    pub fn is_picked(&self, tab: usize, option: usize) -> bool {
        matches!(self.picks.get(tab), Some(Pick::Options(p)) if p.contains(&option))
    }

    pub fn text_answer(&self, tab: usize) -> Option<&str> {
        match self.picks.get(tab) {
            Some(Pick::Text(t)) => Some(t),
            _ => None,
        }
    }

    pub fn choose(&mut self, q: &PendingQuestion, option: usize) -> Option<Vec<Answer>> {
        let item = q.items.get(self.tab)?;
        if option == 0 || option > item.options.len() {
            return None;
        }
        if item.multi_select {
            let mut picked = match std::mem::take(&mut self.picks[self.tab]) {
                Pick::Options(p) => p,
                Pick::None | Pick::Text(_) => Vec::new(),
            };
            if let Some(at) = picked.iter().position(|p| *p == option) {
                picked.remove(at);
            } else {
                picked.push(option);
                picked.sort_unstable();
            }
            self.picks[self.tab] = Pick::Options(picked);
            return None;
        }
        self.picks[self.tab] = Pick::Options(vec![option]);
        if q.items.len() == 1 {
            return self.answers(q);
        }
        self.advance();
        None
    }

    pub fn typed(&self, tab: usize) -> &str {
        self.typed.get(tab).map(String::as_str).unwrap_or("")
    }

    pub fn typed_mut(&mut self) -> Option<&mut String> {
        self.typed.get_mut(self.tab)
    }

    pub fn submit_text(&mut self, q: &PendingQuestion) -> Option<Vec<Answer>> {
        let text = self.typed.get(self.tab)?.trim().to_owned();
        if text.is_empty() || self.tab >= q.items.len() {
            return None;
        }
        self.picks[self.tab] = Pick::Text(text);
        self.typed[self.tab].clear();
        if q.items.len() == 1 {
            return self.answers(q);
        }
        self.advance();
        None
    }

    pub fn next(&mut self) {
        self.advance();
    }

    fn advance(&mut self) {
        let n = self.picks.len();
        if let Some(next) = (1..=n)
            .map(|step| (self.tab + step) % n)
            .find(|&t| !self.answered(t))
        {
            self.tab = next;
        }
    }

    pub fn complete(&self) -> bool {
        !self.picks.is_empty() && (0..self.picks.len()).all(|t| self.answered(t))
    }

    pub fn answers(&self, q: &PendingQuestion) -> Option<Vec<Answer>> {
        if !self.is_for(q) || !self.complete() {
            return None;
        }
        self.picks
            .iter()
            .map(|p| match p {
                Pick::Options(picked) => Some(Answer::Options(picked.clone())),
                Pick::Text(text) => Some(Answer::Text(text.clone())),
                Pick::None => None,
            })
            .collect()
    }

    pub fn needs_submit(q: &PendingQuestion) -> bool {
        q.items.len() > 1 || q.items.iter().any(|i| i.multi_select)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Typed {
    Changed,
    Submit,
    Ignored,
}

pub fn type_key(text: &mut String, key: &str, key_char: Option<&str>, modified: bool) -> Typed {
    match key {
        "enter" => Typed::Submit,
        "backspace" => match text.pop() {
            Some(_) => Typed::Changed,
            None => Typed::Ignored,
        },
        _ if modified => Typed::Ignored,
        _ => match key_char {
            Some(c) if !c.is_empty() && !c.chars().any(char::is_control) => {
                text.push_str(c);
                Typed::Changed
            }
            _ => Typed::Ignored,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::transcript::snapshot::QuestionItem;

    #[test]
    fn keys_type_delete_and_submit_but_never_control_characters() {
        let mut t = String::new();
        assert_eq!(type_key(&mut t, "a", Some("a"), false), Typed::Changed);
        assert_eq!(type_key(&mut t, "space", Some(" "), false), Typed::Changed);
        assert_eq!(type_key(&mut t, "é", Some("é"), false), Typed::Changed);
        assert_eq!(t, "a é");
        assert_eq!(type_key(&mut t, "v", Some("v"), true), Typed::Ignored, "⌘V");
        assert_eq!(
            type_key(&mut t, "escape", Some("\u{1b}"), false),
            Typed::Ignored
        );
        assert_eq!(type_key(&mut t, "tab", Some("\t"), false), Typed::Ignored);
        assert_eq!(type_key(&mut t, "left", None, false), Typed::Ignored);
        assert_eq!(type_key(&mut t, "backspace", None, false), Typed::Changed);
        assert_eq!(t, "a ");
        assert_eq!(type_key(&mut t, "enter", Some("\r"), false), Typed::Submit);
        let mut empty = String::new();
        assert_eq!(
            type_key(&mut empty, "backspace", None, false),
            Typed::Ignored
        );
    }

    fn item(header: &str, options: &[&str], multi: bool) -> QuestionItem {
        QuestionItem {
            header: header.into(),
            question: format!("{header}?"),
            options: options.iter().map(|o| o.to_string()).collect(),
            details: vec![String::new(); options.len()],
            multi_select: multi,
        }
    }

    fn three() -> PendingQuestion {
        PendingQuestion {
            tool_use_id: "hook:q1".into(),
            items: vec![
                item("Database", &["Postgres", "SQLite"], false),
                item("", &["Auth", "Billing", "Search"], true),
                item("Deploy", &["Now", "Later"], false),
            ],
        }
    }

    #[test]
    fn tabs_name_each_question_and_mark_the_answered() {
        let q = three();
        let mut d = QuestionDraft::new(&q);
        let titles: Vec<String> = d.tabs(&q).into_iter().map(|t| t.title).collect();
        assert_eq!(titles, ["Database", "Question 2", "Deploy"]);
        assert!(d.tabs(&q)[0].selected);
        assert_eq!(d.choose(&q, 2), None);
        let tabs = d.tabs(&q);
        assert!(tabs[0].answered && !tabs[1].answered);
        assert!(tabs[1].selected, "a single pick moves on");
    }

    #[test]
    fn multi_select_toggles_and_stays() {
        let q = three();
        let mut d = QuestionDraft::new(&q);
        d.select_tab(1);
        d.choose(&q, 3);
        d.choose(&q, 1);
        assert_eq!(d.tab, 1);
        assert!(d.is_picked(1, 1) && d.is_picked(1, 3) && !d.is_picked(1, 2));
        d.choose(&q, 3);
        assert!(!d.is_picked(1, 3));
        d.choose(&q, 1);
        assert!(!d.answered(1), "nothing picked is no answer");
        d.choose(&q, 2);
        d.next();
        assert_eq!(d.tab, 2, "Next goes to the next unanswered");
        d.choose(&q, 1);
        assert_eq!(d.tab, 0, "wrapping round");
    }

    #[test]
    fn answers_come_in_question_order_once_complete() {
        let q = three();
        let mut d = QuestionDraft::new(&q);
        d.select_tab(2);
        d.choose(&q, 1);
        assert_eq!(d.answers(&q), None);
        d.select_tab(1);
        d.choose(&q, 3);
        d.choose(&q, 1);
        d.select_tab(0);
        d.typed_mut().unwrap().push_str("  MySQL ");
        assert_eq!(d.submit_text(&q), None);
        assert_eq!(d.text_answer(0), Some("MySQL"));
        assert_eq!(d.typed(0), "");
        assert!(d.complete());
        assert_eq!(
            d.answers(&q),
            Some(vec![
                Answer::Text("MySQL".into()),
                Answer::Options(vec![1, 3]),
                Answer::Options(vec![1]),
            ])
        );
        d.select_tab(2);
        d.choose(&q, 2);
        assert_eq!(d.answers(&q).unwrap()[2], Answer::Options(vec![2]));
    }

    #[test]
    fn a_lone_single_question_sends_on_its_pick() {
        let q = PendingQuestion {
            tool_use_id: "t1".into(),
            items: vec![item("Target", &["Prod", "Staging"], false)],
        };
        assert!(!QuestionDraft::needs_submit(&q));
        assert!(QuestionDraft::needs_submit(&three()));
        let mut d = QuestionDraft::new(&q);
        assert_eq!(d.choose(&q, 9), None, "out of range");
        assert_eq!(d.choose(&q, 0), None);
        assert_eq!(d.choose(&q, 2), Some(vec![Answer::Options(vec![2])]));
        let mut typed = QuestionDraft::new(&q);
        assert_eq!(typed.submit_text(&q), None, "empty text is ignored");
        typed.typed_mut().unwrap().push_str("Local");
        assert_eq!(
            typed.submit_text(&q),
            Some(vec![Answer::Text("Local".into())])
        );
    }

    #[test]
    fn a_new_question_starts_over() {
        let q = three();
        let mut d = QuestionDraft::new(&q);
        d.choose(&q, 1);
        d.follow(&q);
        assert!(d.answered(0), "the same question keeps its picks");
        let other = PendingQuestion {
            tool_use_id: "hook:q2".into(),
            ..three()
        };
        d.follow(&other);
        assert!(!d.answered(0) && d.tab == 0);
        assert_eq!(
            d.answers(&q),
            None,
            "answers only for the question it holds"
        );
    }
}
