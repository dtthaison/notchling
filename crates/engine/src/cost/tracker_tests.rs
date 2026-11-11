//! Port of `CostTrackerTests` and the tracker half of `CostSourcesTests`, over transcripts in a
//! temp folder. Nothing reads a real transcript folder; the zone is UTC so days don't depend on
//! the machine.

use super::cache::{old_cache_file, Cache, CACHE_DOC, CURRENT};
use super::service::CostService;
use super::summary::{day_key, Window};
use super::tracker::{Ranged, Root, Tracker, ANTIGRAVITY, CLAUDE_CODE, CODEX, DESKTOP};
use crate::catalog::Catalog;
use crate::pages::clock::{civil_at, text};
use crate::sessions::testutil::TempDir;
use crate::store;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

/// 2027-01-15 08:00:00 UTC.
const NOW: f64 = 1_800_000_000.0;
const DAY: f64 = 86_400.0;

fn iso(ts: f64) -> String {
    format!("{}Z", text(civil_at(ts, 0), true).replace(' ', "T"))
}

fn utc(_: f64) -> i64 {
    0
}

/// An assistant line costing `dollars` at Opus 5.5 prices ($20 per million output tokens).
fn usage_line(id: &str, dollars: f64, at: f64) -> String {
    let tokens = (dollars / 20.0 * 1_000_000.0) as u64;
    format!(
        r#"{{"type":"assistant","timestamp":"{}","message":{{"id":"{id}","model":"claude-opus-5-5","usage":{{"output_tokens":{tokens}}}}}}}"#,
        iso(at)
    ) + "\n"
}

fn append(tmp: &TempDir, rel: &str, text: &str) -> PathBuf {
    let path = tmp.path(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("open");
    file.write_all(text.as_bytes()).expect("write");
    path
}

fn catalog() -> Arc<Catalog> {
    Arc::new(Catalog::built_in())
}

fn claude_tracker(tmp: &TempDir) -> Tracker {
    Tracker::new(
        vec![Root {
            dir: tmp.path("projects"),
            channel: CLAUDE_CODE,
        }],
        Some(tmp.path("data")),
        Some(tmp.path("account-log.json")),
        catalog(),
    )
    .with_zone(utc)
}

fn month(r: &Ranged, w: Window) -> f64 {
    r.overall[&w].month
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} vs {b}");
}

#[test]
fn a_first_scan_then_only_what_was_added() {
    let tmp = TempDir::new("cost-first");
    let mut t = claude_tracker(&tmp);
    append(
        &tmp,
        "projects/-tmp-demo/a.jsonl",
        &usage_line("m1", 20.0, NOW),
    );
    assert!(t.refresh(NOW));
    near(t.ranged(NOW).overall[&Window::Days(30)].today, 20.0);

    append(
        &tmp,
        "projects/-tmp-demo/a.jsonl",
        &usage_line("m2", 4.0, NOW),
    );
    append(
        &tmp,
        "projects/-tmp-demo/b.jsonl",
        &usage_line("m3", 6.0, NOW),
    );
    assert!(t.refresh(NOW + 5.0));
    near(t.ranged(NOW).overall[&Window::Days(30)].today, 30.0);
}

#[test]
fn a_message_copied_to_another_transcript_counts_once() {
    let tmp = TempDir::new("cost-dedupe");
    let mut t = claude_tracker(&tmp);
    append(
        &tmp,
        "projects/-tmp-demo/a.jsonl",
        &usage_line("m1", 20.0, NOW),
    );
    t.refresh(NOW);
    // Claude Code copies messages into the new transcript when it compacts or resumes.
    append(
        &tmp,
        "projects/-tmp-demo/resumed.jsonl",
        &usage_line("m1", 20.0, NOW),
    );
    t.refresh(NOW);
    near(t.ranged(NOW).overall[&Window::Days(30)].today, 20.0);
}

#[test]
fn the_request_id_is_part_of_the_key() {
    let tmp = TempDir::new("cost-request");
    let mut t = claude_tracker(&tmp);
    let line = |request: &str| {
        format!(
            r#"{{"type":"assistant","requestId":"{request}","timestamp":"{}","message":{{"id":"m","model":"claude-opus-5-5","usage":{{"output_tokens":1000000}}}}}}"#,
            iso(NOW)
        ) + "\n"
    };
    append(
        &tmp,
        "projects/-p/a.jsonl",
        &(line("r1") + &line("r1") + &line("r2")),
    );
    t.refresh(NOW);
    near(month(&t.ranged(NOW), Window::All), 40.0);
}

