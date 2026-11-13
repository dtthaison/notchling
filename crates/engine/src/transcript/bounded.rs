use super::model::{ChatItem, Conversation, DiffLine, ItemKind, ToolCall};

const ITEM_BYTES: usize = 64 * 1024;
const TEXT_CHARS: usize = 8 * 1024;
const OUTPUT_CHARS: usize = 16 * 1024;
const DIFF_LINES: usize = 200;

impl Conversation {
    pub fn bounded(mut self, max_bytes: usize) -> (Conversation, usize) {
        let items = std::mem::take(&mut self.items);
        let total = items.len();
        let fixed = json_len(&self);
        let budget = max_bytes.saturating_sub(fixed);
        let mut kept: Vec<ChatItem> = Vec::new();
        let mut used = 0;
        for mut item in items.into_iter().rev() {
            let mut size = json_len(&item) + 1;
            if size > ITEM_BYTES {
                shrink(&mut item);
                size = json_len(&item) + 1;
            }
            if used + size > budget && !kept.is_empty() {
                break;
            }
            used += size;
            kept.push(item);
        }
        let omitted = total - kept.len();
        kept.reverse();
        self.items = kept;
        (self, omitted)
    }
}

fn json_len<T: serde::Serialize>(value: &T) -> usize {
    serde_json::to_vec(value).map_or(0, |v| v.len())
}

fn cut(text: &mut String, max_chars: usize) {
    if let Some((at, _)) = text.char_indices().nth(max_chars) {
        text.truncate(at);
        text.push('…');
    }
}

fn shrink(item: &mut ChatItem) {
    match &mut item.kind {
        ItemKind::User { text, .. }
        | ItemKind::Assistant { text }
        | ItemKind::Thinking { text, .. }
        | ItemKind::Notice { text, .. }
        | ItemKind::CommandOutput { text } => cut(text, TEXT_CHARS),
        ItemKind::Tools(calls) => calls.iter_mut().for_each(shrink_call),
        ItemKind::TurnEnd { .. } => {}
    }
}

fn shrink_call(call: &mut ToolCall) {
    cut(&mut call.title, TEXT_CHARS);
    cut(&mut call.input, TEXT_CHARS);
    if let Some(output) = &mut call.output {
        cut(output, OUTPUT_CHARS);
    }
    if call.diff.len() > DIFF_LINES {
        let left = call.diff.len() - DIFF_LINES;
        call.diff.truncate(DIFF_LINES);
        call.diff.push(DiffLine {
            kind: super::model::DiffKind::Hunk,
            text: format!("… {left} more lines not shown"),
            number: None,
        });
    }
    for line in &mut call.diff {
        cut(&mut line.text, 1000);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::model::{DiffKind, ToolKind};

    fn item(n: usize, kind: ItemKind) -> ChatItem {
        ChatItem {
            id: format!("i{n}"),
            kind,
            date: Some(n as f64),
        }
    }

    fn talk(count: usize, size: usize) -> Conversation {
        Conversation {
            items: (0..count)
                .map(|n| {
                    item(
                        n,
                        ItemKind::Assistant {
                            text: format!("{n}:{}", "a".repeat(size)),
                        },
                    )
                })
                .collect(),
            ..Conversation::default()
        }
    }

    #[test]
    fn a_conversation_that_fits_is_unchanged() {
        let c = talk(10, 100);
        let (kept, omitted) = c.clone().bounded(512 * 1024);
        assert_eq!(kept, c);
        assert_eq!(omitted, 0);
    }

    #[test]
    fn a_long_one_keeps_its_newest_items_in_order() {
        let c = talk(1000, 5000);
        let (kept, omitted) = c.bounded(100 * 1024);
        assert!(json_len(&kept) <= 100 * 1024, "{}", json_len(&kept));
        assert!(kept.items.len() >= 15 && kept.items.len() < 25);
        assert_eq!(kept.items.len() + omitted, 1000);
        assert_eq!(kept.items.last().unwrap().id, "i999", "the newest stays");
        let ids: Vec<usize> = kept
            .items
            .iter()
            .map(|i| i.id[1..].parse().unwrap())
            .collect();
        assert!(ids.windows(2).all(|w| w[1] == w[0] + 1), "{ids:?}");
    }

    #[test]
    fn one_enormous_newest_item_stays_but_is_cut() {
        let mut c = talk(3, 100);
        let mut call = ToolCall::new(
            "t",
            "edit_file",
            ToolKind::Edit,
            "big".into(),
            "i".repeat(1_000_000),
        );
        call.output = Some("o".repeat(5_000_000));
        call.diff = (0..100_000)
            .map(|n| DiffLine {
                kind: DiffKind::Add,
                text: format!("line {n}"),
                number: Some(n),
            })
            .collect();
        c.items.push(item(3, ItemKind::Tools(vec![call])));
        let (kept, omitted) = c.bounded(512 * 1024);
        assert!(json_len(&kept) <= 512 * 1024, "{}", json_len(&kept));
        assert_eq!(kept.items.last().unwrap().id, "i3");
        assert_eq!(kept.items.len() + omitted, 4);
        let ItemKind::Tools(calls) = &kept.items.last().unwrap().kind else {
            panic!("a tool item");
        };
        assert_eq!(calls[0].diff.last().unwrap().kind, DiffKind::Hunk);
        assert!(calls[0].input.ends_with('…'));
    }

    #[test]
    fn the_rest_of_the_conversation_is_kept_whole() {
        let mut c = talk(50, 10_000);
        c.activity = Some("Editing".into());
        c.queued = vec!["next".into()];
        c.facts.turns = 7;
        let (kept, omitted) = c.bounded(64 * 1024);
        assert!(omitted > 0);
        assert_eq!(kept.activity.as_deref(), Some("Editing"));
        assert_eq!(kept.queued, ["next"]);
        assert_eq!(kept.facts.turns, 7);
    }
}
