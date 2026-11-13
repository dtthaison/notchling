use super::exec::{Failure, Pty};
use super::keys::{timing::*, LEFT, RIGHT};
use crate::terminal::ask_dialog::{self, Page};
use crate::transcript::snapshot::{PendingQuestion, QuestionItem};
use std::time::Duration;

pub const NEXT_PAGE_MS: u64 = 2_500;
pub const BACK_MS: u64 = 2_500;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

pub fn read_question(pty: &mut dyn Pty, job: &str) -> Result<Option<PendingQuestion>, Failure> {
    if !pty.wait_quiet(ms(ATTACH_QUIET_MS), ms(ATTACH_TIMEOUT_MS)) {
        return Err(Failure::Timeout);
    }
    let lines = pty.screen();
    if !super::keys::dialog_open(&lines) {
        return Ok(None);
    }
    let Some(first) = ask_dialog::page(&lines).filter(|p| !p.on_review) else {
        return Ok(None);
    };
    let total = first.tabs.len().max(1);
    let mut pages = vec![first];
    let mut moved = 0;
    while pages.len() < total {
        let before = pages[pages.len() - 1].question.clone();
        write(pty, RIGHT)?;
        moved += 1;
        match next_page(pty, &before) {
            Some(page) => pages.push(page),
            None => break,
        }
    }
    if moved > 0 {
        for _ in 0..moved {
            write(pty, LEFT)?;
            pty.sleep(ms(KEY_PAUSE_MS));
        }
        let question = pages[0].question.clone();
        wait(pty, BACK_MS, |lines| ask_dialog::shows(&question, lines));
    }
    pty.wait_quiet(ms(DETACH_QUIET_MS), ms(DETACH_TIMEOUT_MS));
    if pages.len() < total {
        return Ok(None);
    }
    Ok(Some(question_from(job, &pages)))
}

fn write(pty: &mut dyn Pty, keys: &str) -> Result<(), Failure> {
    pty.write(keys.as_bytes())
        .map_err(|e| Failure::Pty(e.to_string()))
}

fn next_page(pty: &mut dyn Pty, before: &str) -> Option<Page> {
    let mut found = None;
    wait(pty, NEXT_PAGE_MS, |lines| {
        found = ask_dialog::page(lines).filter(|p| !p.on_review && p.question != before);
        found.is_some()
    });
    found
}

fn wait(pty: &mut dyn Pty, timeout_ms: u64, mut done: impl FnMut(&[String]) -> bool) -> bool {
    let deadline = pty.now_ms() + timeout_ms;
    loop {
        pty.wait_quiet(ms(WAIT_QUIET_MS), ms(WAIT_STEP_MS));
        if done(&pty.screen()) {
            return true;
        }
        if pty.now_ms() >= deadline {
            return false;
        }
    }
}