#[test]
fn nothing_changed_keeps_the_summary_and_reports_clean() {
    let tmp = TempDir::new("cost-same");
    let mut t = claude_tracker(&tmp);
    append(
        &tmp,
        "projects/-tmp-demo/a.jsonl",
        &usage_line("m1", 20.0, NOW),
    );
    t.refresh(NOW);
    let before = t.ranged(NOW);
    assert!(!t.refresh(NOW + 1.0));
    assert_eq!(t.ranged(NOW), before);
}

#[test]
fn a_line_still_being_written_is_read_when_it_is_whole() {
    let tmp = TempDir::new("cost-partial");
    let mut t = claude_tracker(&tmp);
    let line = usage_line("m1", 20.0, NOW);
    let (head, tail) = line.split_at(line.len() / 2);
    append(&tmp, "projects/-p/a.jsonl", head);
    t.refresh(NOW);
    assert_eq!(month(&t.ranged(NOW), Window::All), 0.0);
    append(&tmp, "projects/-p/a.jsonl", tail);
    t.refresh(NOW);
    near(month(&t.ranged(NOW), Window::All), 20.0);
}

#[test]
fn a_rewritten_shorter_file_is_read_again_without_double_counting() {
    let tmp = TempDir::new("cost-shrink");
    let mut t = claude_tracker(&tmp);
    let path = append(
        &tmp,
        "projects/-p/a.jsonl",
        &(usage_line("m1", 20.0, NOW) + &usage_line("m2", 10.0, NOW)),
    );
    t.refresh(NOW);
    // Shorter than what was read, so it is read from the start; m1 and m2 are already counted.
    std::fs::write(&path, usage_line("m3", 1.0, NOW)).unwrap();
    t.refresh(NOW);
    near(month(&t.ranged(NOW), Window::All), 31.0);
}

