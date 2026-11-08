//! Answers to an AskUserQuestion, as keys for its terminal page or as the answers a hook releases
//! it with (`ReplySender.keys`, `pageKeys`, `answerKeys`, `hookAnswers`).
//!
//! The terminal shows one question at a time. A digit picks a single-select option and moves on;
//! on a multi-select page digits tick options and Tab moves on. The number after the last option is
//! "Type something": that number, the text, Enter. Several questions (or any multi-select) end on a
//! review screen, where `1` submits.

use super::keys::{ENTER, SUBMIT, TAB};
pub use crate::transcript::snapshot::{PendingQuestion, QuestionItem};
use serde_json::Value;
use std::collections::BTreeMap;

/// One question's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// A single-select option, 1-based.
    Option(usize),
    /// Typed on the "Type something" row.
    Text(String),
    /// Multi-select options, 1-based.
    Options(Vec<usize>),
}

/// Free text as it is typed: newlines become spaces (Enter would submit), and other control
/// characters are dropped, since this text is typed as keys, not pasted (an Esc would cancel the
/// dialog, a `\r` would submit half of it).
pub fn typed_text(text: &str) -> String {
    text.replace("\r\n", " ")
        .replace('\n', " ")
        .chars()
        .filter(|c| !c.is_control())
        .collect()
}

/// `ReplySender.keys(for:in:)`: one question's keys, one chunk per key press. The multi-select walk
/// here (Tab count+1 times, then Enter) is the older one the host terminals still use.
pub fn keys_for(choice: &Choice, item: &QuestionItem) -> Vec<String> {
    match choice {
        Choice::Option(n) => vec![n.to_string()],
        Choice::Text(t) => vec![
            (item.options.len() + 1).to_string(),
            typed_text(t),
            ENTER.to_string(),
        ],
        Choice::Options(numbers) => {
            let mut keys: Vec<String> = numbers.iter().map(usize::to_string).collect();
            keys.extend(std::iter::repeat_n(TAB.to_string(), item.options.len() + 1));
            keys.push(ENTER.to_string());
            keys
        }
    }
}

/// `ReplySender.pageKeys`: the keys that answer one page of a background job's dialog (2.1.319).
/// On a multi-select page digits tick and one Tab moves on.
pub fn page_keys(choice: &Choice, item: &QuestionItem) -> Vec<String> {
    match choice {
        Choice::Options(numbers) => {
            let mut keys: Vec<String> = numbers.iter().map(usize::to_string).collect();
            keys.push(TAB.to_string());
            keys
        }
        other => keys_for(other, item),
    }
}

/// Whether the dialog ends on the review screen: several questions, or any multi-select.
pub fn has_review(question: &PendingQuestion) -> bool {
    question.items.len() > 1 || question.items.iter().any(|i| i.multi_select)
}

/// `ReplySender.answerKeys`: every key of an answer typed blind (host terminals, the phone).
pub fn answer_keys(choices: &[Choice], question: &PendingQuestion) -> Vec<String> {
    let mut keys: Vec<String> = question
        .items
        .iter()
        .zip(choices)
        .flat_map(|(item, choice)| keys_for(choice, item))
        .collect();
    if has_review(question) {
        keys.push(SUBMIT.to_string());
    }
    keys
}

/// `ReplySender.hookAnswers`: the answers a hook-held question is released with, keyed by question
/// text. Multi-select labels are joined with ", " (Claude Code does the same). An option out of
/// range is skipped, and a question left with nothing is left out.
pub fn hook_answers(choices: &[Choice], question: &PendingQuestion) -> BTreeMap<String, String> {
    let mut answers = BTreeMap::new();
    for (item, choice) in question.items.iter().zip(choices) {
        let label = |n: usize| n.checked_sub(1).and_then(|i| item.options.get(i)).cloned();
        let answer = match choice {
            Choice::Option(n) => label(*n),
            Choice::Text(t) => Some(t.clone()),
            Choice::Options(numbers) => {
                let labels: Vec<String> = numbers.iter().filter_map(|n| label(*n)).collect();
                (!labels.is_empty()).then(|| labels.join(", "))
            }
        };
        if let Some(answer) = answer {
            answers.insert(item.question.clone(), answer);
        }
    }
    answers
}