pub fn question_from(job: &str, pages: &[Page]) -> PendingQuestion {
    let tabs = pages.first().map(|p| p.tabs.clone()).unwrap_or_default();
    let items: Vec<QuestionItem> = pages
        .iter()
        .enumerate()
        .map(|(i, page)| QuestionItem {
            header: tabs.get(i).cloned().unwrap_or_else(|| page.header.clone()),
            question: page.question.clone(),
            options: page.options.clone(),
            details: page.details.clone(),
            multi_select: page.multi_select,
        })
        .collect();
    let joined: Vec<&str> = items.iter().map(|i| i.question.as_str()).collect();
    PendingQuestion {
        tool_use_id: format!("screen:{job}:{}", joined.join("|")),
        items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tabs {
        pages: Vec<Vec<String>>,
        at: usize,
        clock: u64,
        writes: Vec<String>,
    }

    impl Tabs {
        fn new(pages: Vec<Vec<String>>) -> Tabs {
            Tabs {
                pages,
                at: 0,
                clock: 0,
                writes: Vec::new(),
            }
        }
    }

    impl Pty for Tabs {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
            let keys = String::from_utf8_lossy(bytes).into_owned();
            if keys == RIGHT {
                self.at = (self.at + 1).min(self.pages.len() - 1);
            } else if keys == LEFT {
                self.at = self.at.saturating_sub(1);
            }
            self.writes.push(keys);
            Ok(())
        }
        fn wait_quiet(&mut self, quiet: Duration, _timeout: Duration) -> bool {
            self.clock += quiet.as_millis() as u64;
            true
        }
        fn screen(&mut self) -> Vec<String> {
            self.pages[self.at].clone()
        }
        fn sleep(&mut self, duration: Duration) {
            self.clock += duration.as_millis() as u64;
        }
        fn now_ms(&self) -> u64 {
            self.clock
        }
    }

    fn page(tabs: Option<&str>, question: &str, options: &[&str]) -> Vec<String> {
        let mut v = vec!["────────────────────────────".to_string()];
        if let Some(t) = tabs {
            v.push(t.to_string());
        }
        v.push(question.to_string());
        for (i, o) in options.iter().enumerate() {
            let mark = if i == 0 { "❯" } else { " " };
            v.push(format!("{mark} {}. {o}", i + 1));
        }
        v.push(format!("  {}. Type something.", options.len() + 1));
        v.push("────────────────────────────".to_string());
        v.push(format!("  {}. Chat about this", options.len() + 2));
        v.push("Enter to select · ↑/↓ to navigate · Esc to cancel".to_string());
        v
    }

    #[test]
    fn a_single_question_is_read_without_a_key() {
        let screen = page(
            None,
            "How should I install it?",
            &["Side by side (Recommended)", "Full replace now"],
        );
        let mut pty = Tabs::new(vec![screen]);
        let q = read_question(&mut pty, "job1")
            .unwrap()
            .expect("a question");
        assert!(pty.writes.is_empty(), "{:?}", pty.writes);
        assert_eq!(q.items.len(), 1);
        assert_eq!(q.items[0].question, "How should I install it?");
        assert_eq!(
            q.items[0].options,
            ["Side by side (Recommended)", "Full replace now"]
        );
        assert_eq!(q.tool_use_id, "screen:job1:How should I install it?");
    }

    #[test]
    fn several_questions_are_read_tab_by_tab_and_put_back() {
        let tabs = "←  ☐ Color  ☐ Fruit  ✔ Submit  →";
        let mut pty = Tabs::new(vec![
            page(Some(tabs), "Favourite color?", &["Red", "Blue"]),
            page(Some(tabs), "Favourite fruit?", &["Apple", "Pear", "Plum"]),
        ]);
        let q = read_question(&mut pty, "j").unwrap().expect("a question");
        assert_eq!(q.items.len(), 2);
        assert_eq!(q.items[0].header, "Color");
        assert_eq!(q.items[1].header, "Fruit");
        assert_eq!(q.items[1].options, ["Apple", "Pear", "Plum"]);
        assert_eq!(pty.writes, [RIGHT, LEFT]);
        assert_eq!(pty.at, 0, "back on the first question");
    }

    #[test]
    fn no_dialog_is_no_question() {
        let mut pty = Tabs::new(vec![vec!["> ".into(), "  ? for shortcuts".into()]]);
        assert_eq!(read_question(&mut pty, "j").unwrap(), None);
        let mut pty = Tabs::new(vec![vec![
            "Bash command".into(),
            " Do you want to proceed?".into(),
            " 1. Yes".into(),
            " 2. No".into(),
            "Esc to cancel".into(),
        ]]);
        assert_eq!(read_question(&mut pty, "j").unwrap(), None);
        assert!(pty.writes.is_empty());
    }

    #[test]
    fn a_tab_that_never_moves_reads_nothing_and_goes_back() {
        let tabs = "←  ☐ Color  ☐ Fruit  ✔ Submit  →";
        let mut pty = Tabs::new(vec![page(Some(tabs), "Favourite color?", &["Red", "Blue"])]);
        assert_eq!(read_question(&mut pty, "j").unwrap(), None);
        assert_eq!(pty.writes, [RIGHT, LEFT]);
    }
}