#[test]
fn the_cache_is_saved_and_a_new_tracker_reads_it() {
    let tmp = TempDir::new("cost-cache");
    let mut t = claude_tracker(&tmp);
    append(
        &tmp,
        "projects/-tmp-demo/a.jsonl",
        &usage_line("m1", 20.0, NOW),
    );
    t.refresh(NOW);
    t.save_now(NOW);
    let data = tmp.path("data");
    assert!(store::doc(&data, CACHE_DOC).unwrap().is_some());
    assert!(!old_cache_file(&data).exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(store::db_path(&data))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
    let mut again = claude_tracker(&tmp);
    again.refresh(NOW + 10.0);
    near(again.ranged(NOW).overall[&Window::Days(30)].today, 20.0);
    // The cache has the message counted, so a copy of it elsewhere is still not counted twice.
    append(
        &tmp,
        "projects/-tmp-demo/c.jsonl",
        &usage_line("m1", 20.0, NOW),
    );
    again.refresh(NOW + 20.0);
    near(again.ranged(NOW).overall[&Window::Days(30)].today, 20.0);
}

#[test]
fn a_cache_of_another_version_or_a_broken_one_is_read_from_scratch() {
    let tmp = TempDir::new("cost-version");
    append(&tmp, "projects/-p/a.jsonl", &usage_line("m1", 20.0, NOW));
    let data = tmp.path("data");
    for stale in [
        format!(r#"{{"version":{},"files":{{}}}}"#, CURRENT + 1),
        "not json".to_string(),
    ] {
        store::put_doc(&data, CACHE_DOC, &stale).unwrap();
        let mut t = claude_tracker(&tmp);
        t.refresh(NOW);
        near(month(&t.ranged(NOW), Window::All), 20.0);
    }
    assert_eq!(
        Cache::load(&tmp.path("missing"), &catalog()).version,
        CURRENT
    );
}

#[test]
fn the_old_cache_file_moves_in_once_and_a_stale_one_is_read_from_scratch() {
    let tmp = TempDir::new("cost-import");
    let data = tmp.path("data");
    append(&tmp, "projects/-p/a.jsonl", &usage_line("m1", 20.0, NOW));
    let mut first = claude_tracker(&tmp);
    first.refresh(NOW);
    first.save_now(NOW);
    // Put what it saved back where the old file was.
    let saved = store::doc(&data, CACHE_DOC).unwrap().unwrap();
    store::update_doc(&data, CACHE_DOC, |_| Ok(None)).unwrap();
    let old = tmp.write("data/daemon/cost-cache.json", &saved);
    let loaded = Cache::load(&data, &catalog());
    assert!(!old.exists(), "the old file moved in");
    assert!(!loaded.files.is_empty(), "and its offsets came with it");
    assert_eq!(store::doc(&data, CACHE_DOC).unwrap().unwrap(), saved);

    // A file of another version isn't moved in; the next save removes it.
    let tmp = TempDir::new("cost-import-stale");
    let data = tmp.path("data");
    append(&tmp, "projects/-p/a.jsonl", &usage_line("m1", 20.0, NOW));
    let stale = tmp.write(
        "data/daemon/cost-cache.json",
        &format!(r#"{{"version":{},"files":{{}}}}"#, CURRENT + 1),
    );
    assert!(Cache::load(&data, &catalog()).files.is_empty());
    assert!(stale.exists());
    assert_eq!(store::doc(&data, CACHE_DOC).unwrap(), None);
    let mut t = claude_tracker(&tmp);
    t.refresh(NOW);
    near(month(&t.ranged(NOW), Window::All), 20.0);
    assert!(!stale.exists(), "the first save replaces it");
    assert!(store::doc(&data, CACHE_DOC).unwrap().is_some());
}

#[test]
fn windows_cover_the_last_7_and_30_days_and_everything() {
    let tmp = TempDir::new("cost-windows");
    let mut t = claude_tracker(&tmp);
    append(
        &tmp,
        "projects/-p/a.jsonl",
        &(usage_line("today", 1.0, NOW)
            + &usage_line("six", 2.0, NOW - 6.0 * DAY)
            + &usage_line("ten", 4.0, NOW - 10.0 * DAY)
            + &usage_line("fifty", 8.0, NOW - 50.0 * DAY)),
    );
    t.refresh(NOW);
    let r = t.ranged(NOW);
    near(month(&r, Window::Days(7)), 3.0);
    near(month(&r, Window::Days(30)), 7.0);
    near(month(&r, Window::All), 15.0);
    let thirty = &r.overall[&Window::Days(30)];
    near(thirty.today, 1.0);
    near(thirty.week, 3.0);
    assert_eq!(thirty.days.len(), 30);
    assert_eq!(
        thirty.days.last().map(|d| d.day.clone()),
        Some(day_key(t.day_number(NOW)))
    );
    assert_eq!(r.overall[&Window::Days(7)].days.len(), 7);
    assert_eq!(r.overall[&Window::All].days.len(), 30);
}

#[test]
fn the_cli_account_comes_from_the_switch_log() {
    let tmp = TempDir::new("cost-log");
    // Foundation dates count from 2001-01-01.
    let reference = NOW - 978_307_200.0;
    std::fs::write(
        tmp.path("account-log.json"),
        format!(
            r#"[{{"at":{},"id":"acct-a"}},{{"at":{},"id":"acct-b"}}]"#,
            reference - 3.0 * DAY,
            reference - 1.0 * DAY
        ),
    )
    .unwrap();
    let mut t = claude_tracker(&tmp);
    append(
        &tmp,
        "projects/-p/a.jsonl",
        &(usage_line("before", 1.0, NOW - 5.0 * DAY)
            + &usage_line("a", 2.0, NOW - 2.0 * DAY)
            + &usage_line("b", 4.0, NOW)),
    );
    t.refresh(NOW);
    let r = t.ranged(NOW);
    let by = &r.by_account[&Window::All];
    near(by[""].month, 1.0);
    near(by["acct-a"].month, 2.0);
    near(by["acct-b"].month, 4.0);
    let accounts: Vec<_> = r.overall[&Window::All]
        .accounts
        .iter()
        .map(|a| a.id.as_str())
        .collect();
    assert_eq!(accounts, ["acct-b", "acct-a", ""]);
}

#[test]
fn a_desktop_transcript_carries_its_account_in_the_path() {
    let tmp = TempDir::new("cost-desktop");
    let account = "8f1d2c3b-aaaa-4bbb-8ccc-0123456789ab";
    let org = "11112222-3333-4444-5555-666677778888";
    append(
        &tmp,
        &format!("desktop/local-agent-mode-sessions/{account}/{org}/proj/a.jsonl"),
        &usage_line("d1", 3.0, NOW),
    );
    let mut t = Tracker::new(
        vec![Root {
            dir: tmp.path("desktop/local-agent-mode-sessions"),
            channel: DESKTOP,
        }],
        None,
        None,
        catalog(),
    )
    .with_zone(utc);
    t.refresh(NOW);
    let r = t.ranged(NOW);
    near(
        r.by_account[&Window::All][&format!("{account}:{org}")].month,
        3.0,
    );
    assert_eq!(r.overall[&Window::All].channels[0].name, "Desktop");
}

#[test]
fn per_model_token_totals_and_projects() {
    let tmp = TempDir::new("cost-models");
    let line = format!(
        r#"{{"type":"assistant","cwd":"/w/app","timestamp":"{}","message":{{"id":"m","model":"claude-opus-5-5[1m]","usage":{{"input_tokens":1000,"output_tokens":2000,"cache_read_input_tokens":3000,"cache_creation_input_tokens":4000}}}}}}"#,
        iso(NOW)
    ) + "\n";
    let synthetic = format!(
        r#"{{"type":"assistant","timestamp":"{}","message":{{"id":"s","model":"<synthetic>","usage":{{"output_tokens":99}}}}}}"#,
        iso(NOW)
    ) + "\n";
    let unpriced = format!(
        r#"{{"type":"assistant","timestamp":"{}","message":{{"id":"u","model":"claude-unknown-1","usage":{{"output_tokens":99}}}}}}"#,
        iso(NOW)
    ) + "\n";
    append(
        &tmp,
        "projects/-w-app/a.jsonl",
        &(line + &synthetic + &unpriced + "garbage\n"),
    );
    let mut t = claude_tracker(&tmp);
    t.refresh(NOW);
    let s = &t.ranged(NOW).overall[&Window::All];
    assert_eq!(s.projects.len(), 1);
    assert_eq!(
        s.projects[0].name, "app",
        "the message's cwd names the project"
    );
    let detail = &s.model_detail;
    // The synthetic message is skipped; the unpriced model keeps its tokens at $0, after the priced
    // one, and is listed.
    assert_eq!(detail.len(), 2, "{detail:?}");
    assert_eq!(detail[1].name, "Unknown 1");
    assert_eq!((detail[1].cost, detail[1].output), (0.0, 99.0));
    assert_eq!(s.unpriced, ["claude-unknown-1"]);
    assert_eq!(detail[0].name, "Opus 5.5");
    assert_eq!(
        (
            detail[0].input,
            detail[0].output,
            detail[0].cache_read,
            detail[0].cache_write
        ),
        (1000.0, 2000.0, 3000.0, 4000.0)
    );
    assert!((detail[0].percent - 100.0).abs() < 1e-9);
}

#[test]
fn projects_come_from_the_cwd_else_the_folder() {
    let tmp = TempDir::new("cost-projects");
    let with_cwd = format!(
        r#"{{"type":"assistant","cwd":"/w/app","timestamp":"{}","message":{{"id":"a","model":"claude-opus-5-5","usage":{{"output_tokens":1000000}}}}}}"#,
        iso(NOW)
    ) + "\n";
    append(
        &tmp,
        "projects/-w-other/a.jsonl",
        &(with_cwd + &usage_line("b", 5.0, NOW)),
    );
    let mut t = claude_tracker(&tmp);
    t.refresh(NOW);
    let names: Vec<_> = t.ranged(NOW).overall[&Window::All]
        .projects
        .iter()
        .map(|p| (p.name.clone(), p.cost))
        .collect();
    assert_eq!(
        names,
        [("app".to_string(), 20.0), ("other".to_string(), 5.0)]
    );
}

fn other_tracker(tmp: &TempDir) -> Tracker {
    Tracker::new(
        vec![
            Root {
                dir: tmp.path("codex"),
                channel: CODEX,
            },
            Root {
                dir: tmp.path("brain"),
                channel: ANTIGRAVITY,
            },
        ],
        None,
        None,
        catalog(),
    )
    .with_zone(utc)
}

const CODEX_ROLLOUT: &str = include_str!("fixtures/codex-rollout.jsonl");
const AGY_TRANSCRIPT: &str = include_str!("fixtures/antigravity-transcript.jsonl");
const ROLLOUT: &str = "codex/2026/11/06/rollout-2026-11-06T07-49-29-abc.jsonl";

fn other_files(tmp: &TempDir) {
    tmp.write(ROLLOUT, CODEX_ROLLOUT);
    tmp.write("codex/2026/11/06/rollout-old.jsonl.zst", "not json");
    tmp.write(
        "brain/c1/.system_generated/logs/transcript.jsonl",
        AGY_TRANSCRIPT,
    );
    tmp.write(
        "brain/c1/.system_generated/logs/transcript_full.jsonl",
        "not read",
    );
}

#[test]
fn codex_and_antigravity_channels() {
    let tmp = TempDir::new("cost-others");
    other_files(&tmp);
    let mut t = other_tracker(&tmp);
    t.refresh(NOW);
    let r = t.ranged(NOW);
    let all = &r.overall[&Window::All];
    // Two requests of gpt-5.1: (20000 in, 100 out) and (5000 in, 20000 cached, 200 out); the
    // repeat is dropped.
    let codex = all
        .channels
        .iter()
        .find(|c| c.name == "Codex")
        .expect("codex");
    near(codex.cost, 0.026 + 0.01075);
    assert_eq!(all.channel_tokens["Codex"], 20_100.0 + 25_200.0);
    // Gemini 4 Argon has no price: tokens only.
    let agy = all
        .channels
        .iter()
        .find(|c| c.name == "Antigravity CLI")
        .expect("agy");
    assert_eq!(agy.cost, 0.0);
    assert_eq!(
        all.channel_tokens["Antigravity CLI"],
        8472.0 + 248.0 + 2567.0 + 6418.0 + 365.0
    );
    let gemini = all
        .model_detail
        .iter()
        .find(|m| m.name == "Gemini 4 Argon")
        .expect("gemini");
    assert_eq!(gemini.input, 8472.0 + 2567.0);
    assert_eq!(gemini.cache_read, 6418.0);
    assert_eq!(all.projects[0].name, "demo");
    near(
        r.by_account[&Window::All]["user-AAA:acct-111"].month,
        0.026 + 0.01075,
    );
}

#[test]
fn appended_codex_lines_keep_the_model_and_skip_a_repeat() {
    let tmp = TempDir::new("cost-codex-append");
    other_files(&tmp);
    let mut t = other_tracker(&tmp);
    t.refresh(NOW);
    let again = r#"{"timestamp":"2026-11-06T00:51:00.000Z","ordinal":30,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":45300},"last_token_usage":{"input_tokens":25000,"cached_input_tokens":20000,"output_tokens":200,"total_tokens":25200}}}}"#;
    let next = r#"{"timestamp":"2026-11-06T00:52:00.000Z","ordinal":31,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":55300},"last_token_usage":{"input_tokens":10000,"cached_input_tokens":0,"output_tokens":0,"total_tokens":10000}}}}"#;
    append(&tmp, ROLLOUT, &format!("{again}\n{next}\n"));
    t.refresh(NOW + 5.0);
    let codex = t.ranged(NOW).overall[&Window::All]
        .channels
        .iter()
        .find(|c| c.name == "Codex")
        .map(|c| c.cost)
        .expect("codex");
    // Only `next` is new, priced as gpt-5.1 from the turn_context read in the first pass.
    near(codex, 0.026 + 0.01075 + 0.0125);
}

#[test]
fn the_codex_model_survives_a_restart_through_the_cache() {
    let tmp = TempDir::new("cost-codex-cache");
    other_files(&tmp);
    let build = || {
        Tracker::new(
            vec![Root {
                dir: tmp.path("codex"),
                channel: CODEX,
            }],
            Some(tmp.path("data")),
            None,
            catalog(),
        )
        .with_zone(utc)
    };
    let mut t = build();
    t.refresh(NOW);
    t.save_now(NOW);
    let next = r#"{"timestamp":"2026-11-06T00:52:00.000Z","ordinal":31,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":55300},"last_token_usage":{"input_tokens":10000,"cached_input_tokens":0,"output_tokens":0,"total_tokens":10000}}}}"#;
    append(&tmp, ROLLOUT, &format!("{next}\n"));
    let mut again = build();
    again.refresh(NOW + 5.0);
    near(
        month(&again.ranged(NOW), Window::All),
        0.026 + 0.01075 + 0.0125,
    );
}

#[test]
fn a_file_that_vanished_is_forgotten() {
    let tmp = TempDir::new("cost-gone");
    let mut t = claude_tracker(&tmp);
    let path = append(&tmp, "projects/-p/a.jsonl", &usage_line("m1", 1.0, NOW));
    assert!(t.refresh(NOW));
    std::fs::remove_file(path).unwrap();
    assert!(t.refresh(NOW + 1.0), "forgetting the file is a change");
    assert!(!t.refresh(NOW + 2.0));
    near(month(&t.ranged(NOW), Window::All), 1.0);
}

#[test]
fn days_older_than_400_are_forgotten() {
    let tmp = TempDir::new("cost-prune");
    let mut t = claude_tracker(&tmp);
    append(
        &tmp,
        "projects/-p/a.jsonl",
        &(usage_line("old", 5.0, NOW - 401.0 * DAY) + &usage_line("new", 1.0, NOW)),
    );
    t.refresh(NOW);
    near(month(&t.ranged(NOW), Window::All), 1.0);
}

#[test]
fn the_service_answers_at_once_and_scans_on_a_thread() {
    let tmp = TempDir::new("cost-service");
    append(&tmp, "projects/-p/a.jsonl", &usage_line("m1", 20.0, NOW));
    let service = CostService::new(claude_tracker(&tmp));
    let first = service.request(NOW);
    assert_eq!(
        first.scanned, None,
        "nothing was published before the first scan"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let done = loop {
        let snap = service.snapshot();
        if snap.scanned.is_some() {
            break snap;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the scan did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    near(done.summary(Window::All).expect("all").month, 20.0);
    // A new line is seen by the next scan, and a request within 20 s starts none.
    append(&tmp, "projects/-p/a.jsonl", &usage_line("m2", 5.0, NOW));
    service.request(NOW + 5.0);
    std::thread::sleep(std::time::Duration::from_millis(100));
    near(
        service.snapshot().summary(Window::All).expect("all").month,
        20.0,
    );
    service.refresh(NOW + 30.0);
    near(
        service.snapshot().summary(Window::All).expect("all").month,
        25.0,
    );
    assert!(!service.snapshot().scanning);
}

/// An assistant line for any model with the given usage object.
fn model_line(id: &str, model: &str, usage: &str, at: f64) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"{}","message":{{"id":"{id}","model":"{model}","usage":{usage}}}}}"#,
        iso(at)
    ) + "\n"
}

#[test]
fn an_unpriced_claude_model_counts_tokens_and_is_listed() {
    let tmp = TempDir::new("cost-unpriced");
    append(
        &tmp,
        "projects/-p/a.jsonl",
        &model_line("m1", "claude-new-9", r#"{"output_tokens":1000}"#, NOW),
    );
    let mut t = claude_tracker(&tmp);
    t.refresh(NOW);
    let r = t.ranged(NOW);
    let s = &r.overall[&Window::All];
    near(s.month, 0.0);
    assert_eq!(s.unpriced, ["claude-new-9"]);
    assert!(s.borrowed.is_empty());
    assert_eq!(s.model_detail.len(), 1, "its tokens are counted");
    near(s.model_detail[0].output, 1000.0);
    assert_eq!(s.channel_tokens[CLAUDE_CODE], 1000.0);
    // A priced model alone lists nothing.
    let tmp = TempDir::new("cost-priced");
    append(&tmp, "projects/-p/a.jsonl", &usage_line("m1", 1.0, NOW));
    let mut t = claude_tracker(&tmp);
    t.refresh(NOW);
    assert!(t.ranged(NOW).overall[&Window::All].unpriced.is_empty());
}

#[test]
fn a_newer_family_model_is_priced_as_borrowed() {
    let tmp = TempDir::new("cost-borrowed");
    append(
        &tmp,
        "projects/-p/a.jsonl",
        &model_line(
            "m1",
            "claude-fable-5-3",
            r#"{"output_tokens":1000000}"#,
            NOW,
        ),
    );
    let mut t = claude_tracker(&tmp);
    t.refresh(NOW);
    let r = t.ranged(NOW);
    let s = &r.overall[&Window::All];
    // Fable 5.2: $40 per million output tokens.
    near(s.month, 40.0);
    assert_eq!(s.borrowed, ["claude-fable-5-3"]);
    assert!(s.unpriced.is_empty());
}

#[test]
fn a_changed_price_reprices_days_already_counted() {
    let tmp = TempDir::new("cost-reprice");
    append(
        &tmp,
        "projects/-p/a.jsonl",
        &model_line(
            "m1",
            "claude-opus-5-5",
            r#"{"input_tokens":1000000,"output_tokens":1000000}"#,
            NOW,
        ),
    );
    let mut first = claude_tracker(&tmp);
    first.refresh(NOW);
    first.save_now(NOW);
    near(month(&first.ranged(NOW), Window::All), 4.0 + 20.0);

    // Opus 5.5's input price fixed to $8 in an override file.
    let file = tmp.write(
        "catalog.json",
        r#"{"claude": {"prices": [{"match": "opus-5-5", "input": 8.0, "output": 20.0, "cache_read": 0.2}]}}"#,
    );
    let mut second = Tracker::new(
        vec![Root {
            dir: tmp.path("projects"),
            channel: CLAUDE_CODE,
        }],
        Some(tmp.path("data")),
        Some(tmp.path("account-log.json")),
        Arc::new(Catalog::load(&file)),
    )
    .with_zone(utc);
    second.refresh(NOW + 1.0);
    near(month(&second.ranged(NOW), Window::All), 8.0 + 20.0);

    // Back on the built-in prices it is repriced again, and nothing is counted twice.
    let mut third = claude_tracker(&tmp);
    third.refresh(NOW + 2.0);
    near(month(&third.ranged(NOW), Window::All), 4.0 + 20.0);
}

/// A Claude Code tracker over `tmp`'s transcripts and cache, pricing with `catalog`.
fn tracker_with(tmp: &TempDir, catalog: Catalog) -> Tracker {
    Tracker::new(
        vec![Root {
            dir: tmp.path("projects"),
            channel: CLAUDE_CODE,
        }],
        Some(tmp.path("data")),
        Some(tmp.path("account-log.json")),
        Arc::new(catalog),
    )
    .with_zone(utc)
}

/// A catalog with the override file `body` laid over the built-in one.
fn overridden(tmp: &TempDir, body: &str) -> Catalog {
    Catalog::load(&tmp.write("catalog.json", body))
}

/// $20 counted forty days ago in a transcript that is then deleted, and $2 today in one that stays.
fn old_day_gone(tmp: &TempDir) {
    let old = append(
        tmp,
        "projects/-p/old.jsonl",
        &usage_line("m-old", 20.0, NOW - 40.0 * DAY),
    );
    append(tmp, "projects/-p/new.jsonl", &usage_line("m-new", 2.0, NOW));
    let mut first = claude_tracker(tmp);
    first.refresh(NOW);
    first.save_now(NOW);
    near(month(&first.ranged(NOW), Window::All), 22.0);
    std::fs::remove_file(old).unwrap();
    let mut again = claude_tracker(tmp);
    again.refresh(NOW + 1.0);
    again.save_now(NOW + 1.0);
    near(month(&again.ranged(NOW), Window::All), 22.0);
}

#[test]
fn a_price_added_for_a_model_used_nowhere_keeps_days_whose_transcripts_are_gone() {
    let tmp = TempDir::new("cost-unrelated-price");
    old_day_gone(&tmp);
    let catalog = overridden(
        &tmp,
        r#"{"claude": {"prices": [{"match": "fable-5-3", "input": 9.0, "output": 45.0, "cache_read": 0.2}]}}"#,
    );
    let mut t = tracker_with(&tmp, catalog);
    t.refresh(NOW + 2.0);
    near(month(&t.ranged(NOW), Window::All), 22.0);
    t.save_now(NOW + 2.0);
    let mut back = claude_tracker(&tmp);
    back.refresh(NOW + 3.0);
    near(month(&back.ranged(NOW), Window::All), 22.0);
}

#[test]
fn a_changed_price_reprices_a_day_whose_transcript_is_gone_from_its_tokens() {
    let tmp = TempDir::new("cost-reprice-gone");
    old_day_gone(&tmp);
    // Opus 5.5's output price doubled: today's request is read again, the old day is priced
    // again from the tokens the cache kept.
    let doubled = overridden(
        &tmp,
        r#"{"claude": {"prices": [{"match": "opus-5-5", "input": 4.0, "output": 40.0, "cache_read": 0.2}]}}"#,
    );
    let mut t = tracker_with(&tmp, doubled);
    t.refresh(NOW + 2.0);
    t.save_now(NOW + 2.0);
    let r = t.ranged(NOW);
    near(month(&r, Window::All), 44.0);
    near(r.overall[&Window::All].today, 4.0);
    // Back on the built-in price, every day is as it was.
    let mut back = claude_tracker(&tmp);
    back.refresh(NOW + 3.0);
    near(month(&back.ranged(NOW), Window::All), 22.0);
}

#[test]
fn a_model_priced_later_is_priced_from_its_tokens_when_its_transcript_is_gone() {
    let tmp = TempDir::new("cost-priced-later");
    let old = append(
        &tmp,
        "projects/-p/a.jsonl",
        &model_line(
            "m1",
            "claude-new-9",
            r#"{"output_tokens":1000000}"#,
            NOW - 40.0 * DAY,
        ),
    );
    let mut first = claude_tracker(&tmp);
    first.refresh(NOW);
    first.save_now(NOW);
    assert_eq!(
        first.ranged(NOW).overall[&Window::All].unpriced,
        ["claude-new-9"]
    );
    std::fs::remove_file(old).unwrap();
    let priced = overridden(
        &tmp,
        r#"{"claude": {"prices": [{"match": "new-9", "input": 1.0, "output": 10.0, "cache_read": 0.1}]}}"#,
    );
    let mut t = tracker_with(&tmp, priced);
    t.refresh(NOW + 1.0);
    let r = t.ranged(NOW);
    let s = &r.overall[&Window::All];
    near(s.month, 10.0);
    assert!(s.unpriced.is_empty());
}

#[test]
fn a_price_of_zero_still_counts_the_request() {
    let tmp = TempDir::new("cost-zero-price");
    append(&tmp, "projects/-p/a.jsonl", &usage_line("m1", 1.0, NOW));
    let free = overridden(
        &tmp,
        r#"{"claude": {"prices": [{"match": "opus-5-5", "input": 0.0, "output": 0.0, "cache_read": 0.0}]}}"#,
    );
    let mut t = tracker_with(&tmp, free);
    t.refresh(NOW);
    let r = t.ranged(NOW);
    let s = &r.overall[&Window::All];
    near(s.month, 0.0);
    assert!(s.unpriced.is_empty(), "it has a price, of 0");
    assert_eq!(s.model_detail.len(), 1, "its tokens are counted");
    near(s.model_detail[0].output, 50_000.0);
}

#[test]
fn a_borrowed_model_named_current_later_is_priced_again() {
    let tmp = TempDir::new("cost-borrowed-current");
    append(
        &tmp,
        "projects/-p/a.jsonl",
        &model_line(
            "m1",
            "claude-fable-5-3",
            r#"{"output_tokens":1000000}"#,
            NOW,
        ),
    );
    let mut first = claude_tracker(&tmp);
    first.refresh(NOW);
    first.save_now(NOW);
    // Priced as Fable 5.2, $40 per million output tokens.
    near(month(&first.ranged(NOW), Window::All), 40.0);
    // A data file that names Fable 5.3 current with no price of its own: the family's, $50. The
    // price entries are the same, so their fingerprint is too.
    let named = Catalog::built_in().with_current("claude-fable-5-3");
    assert_eq!(named.prices_hash(), Catalog::built_in().prices_hash());
    let mut t = tracker_with(&tmp, named);
    t.refresh(NOW + 1.0);
    near(month(&t.ranged(NOW), Window::All), 50.0);
}

#[path = "tracker_v2_tests.rs"]
mod v2;
