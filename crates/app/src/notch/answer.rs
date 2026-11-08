//! Answering a question from the notch, pure: one tab per question, a pick per tab (an option for
//! single select, a set for multi select, or typed text), and the answers `session.answer` takes
//! once every tab has one. The Swift card walked the questions in order (`choices`, Back); here
//! the tabs let you go back to any of them, and the Claude Code terminal's review step is the
//! Submit button.

use notchling_engine::transcript::snapshot::PendingQuestion;

use crate::acting::Answer;

/// What one question has so far.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Pick {
    #[default]
    None,
    /// 1-based, in the order the options are listed.
    Options(Vec<usize>),
    Text(String),
}

/// A tab's title and state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    pub title: String,
    pub answered: bool,
    pub selected: bool,
}

/// The answers being put together for one question.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuestionDraft {
    tool_use_id: String,
    /// The tab on show.
    pub tab: usize,
    picks: Vec<Pick>,
    /// What's typed in each tab's text box.
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

    /// Starts over when the question on show is another one.
    pub fn follow(&mut self, q: &PendingQuestion) {
        if self.tool_use_id != q.tool_use_id || self.picks.len() != q.items.len() {
            *self = QuestionDraft::new(q);
        }
    }

    pub fn is_for(&self, q: &PendingQuestion) -> bool {
        self.tool_use_id == q.tool_use_id
    }

    /// Each question's tab: its header, else "Question n".
    pub fn tabs(&self, q: &PendingQuestion) -> Vec<Tab> {
        q.items
            .iter()
            .enumerate()
            .map(|(i, item)| Tab {
                title: if item.header.trim().is_empty() {
                    format!("Question {}", i + 1)
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

    /// Whether an option (1-based) is picked in a tab.
    pub fn is_picked(&self, tab: usize, option: usize) -> bool {
        matches!(self.picks.get(tab), Some(Pick::Options(p)) if p.contains(&option))
    }

    /// The text a tab was answered with.
    pub fn text_answer(&self, tab: usize) -> Option<&str> {
        match self.picks.get(tab) {
            Some(Pick::Text(t)) => Some(t),
            _ => None,
        }
    }

    /// Picks an option (1-based) in the tab on show. Single select takes it and moves to the next
    /// question still unanswered; multi select toggles it and stays. Returns the answers when this
    /// pick finishes a lone single-select question, which is sent at once as the Swift card did.
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

    /// The tab's text box.
    pub fn typed(&self, tab: usize) -> &str {
        self.typed.get(tab).map(String::as_str).unwrap_or("")
    }

    pub fn typed_mut(&mut self) -> Option<&mut String> {
        self.typed.get_mut(self.tab)
    }

    /// Takes the typed text as the tab's answer and moves on; empty text is ignored. Like a
    /// single pick, it sends a lone question at once.
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

    /// Multi select's Next: on to the next question still unanswered.
    pub fn next(&mut self) {
        self.advance();
    }

    /// The next unanswered tab after this one, wrapping; stays when all are answered.
    fn advance(&mut self) {
        let n = self.picks.len();
        if let Some(next) = (1..=n)
            .map(|step| (self.tab + step) % n)
            .find(|&t| !self.answered(t))
        {
            self.tab = next;
        }
    }

    /// Every question has an answer.
    pub fn complete(&self) -> bool {
        !self.picks.is_empty() && (0..self.picks.len()).all(|t| self.answered(t))
    }

    /// The answers in question order, once complete.
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

    /// Whether the card needs a Submit button: more than one question, or a multi-select one
    /// (a lone single-select question is sent by its pick).
    pub fn needs_submit(q: &PendingQuestion) -> bool {
        q.items.len() > 1 || q.items.iter().any(|i| i.multi_select)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::transcript::snapshot::QuestionItem;

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
        // Changing an answer later is allowed.
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
