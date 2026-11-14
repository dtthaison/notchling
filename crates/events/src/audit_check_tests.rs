use crate::audit_check::{check, CheckInput, CheckOutcome, CheckResult, CHAIN_STARTED};
use crate::audit_line::{sealed_line, unsealed_line, UnsealedWhy};
use crate::audit_line_tests::{envelope, event, mode};
use crate::event::Event;
use crate::field::List;
use crate::field::Seal;
use crate::kinds::security_log::{AuditChainStarted, ChainReason};
use crate::spec::Outcome;

const KEY: [u8; 32] = [9; 32];

struct Chain {
    lines: Vec<String>,
    seals: Vec<Seal>,
}

fn chain_with(key: &[u8; 32], from_n: u64, count: u64, prev: Option<Seal>) -> Chain {
    let mut lines = Vec::new();
    let mut seals: Vec<Seal> = Vec::new();
    let mut last = prev;
    for n in from_n..from_n + count {
        let (line, seal) = sealed_line(key, n, last.as_ref(), &envelope(n), &event(), &mode());
        lines.push(line);
        seals.push(seal.clone());
        last = Some(seal);
    }
    Chain { lines, seals }
}

fn chain(count: u64) -> Chain {
    chain_with(&KEY, 1, count, None)
}

fn text(lines: &[String]) -> String {
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn run(
    lines: &[String],
    anchor: Option<&Seal>,
    checkpoint: Option<(u64, &Seal)>,
    key: Option<&[u8; 32]>,
) -> CheckOutcome {
    let body = text(lines);
    let months = [(202611u32, Some(body.as_str()))];
    check(&CheckInput {
        key,
        months: &months,
        anchor,
        checkpoint,
        from_n: None,
    })
}

fn chain_started(key: &[u8; 32], n: u64) -> (String, Seal) {
    let event = Event::new(
        &AuditChainStarted {
            reason: ChainReason::User,
            previous_head_n: None,
            previous_head_seal: None,
            legacy_files: None,
            legacy_lines: None,
            legacy_sha: List::new(vec![]),
        },
        Outcome::Ok,
        1,
    );
    assert_eq!(event.spec.name, CHAIN_STARTED);
    sealed_line(key, n, None, &envelope(n), &event, &mode())
}

#[test]
fn intact_chain() {
    let c = chain(5);
    let out = run(&c.lines, None, Some((5, &c.seals[4])), Some(&KEY));
    assert_eq!(out.result, CheckResult::Intact);
    assert_eq!(out.from_n, 1);
    assert_eq!(out.to_n, 5);
    assert_eq!(out.last_sealed, Some((5, c.seals[4].clone())));
}

#[test]
fn edited_line_is_broken() {
    let mut c = chain(5);
    c.lines[2] = c.lines[2].replacen("\"alpha\":true", "\"alpha\":false", 1);
    let out = run(&c.lines, None, None, Some(&KEY));
    assert_eq!(out.result, CheckResult::Broken);
    assert_eq!(out.first_bad_n, Some(3));
}

#[test]
fn inserted_line_is_broken() {
    let mut c = chain(5);
    let (forged, _) = sealed_line(&[1; 32], 4, None, &envelope(4), &event(), &mode());
    c.lines.insert(3, forged);
    let out = run(&c.lines, None, None, Some(&KEY));
    assert_eq!(out.result, CheckResult::Broken);
}

#[test]
fn deleted_line_is_gap() {
    let mut c = chain(5);
    c.lines.remove(2);
    let out = run(&c.lines, None, None, Some(&KEY));
    assert_eq!(out.result, CheckResult::Gap);
    assert_eq!(out.first_bad_n, Some(4));
}

#[test]
fn reordered_lines_are_broken() {
    let mut c = chain(5);
    c.lines.swap(2, 3);
    let out = run(&c.lines, None, None, Some(&KEY));
    assert_eq!(out.result, CheckResult::Broken);
}

#[test]
fn cut_before_checkpoint_is_tail_missing() {
    let c = chain(5);
    let out = run(&c.lines[..3], None, Some((5, &c.seals[4])), Some(&KEY));
    assert_eq!(out.result, CheckResult::TailMissing);
}

#[test]
fn cut_after_checkpoint_is_intact() {
    let c = chain(5);
    let out = run(&c.lines[..4], None, Some((3, &c.seals[2])), Some(&KEY));
    assert_eq!(out.result, CheckResult::Intact);
}

#[test]
fn file_ahead_of_checkpoint_is_intact() {
    let c = chain(8);
    let out = run(&c.lines, None, Some((5, &c.seals[4])), Some(&KEY));
    assert_eq!(out.result, CheckResult::Intact);
}

#[test]
fn checkpoint_seal_not_at_its_n_is_tail_missing() {
    let c = chain(5);
    let other = chain_with(&[4; 32], 1, 5, None);
    let out = run(&c.lines, None, Some((4, &other.seals[3])), Some(&KEY));
    assert_eq!(out.result, CheckResult::TailMissing);
}

#[test]
fn month_removed_without_anchor_is_file_missing() {
    let c = chain(6);
    let later = text(&c.lines[3..]);
    let months = [(202610u32, None), (202611u32, Some(later.as_str()))];
    let out = check(&CheckInput {
        key: Some(&KEY),
        months: &months,
        anchor: None,
        checkpoint: None,
        from_n: None,
    });
    assert_eq!(out.result, CheckResult::FileMissing);
}

#[test]
fn month_removed_with_anchor_is_intact() {
    let c = chain(6);
    let later = text(&c.lines[3..]);
    let months = [(202611u32, Some(later.as_str()))];
    let out = check(&CheckInput {
        key: Some(&KEY),
        months: &months,
        anchor: Some(&c.seals[2]),
        checkpoint: None,
        from_n: None,
    });
    assert_eq!(out.result, CheckResult::Intact);
    assert_eq!(out.from_n, 4);
}

#[test]
fn wrong_anchor_is_unanchored() {
    let c = chain(6);
    let later = text(&c.lines[3..]);
    let months = [(202611u32, Some(later.as_str()))];
    let out = check(&CheckInput {
        key: Some(&KEY),
        months: &months,
        anchor: Some(&c.seals[0]),
        checkpoint: None,
        from_n: None,
    });
    assert_eq!(out.result, CheckResult::Unanchored);
    assert_eq!(out.first_bad_n, Some(4));
}

#[test]
fn unsealed_tail_is_unsealed() {
    let mut c = chain(3);
    c.lines.push(unsealed_line(
        4,
        UnsealedWhy::KeyMissing,
        &envelope(4),
        &event(),
        &mode(),
    ));
    c.lines.push(unsealed_line(
        5,
        UnsealedWhy::KeyMissing,
        &envelope(5),
        &event(),
        &mode(),
    ));
    let out = run(&c.lines, None, None, Some(&KEY));
    assert_eq!(out.result, CheckResult::Unsealed);
    assert_eq!(out.unsealed_stretch, Some((4, 5)));
}

#[test]
fn unsealed_stretch_then_sealed_is_resumed() {
    let c = chain(3);
    let mut lines = c.lines.clone();
    for n in 4..=5 {
        lines.push(unsealed_line(
            n,
            UnsealedWhy::KeyWaiting,
            &envelope(n),
            &event(),
            &mode(),
        ));
    }
    let (next, _) = sealed_line(&KEY, 6, Some(&c.seals[2]), &envelope(6), &event(), &mode());
    lines.push(next);
    let out = run(&lines, None, None, Some(&KEY));
    assert_eq!(out.result, CheckResult::Resumed);
    assert_eq!(out.unsealed_stretch, Some((4, 5)));
    assert_eq!(out.to_n, 6);
}

#[test]
fn new_chain_after_chain_started_is_intact() {
    let old = chain_with(&[1; 32], 1, 3, None);
    let (started, started_seal) = chain_started(&KEY, 4);
    let fresh = chain_with(&KEY, 5, 3, Some(started_seal));
    let mut lines = old.lines.clone();
    lines.push(started);
    lines.extend(fresh.lines.clone());
    let out = run(&lines, None, None, Some(&KEY));
    assert_eq!(out.result, CheckResult::Intact);
    assert_eq!(out.to_n, 7);
}

#[test]
fn wrong_key_is_broken() {
    let c = chain(4);
    let out = run(&c.lines, None, None, Some(&[1; 32]));
    assert_eq!(out.result, CheckResult::Broken);
    assert_eq!(out.first_bad_n, Some(1));
}

#[test]
fn unparseable_line_is_broken() {
    let mut c = chain(4);
    c.lines[1] = "{\"n\":2,".to_string();
    let out = run(&c.lines, None, None, Some(&KEY));
    assert_eq!(out.result, CheckResult::Broken);
}

#[test]
fn result_names_match_the_registry_choice() {
    use crate::kinds::security_log::CheckResult as Choice;
    for result in [
        CheckResult::Intact,
        CheckResult::Broken,
        CheckResult::Gap,
        CheckResult::TailMissing,
        CheckResult::FileMissing,
        CheckResult::Unanchored,
        CheckResult::Unsealed,
        CheckResult::Resumed,
    ] {
        assert!(
            Choice::from_name(result.name()).is_some(),
            "{}",
            result.name()
        );
    }
}