/// Reads the local API's `answers` (`[[int] | string]`, one per question, options 1-based) against
/// the question's items: a string is typed text, a list on a multi-select page ticks those options,
/// and a list of one on a single-select page picks it.
pub fn choices_from_json(answers: &Value, items: &[QuestionItem]) -> Result<Vec<Choice>, String> {
    let list = answers
        .as_array()
        .ok_or("answers must be a list, one per question")?;
    if list.len() != items.len() {
        return Err(format!(
            "{} answers for {} questions",
            list.len(),
            items.len()
        ));
    }
    list.iter()
        .zip(items)
        .map(|(answer, item)| match answer {
            Value::String(text) => Ok(Choice::Text(text.clone())),
            Value::Array(numbers) => {
                let numbers = numbers
                    .iter()
                    .map(|n| match n.as_u64() {
                        Some(n) if n >= 1 && n as usize <= item.options.len() => Ok(n as usize),
                        _ => Err(format!("{n} is not an option of \"{}\"", item.question)),
                    })
                    .collect::<Result<Vec<usize>, String>>()?;
                match (item.multi_select, numbers.as_slice()) {
                    (true, [_, ..]) => Ok(Choice::Options(numbers)),
                    (false, [one]) => Ok(Choice::Option(*one)),
                    _ => Err(format!(
                        "\"{}\" takes {}",
                        item.question,
                        if item.multi_select {
                            "one or more options"
                        } else {
                            "one option"
                        }
                    )),
                }
            }
            _ => Err("each answer is a list of option numbers or a text".into()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn single() -> QuestionItem {
        QuestionItem {
            header: "Color".into(),
            question: "Pick a color".into(),
            options: vec!["Red".into(), "Green".into(), "Blue".into()],
            details: vec![],
            multi_select: false,
        }
    }

    fn multi() -> QuestionItem {
        QuestionItem {
            header: "Pets".into(),
            question: "Pick pets".into(),
            options: vec!["Cat".into(), "Dog".into(), "Fish".into(), "Bird".into()],
            details: vec![],
            multi_select: true,
        }
    }

    fn q(items: Vec<QuestionItem>) -> PendingQuestion {
        PendingQuestion {
            tool_use_id: "t".into(),
            items,
        }
    }

    fn s(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|k| k.to_string()).collect()
    }

    // HostReplyTests, key for key.
    #[test]
    fn answer_keys_for_a_single_select_question() {
        assert_eq!(
            answer_keys(&[Choice::Option(2)], &q(vec![single()])),
            s(&["2"])
        );
    }

    #[test]
    fn answer_keys_for_a_multi_select_question_tab_to_submit_then_review() {
        assert_eq!(
            answer_keys(&[Choice::Options(vec![1, 3])], &q(vec![multi()])),
            s(&["1", "3", "\t", "\t", "\t", "\t", "\t", "\r", "1"])
        );
    }

    #[test]
    fn answer_keys_for_several_questions() {
        let mut want = s(&["2", "1", "3"]);
        want.extend(s(&["\t"; 5]));
        want.extend(s(&["\r", "1"]));
        assert_eq!(
            answer_keys(
                &[Choice::Option(2), Choice::Options(vec![1, 3])],
                &q(vec![single(), multi()])
            ),
            want
        );
    }

    #[test]
    fn answer_keys_for_typed_text_flatten_newlines() {
        assert_eq!(
            answer_keys(&[Choice::Text("hi\nthere".into())], &q(vec![single()])),
            s(&["4", "hi there", "\r"])
        );
    }

    #[test]
    fn hook_answers_join_multi_select_and_skip_out_of_range() {
        let answers = hook_answers(
            &[Choice::Option(2), Choice::Options(vec![1, 3, 9])],
            &q(vec![single(), multi()]),
        );
        assert_eq!(answers["Pick a color"], "Green");
        assert_eq!(answers["Pick pets"], "Cat, Fish");
        assert!(hook_answers(&[Choice::Option(7)], &q(vec![single()])).is_empty());
        assert!(hook_answers(&[Choice::Option(0)], &q(vec![single()])).is_empty());
    }

    // AskDialogTests.testKeysForAReadQuestion and testMultiSelectTicksThenTabsOn.
    #[test]
    fn keys_for_a_read_question() {
        let mut item = single();
        item.options.truncate(2);
        assert_eq!(keys_for(&Choice::Option(2), &item), s(&["2"]));
        assert_eq!(
            keys_for(&Choice::Text("teal".into()), &item),
            s(&["3", "teal", "\r"])
        );
    }

    #[test]
    fn multi_select_ticks_then_tabs_on() {
        let mut pets = multi();
        pets.options.truncate(3);
        assert_eq!(
            page_keys(&Choice::Options(vec![1, 3]), &pets),
            s(&["1", "3", "\t"])
        );
        assert_eq!(page_keys(&Choice::Option(2), &pets), s(&["2"]));
        assert_eq!(
            page_keys(&Choice::Text("x".into()), &pets),
            s(&["4", "x", "\r"])
        );
    }

    #[test]
    fn typed_text_cannot_carry_keys() {
        assert_eq!(typed_text("a\x1b[201~b\rc\x03d\u{9b}e"), "a[201~bcde");
        assert_eq!(typed_text("x\r\ny"), "x y");
    }

    #[test]
    fn json_answers_follow_each_page() {
        let items = [single(), multi()];
        assert_eq!(
            choices_from_json(&json!([[2], [1, 3]]), &items),
            Ok(vec![Choice::Option(2), Choice::Options(vec![1, 3])])
        );
        assert_eq!(
            choices_from_json(&json!(["teal", [4]]), &items),
            Ok(vec![Choice::Text("teal".into()), Choice::Options(vec![4])])
        );
        assert!(choices_from_json(&json!([[1, 2], [1]]), &items).is_err());
        assert!(choices_from_json(&json!([[4], [1]]), &items).is_err());
        assert!(choices_from_json(&json!([[0], [1]]), &items).is_err());
        assert!(choices_from_json(&json!([[1]]), &items).is_err());
        assert!(choices_from_json(&json!([[1], []]), &items).is_err());
        assert!(choices_from_json(&json!({"a": 1}), &items).is_err());
        assert!(choices_from_json(&json!([1, [1]]), &items).is_err());
    }
}
