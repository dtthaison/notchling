use crate::audit_line::{read_head, seal_of, signed_part, LineHead};
use crate::field::Seal;

pub const CHAIN_STARTED: &str = "audit.chain_started";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckResult {
    Intact,
    Broken,
    Gap,
    TailMissing,
    FileMissing,
    Unanchored,
    Unsealed,
    Resumed,
}

impl CheckResult {
    pub fn name(self) -> &'static str {
        match self {
            CheckResult::Intact => "intact",
            CheckResult::Broken => "broken",
            CheckResult::Gap => "gap",
            CheckResult::TailMissing => "tail_missing",
            CheckResult::FileMissing => "file_missing",
            CheckResult::Unanchored => "unanchored",
            CheckResult::Unsealed => "unsealed",
            CheckResult::Resumed => "resumed",
        }
    }

    pub fn is_warning(self) -> bool {
        !matches!(self, CheckResult::Intact | CheckResult::Resumed)
    }
}

pub struct CheckInput<'a> {
    pub key: Option<&'a [u8; 32]>,
    pub months: &'a [(u32, Option<&'a str>)],
    pub anchor: Option<&'a Seal>,
    pub checkpoint: Option<(u64, &'a Seal)>,
    pub from_n: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckOutcome {
    pub result: CheckResult,
    pub from_n: u64,
    pub to_n: u64,
    pub first_bad_n: Option<u64>,
    pub last_sealed: Option<(u64, Seal)>,
    pub unsealed_stretch: Option<(u64, u64)>,
}

#[derive(Default)]
struct Found {
    broken: Option<u64>,
    gap: Option<u64>,
    file_missing: bool,
    tail_missing: Option<u64>,
    unanchored: Option<u64>,
}

struct Line<'a> {
    text: &'a str,
    head: Option<LineHead>,
}

fn precedence(found: &Found, unsealed_at_end: bool, resumed: bool) -> (CheckResult, Option<u64>) {
    if let Some(n) = found.broken {
        return (CheckResult::Broken, Some(n));
    }
    if let Some(n) = found.gap {
        return (CheckResult::Gap, Some(n));
    }
    if found.file_missing {
        return (CheckResult::FileMissing, None);
    }
    if let Some(n) = found.tail_missing {
        return (CheckResult::TailMissing, Some(n));
    }
    if let Some(n) = found.unanchored {
        return (CheckResult::Unanchored, Some(n));
    }
    if unsealed_at_end {
        return (CheckResult::Unsealed, None);
    }
    if resumed {
        return (CheckResult::Resumed, None);
    }
    (CheckResult::Intact, None)
}

fn first(slot: &mut Option<u64>, n: u64) {
    if slot.is_none() {
        *slot = Some(n);
    }
}

pub fn check(input: &CheckInput) -> CheckOutcome {
    let mut found = Found::default();
    let mut lines: Vec<Line> = Vec::new();
    for (_, text) in input.months {
        match text {
            None => found.file_missing = true,
            Some(text) => {
                for line in text.lines().filter(|l| !l.trim().is_empty()) {
                    lines.push(Line {
                        text: line,
                        head: read_head(line),
                    });
                }
            }
        }
    }

    let chain_from = lines
        .iter()
        .rposition(|l| {
            l.head
                .as_ref()
                .is_some_and(|h| h.kind == CHAIN_STARTED && h.prev.is_none() && h.seal.is_some())
        })
        .unwrap_or(0);

    let mut last_n: Option<u64> = None;
    let mut first_n: Option<u64> = None;
    let mut last_sealed: Option<(u64, Seal)> = None;
    let mut unsealed_start: Option<u64> = None;
    let mut unsealed_end: Option<u64> = None;
    let mut resumed: Option<(u64, u64)> = None;
    let mut seen_sealed_in_chain = false;

    for (index, line) in lines.iter().enumerate() {
        let Some(head) = &line.head else {
            let n = last_n.map_or(0, |n| n + 1);
            first(&mut found.broken, n);
            continue;
        };
        first_n.get_or_insert(head.n);
        let mut skipped = false;
        match last_n {
            Some(previous) if head.n == previous + 1 => {}
            Some(previous) if head.n > previous + 1 => {
                first(&mut found.gap, head.n);
                skipped = true;
            }
            Some(_) => first(&mut found.broken, head.n),
            None => {}
        }
        last_n = Some(head.n);
        if let Some((cn, cseal)) = input.checkpoint {
            if head.n == cn && head.seal.as_ref() != Some(cseal) {
                first(&mut found.tail_missing, cn);
            }
        }
        if index < chain_from {
            continue;
        }
        match &head.seal {
            None => {
                unsealed_start.get_or_insert(head.n);
                unsealed_end = Some(head.n);
            }
            Some(seal) => {
                if let Some(key) = input.key {
                    let expected = signed_part(line.text).map(|part| seal_of(key, part.as_bytes()));
                    if expected.as_ref() != Some(seal) {
                        first(&mut found.broken, head.n);
                    }
                }
                match (&head.prev, &last_sealed) {
                    (None, None) => {}
                    (None, Some(_)) => {
                        if head.kind != CHAIN_STARTED {
                            first(&mut found.broken, head.n);
                        }
                    }
                    (Some(prev), Some((_, last))) => {
                        if prev != last && !skipped {
                            first(&mut found.broken, head.n);
                        }
                    }
                    (Some(prev), None) => {
                        if !seen_sealed_in_chain {
                            match input.anchor {
                                Some(anchor) if prev == anchor => {}
                                _ => first(&mut found.unanchored, head.n),
                            }
                        }
                    }
                }
                seen_sealed_in_chain = true;
                if let (Some(start), Some(end)) = (unsealed_start, unsealed_end) {
                    resumed = Some((start, end));
                }
                unsealed_start = None;
                unsealed_end = None;
                last_sealed = Some((head.n, seal.clone()));
            }
        }
    }

    let to_n = last_n.unwrap_or(0);
    if let Some((cn, _)) = input.checkpoint {
        let below_range = first_n.is_some_and(|f| cn < f);
        if !below_range && last_n.is_none_or(|last| last < cn) {
            first(&mut found.tail_missing, cn);
        }
    }

    let unsealed_at_end = unsealed_start.is_some();
    let (result, first_bad_n) = precedence(&found, unsealed_at_end, resumed.is_some());
    let unsealed_stretch = match (unsealed_start, unsealed_end) {
        (Some(start), Some(end)) => Some((start, end)),
        _ => resumed,
    };
    CheckOutcome {
        result,
        from_n: input.from_n.or(first_n).unwrap_or(0),
        to_n,
        first_bad_n,
        last_sealed,
        unsealed_stretch,
    }
}
