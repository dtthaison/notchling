#![cfg(unix)]

mod common;

use common::{home, scratch, Daemon, Dir};
use notchling_engine::organizations::Organizations;
use notchling_engine::pages::{PageLibrary, RecordContext};
use notchling_local_api::{ops, ClientKind};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SESSION: &str = "52afae32-0000-4000-8000-000000000001";
const HELPER: &str = "scope_helper_runs_commands_as_the_session_process";

const WORK_LINK: &str = "https://claude.ai/artifact/WwWwWwWwWwWwWwWwWwWwWw";
const OTHER_LINK: &str = "https://claude.ai/artifact/OoOoOoOoOoOoOoOoOoOoOo";
const MISC_LINK: &str = "https://claude.ai/artifact/MmMmMmMmMmMmMmMmMmMmMm";

#[test]
fn scope_helper_runs_commands_as_the_session_process() {
    let Ok(bin) = std::env::var("NL_SCOPE_E2E_BIN") else {
        return;
    };
    let mut go = String::new();
    std::io::stdin().lock().read_line(&mut go).unwrap();
    let steps: Vec<Vec<String>> =
        serde_json::from_str(&std::env::var("NL_SCOPE_E2E_STEPS").unwrap()).unwrap();
    for step in steps {
        let out = Command::new(&bin)
            .args(&step)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        println!(
            "STEP {}",
            json!({"code": out.status.code(), "out": String::from_utf8_lossy(&out.stdout),
                   "err": String::from_utf8_lossy(&out.stderr)})
        );
    }
}

fn as_session(world: &World, steps: &[&[&str]]) -> Vec<Value> {
    let claude_dir = world.root.join(".local/share/claude/versions/1");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let claude = claude_dir.join("claude");
    let _ = std::fs::remove_file(&claude);
    std::fs::copy(std::env::current_exe().unwrap(), &claude).unwrap();
    let mut parent = common::spawn_retrying(
        Command::new(&claude)
            .args(["--exact", HELPER, "--nocapture", "--test-threads=1"])
            .env("NL_SCOPE_E2E_BIN", env!("CARGO_BIN_EXE_notchling"))
            .env("NL_SCOPE_E2E_STEPS", serde_json::to_string(steps).unwrap())
            .env("NOTCHLING_DATA_ROOT", &world.root)
            .env("HOME", &world.home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()),
    );
    let sessions = world.root.join("claude/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let file = json!({
        "sessionId": SESSION,
        "pid": parent.id(),
        "cwd": world.root.join("launch").to_string_lossy(),
        "jobId": "j1",
    });
    std::fs::write(
        sessions.join(format!("{}.json", parent.id())),
        file.to_string(),
    )
    .unwrap();
    writeln!(parent.stdin.take().unwrap(), "go").unwrap();
    let reader = BufReader::new(parent.stdout.take().unwrap());
    let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();
    let _ = parent.wait();
    lines
        .iter()
        .filter_map(|line| line.split_once("STEP ").map(|(_, text)| text.to_owned()))
        .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
        .collect()
}

fn parsed(step: &Value) -> Value {
    serde_json::from_str(step["out"].as_str().unwrap_or("")).unwrap_or(Value::Null)
}

fn code(step: &Value) -> i64 {
    step["code"].as_i64().unwrap_or(-1)
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

struct World {
    _dir: Dir,
    root: PathBuf,
    home: PathBuf,
    work: PathBuf,
    other: PathBuf,
    misc: PathBuf,
}

fn world(tag: &str, session_in: &str, organizations: bool) -> World {
    let dir = scratch(tag);
    let root = dir.0.join("root");
    let home = home(&dir.0);
    let (work, other, misc) = (dir.0.join("work"), dir.0.join("other"), dir.0.join("misc"));
    for folder in [&work, &other, &misc] {
        std::fs::create_dir_all(folder.join("app")).unwrap();
    }
    if organizations {
        let book = Organizations::new(&root.join("notchling")).with_home(&root);
        for (name, folder) in [("Work", &work), ("Other", &other)] {
            let id = book.create(name, None, None).unwrap();
            book.set_folders(&id, &[folder.to_string_lossy().into_owned()])
                .unwrap();
        }
    }
    let job = root.join("claude/jobs/j1");
    std::fs::create_dir_all(&job).unwrap();
    let folder = dir.0.join(session_in).join("app");
    std::fs::write(
        job.join("state.json"),
        json!({
            "sessionId": "be50d36a-0000-4000-8000-000000000002",
            "resumeSessionId": SESSION,
            "cwd": folder,
            "name": "scope",
            "state": "running",
        })
        .to_string(),
    )
    .unwrap();
    World {
        _dir: dir,
        root,
        home,
        work,
        other,
        misc,
    }
}

fn plant_page(world: &World, folder: &Path, title: &str, link: &str, at: f64) {
    plant_page_with(world, folder, title, link, at, "body");
}

fn plant_page_with(world: &World, folder: &Path, title: &str, link: &str, at: f64, body: &str) {
    let cwd = folder.join("app");
    let file = format!("{}.html", title.replace(' ', "-"));
    std::fs::write(
        cwd.join(&file),
        format!("<title>{title}</title><p>{body}</p>"),
    )
    .unwrap();
    let event = json!({
        "tool_name": "Artifact",
        "cwd": cwd,
        "tool_input": {"file_path": file},
        "tool_response": format!("Published {link}"),
    });
    PageLibrary::writable(world.root.join("notchling/pages"))
        .record(
            &event,
            &RecordContext {
                account: "me@example.com",
                now: at,
                home: &world.home,
            },
        )
        .unwrap()
        .unwrap();
}

fn plant_pages(world: &World) {
    plant_page(world, &world.work, "Alpha Report", WORK_LINK, now());
    plant_page(world, &world.other, "Beta Report", OTHER_LINK, now());
    plant_page(world, &world.misc, "Gamma Report", MISC_LINK, now());
}

fn rewrite_list(world: &World, change: impl Fn(&mut Value)) {
    let path = world.root.join("notchling/pages/pages.json");
    let mut list: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for entry in list["pages"].as_array_mut().unwrap() {
        change(entry);
    }
    std::fs::write(&path, list.to_string()).unwrap();
}

fn plant_session(world: &World, folder: &Path, name: &str, first: &str, later: &str, ago: f64) {
    let dir = world.root.join("claude/projects/-scope");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.jsonl"));
    let cwd = folder.join("app");
    let user = json!({"type": "user", "cwd": cwd,
        "message": {"role": "user", "content": first}});
    let assistant = json!({"type": "assistant",
        "message": {"role": "assistant", "content": [{"type": "text", "text": later}]}});
    std::fs::write(&path, format!("{user}\n{assistant}\n")).unwrap();
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs_f64(now() - ago))
        .unwrap();
}

fn titles(reply: &Value, key: &str) -> Vec<String> {
    reply[key]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|r| r["title"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn ids(reply: &Value) -> Vec<String> {
    reply["pages"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|r| r["id"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_page_from_an_org_folder_is_not_found_by_a_personal_session() {
    let world = world("scope-personal", "misc", true);
    plant_pages(&world);
    let _daemon = Daemon::start(&world.root, &world.home);
    let steps = as_session(
        &world,
        &[
            &["search", "report", "--json"],
            &["pages", "--json"],
            &["page", WORK_LINK, "--json"],
            &["page", MISC_LINK, "--json"],
        ],
    );
    assert_eq!(steps.len(), 4, "{steps:?}");
    let search = parsed(&steps[0]);
    assert_eq!(titles(&search, "pages"), ["Gamma Report"], "{search}");
    assert_eq!(search["organization"], "personal", "{search}");
    let list = parsed(&steps[1]);
    assert_eq!(ids(&list), ["MmMmMmMmMmMmMmMmMmMmMm"], "{list}");
    assert_eq!(code(&steps[2]), 1, "{:?}", steps[2]);
    assert!(
        !steps[2]["out"].as_str().unwrap().contains("Alpha Report"),
        "{:?}",
        steps[2]
    );
    assert_eq!(code(&steps[3]), 0, "{:?}", steps[3]);
}

#[test]
fn a_page_from_an_org_folder_is_found_by_its_org_session() {
    let world = world("scope-work", "work", true);
    plant_pages(&world);
    let _daemon = Daemon::start(&world.root, &world.home);
    let steps = as_session(
        &world,
        &[
            &["search", "report", "--json"],
            &["pages", "--json"],
            &["page", WORK_LINK, "--json"],
            &["page", OTHER_LINK, "--json"],
            &["page", MISC_LINK, "--json"],
        ],
    );
    assert_eq!(steps.len(), 5, "{steps:?}");
    let search = parsed(&steps[0]);
    assert_eq!(titles(&search, "pages"), ["Alpha Report"], "{search}");
    let list = parsed(&steps[1]);
    assert_eq!(ids(&list), ["WwWwWwWwWwWwWwWwWwWwWw"], "{list}");
    assert_eq!(code(&steps[2]), 0, "{:?}", steps[2]);
    assert_eq!(parsed(&steps[2])["pages"][0]["title"], "Alpha Report");
    assert_eq!(code(&steps[3]), 1, "{:?}", steps[3]);
    assert_eq!(code(&steps[4]), 1, "{:?}", steps[4]);
}

#[test]
fn an_old_page_with_no_cwd_is_placed_by_source() {
    let world = world("scope-source", "work", true);
    plant_pages(&world);
    rewrite_list(&world, |entry| {
        entry.as_object_mut().unwrap().remove("cwd");
    });
    let _daemon = Daemon::start(&world.root, &world.home);
    let steps = as_session(&world, &[&["pages", "--json"]]);
    let list = parsed(&steps[0]);
    assert_eq!(ids(&list), ["WwWwWwWwWwWwWwWwWwWwWw"], "{list}");
}

fn strand(world: &World) {
    rewrite_list(world, |entry| {
        let map = entry.as_object_mut().unwrap();
        map.remove("cwd");
        map.insert("source".into(), json!("/private/tmp/scratch/old.html"));
    });
}

#[test]
fn an_unplaceable_page_is_dropped_for_agents_when_other_orgs_exist() {
    for session_in in ["work", "misc"] {
        let world = world(&format!("scope-strand-{session_in}"), session_in, true);
        plant_pages(&world);
        strand(&world);
        let _daemon = Daemon::start(&world.root, &world.home);
        let steps = as_session(
            &world,
            &[&["pages", "--json"], &["search", "report", "--json"]],
        );
        let list = parsed(&steps[0]);
        assert_eq!(ids(&list), Vec::<String>::new(), "{session_in}: {list}");
        let search = parsed(&steps[1]);
        assert!(
            titles(&search, "pages").is_empty(),
            "{session_in}: {search}"
        );
    }
}

#[test]
fn an_unplaceable_page_is_personal_when_personal_is_the_only_org() {
    let world = world("scope-strand-solo", "misc", false);
    plant_pages(&world);
    strand(&world);
    let _daemon = Daemon::start(&world.root, &world.home);
    let steps = as_session(
        &world,
        &[&["pages", "--json"], &["search", "report", "--json"]],
    );
    let list = parsed(&steps[0]);
    assert_eq!(ids(&list).len(), 3, "{list}");
    let search = parsed(&steps[1]);
    assert_eq!(titles(&search, "pages").len(), 3, "{search}");
}

#[test]
fn a_shell_gets_no_session_for_pages_and_page() {
    let world = world("scope-shell", "misc", true);
    plant_pages(&world);
    let _daemon = Daemon::start(&world.root, &world.home);
    for args in [&["pages", "--json"][..], &["page", MISC_LINK, "--json"]] {
        let (exit, out, _) = common::notchling(&world.root, &world.home, args);
        assert_eq!(exit, 1, "{args:?}: {out}");
        let value: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["error"]["reason"], "no_session", "{args:?}: {out}");
        assert!(!out.contains("Report"), "{args:?} leaked: {out}");
    }
    let (exit, out, _) =
        common::notchling(&world.root, &world.home, &["search", "report", "--json"]);
    assert_eq!(exit, 0, "{out}");
    let value: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["reason"], "no_session", "{out}");
    assert!(!out.contains("Report"), "search leaked: {out}");
}

#[test]
fn own_hits_behind_forty_other_org_hits_are_found() {
    let world = world("scope-pool", "work", true);
    for n in 0..45 {
        plant_session(
            &world,
            &world.other,
            &format!("other-{n}"),
            &format!("needle in the other haystack {n}"),
            "nothing else",
            60.0,
        );
    }
    plant_session(
        &world,
        &world.work,
        "mine",
        "plain talk about nothing",
        "somewhere a needle is mentioned once",
        7200.0,
    );
    let _daemon = Daemon::start(&world.root, &world.home);
    let steps = as_session(&world, &[&["search", "needle", "--json"]]);
    let search = parsed(&steps[0]);
    assert_eq!(
        titles(&search, "sessions"),
        ["plain talk about nothing"],
        "{search}"
    );
}

fn date_of(ts: f64) -> String {
    notchling_engine::pages::clock::local_text(ts)[..10].to_string()
}

#[test]
fn since_takes_durations_dates_and_epoch() {
    let world = world("scope-since", "work", true);
    plant_session(
        &world,
        &world.work,
        "recent",
        "needle two hours ago",
        "ok",
        7200.0,
    );
    let _daemon = Daemon::start(&world.root, &world.home);
    let two_days_ago = date_of(now() - 2.0 * 86400.0);
    let tomorrow = date_of(now() + 86400.0);
    let epoch_hour_ago = format!("{}", (now() - 3600.0) as u64);
    let epoch_three_hours_ago = format!("{}", (now() - 3.0 * 3600.0) as u64);
    let epoch_hour_ahead = format!("{}", (now() + 3600.0) as u64);
    let cases: Vec<(&str, bool)> = vec![
        ("30m", false),
        ("24h", true),
        ("7d", true),
        (two_days_ago.as_str(), true),
        (tomorrow.as_str(), false),
        ("1763900000", true),
        (epoch_hour_ago.as_str(), false),
        (epoch_three_hours_ago.as_str(), true),
        (epoch_hour_ahead.as_str(), false),
    ];
    let steps = as_session(
        &world,
        &cases
            .iter()
            .map(|(since, _)| vec!["search", "needle", "--since", since, "--json"])
            .collect::<Vec<_>>()
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>(),
    );
    assert_eq!(steps.len(), cases.len(), "{steps:?}");
    for ((since, found), step) in cases.iter().zip(&steps) {
        assert_eq!(code(step), 0, "{since}: {step:?}");
        let reply = parsed(step);
        assert_eq!(
            !titles(&reply, "sessions").is_empty(),
            *found,
            "{since}: {reply}"
        );
    }
    let bad = as_session(&world, &[&["search", "needle", "--since", "7x", "--json"]]);
    assert_eq!(code(&bad[0]), 2, "{bad:?}");
}

#[test]
fn since_applies_to_pages_updated() {
    let world = world("scope-since-pages", "work", true);
    plant_page(
        &world,
        &world.work,
        "Old Report",
        WORK_LINK,
        now() - 40.0 * 86400.0,
    );
    plant_page(
        &world,
        &world.work,
        "New Report",
        "https://claude.ai/artifact/NnNnNnNnNnNnNnNnNnNnNn",
        now() - 3600.0,
    );
    let _daemon = Daemon::start(&world.root, &world.home);
    let steps = as_session(
        &world,
        &[
            &["search", "report", "--json"],
            &["search", "report", "--since", "7d", "--json"],
            &["search", "report", "--since", "90d", "--json"],
        ],
    );
    assert_eq!(titles(&parsed(&steps[0]), "pages").len(), 2);
    assert_eq!(titles(&parsed(&steps[1]), "pages"), ["New Report"]);
    assert_eq!(titles(&parsed(&steps[2]), "pages").len(), 2);
}

#[test]
fn limit_flag_is_capped() {
    let world = world("scope-limit", "work", true);
    for n in 0..22 {
        plant_page(
            &world,
            &world.work,
            &format!("Cap Report {n}"),
            &format!("https://claude.ai/artifact/Cap{n:019}"),
            now() - f64::from(n),
        );
    }
    for n in 0..210 {
        plant_session(
            &world,
            &world.work,
            &format!("cap-{n}"),
            &format!("cap needle {n}"),
            "ok",
            60.0 + f64::from(n),
        );
    }
    let _daemon = Daemon::start(&world.root, &world.home);
    let steps = as_session(
        &world,
        &[
            &["search", "cap", "--json"],
            &["search", "cap", "--limit", "2", "--json"],
            &["search", "cap", "--limit", "500", "--json"],
            &["search", "cap", "--limit", "0", "--json"],
            &["search", "cap", "--limit", "many", "--json"],
        ],
    );
    let len = |step: &Value, key: &str| parsed(step)[key].as_array().map_or(0, Vec::len);
    assert_eq!(
        (len(&steps[0], "sessions"), len(&steps[0], "pages")),
        (30, 20)
    );
    assert_eq!(
        (len(&steps[1], "sessions"), len(&steps[1], "pages")),
        (2, 2)
    );
    assert_eq!(
        (len(&steps[2], "sessions"), len(&steps[2], "pages")),
        (200, 20)
    );
    assert_eq!(code(&steps[3]), 2, "{:?}", steps[3]);
    assert_eq!(code(&steps[4]), 2, "{:?}", steps[4]);
}

const SECRET: &str = concat!("sk-ant-", "api03-Zk9wQ1xV7bN3mT5pL8cD2fG6hJ4sA0rE");
const EMAIL: &str = "carol.jones@private-corp.example.com";

fn plant_secret_session(world: &World) {
    plant_session(
        world,
        &world.work,
        "secrets",
        "deploy notes",
        &format!("zebra: {SECRET} {EMAIL} done"),
        60.0,
    );
}

#[test]
fn agent_snippets_mask_emails_and_tokens() {
    let world = world("scope-mask", "work", true);
    plant_secret_session(&world);
    let _daemon = Daemon::start(&world.root, &world.home);
    let steps = as_session(&world, &[&["search", "zebra", "--json"]]);
    let reply = parsed(&steps[0]);
    let row = &reply["sessions"][0];
    let snippet = row["snippet"].as_str().unwrap();
    assert!(snippet.contains("zebra"), "{snippet}");
    for leaked in [
        SECRET,
        "Zk9wQ1xV7bN3mT5pL8cD2fG6hJ4sA0rE",
        EMAIL,
        "carol.jones",
    ] {
        assert!(!snippet.contains(leaked), "{leaked} leaked: {snippet}");
    }
    assert!(snippet.contains("c***@p***.com"), "{snippet}");
    assert_eq!(reply["masked"], true, "{reply}");
    for range in row["highlights"].as_array().unwrap() {
        let (start, end) = (
            range[0].as_u64().unwrap() as usize,
            range[1].as_u64().unwrap() as usize,
        );
        assert_eq!(snippet[start..end].to_lowercase(), "zebra", "{snippet}");
    }
}

#[test]
fn the_app_gets_snippets_unmasked() {
    let world = world("scope-app", "work", true);
    plant_secret_session(&world);
    let _daemon = Daemon::start_for_app(&world.root, &world.home);
    let mut client = common::connect(&world.root, ClientKind::App);
    let reply = client
        .call(ops::SEARCH_QUERY, json!({"text": "zebra"}))
        .unwrap();
    let snippet = reply["sessions"][0]["snippet"].as_str().unwrap();
    assert!(snippet.contains(EMAIL), "{snippet}");
    assert!(snippet.contains(SECRET), "{snippet}");
    assert!(reply.get("masked").is_none(), "{reply}");
}

#[test]
fn an_old_session_is_found_by_the_app_tier_but_not_by_an_agent() {
    let world = world("scope-old", "work", true);
    plant_session(
        &world,
        &world.work,
        "ancient",
        "quokka planning",
        "the quokka plan was agreed",
        40.0 * 86_400.0,
    );
    plant_session(
        &world,
        &world.work,
        "recent",
        "quokka today",
        "fresh quokka notes",
        3_600.0,
    );
    let _daemon = Daemon::start_for_app(&world.root, &world.home);
    let mut client = common::connect(&world.root, ClientKind::App);
    let reply = client
        .call(ops::SEARCH_QUERY, json!({"text": "quokka"}))
        .unwrap();
    let mut found = titles(&reply, "sessions");
    found.sort();
    assert_eq!(found, ["quokka planning", "quokka today"], "{reply}");
    let steps = as_session(&world, &[&["search", "quokka", "--json"]]);
    let search = parsed(&steps[0]);
    assert_eq!(titles(&search, "sessions"), ["quokka today"], "{search}");
    assert_eq!(search["window_days"], 14, "{search}");
}

#[test]
fn the_reply_says_what_is_indexed() {
    let world = world("scope-indexed", "work", true);
    plant_session(
        &world,
        &world.work,
        "old",
        "quokka",
        "plan",
        40.0 * 86_400.0,
    );
    let _daemon = Daemon::start_for_app(&world.root, &world.home);
    let mut client = common::connect(&world.root, ClientKind::App);
    let reply = client
        .call(ops::SEARCH_QUERY, json!({"text": "quokka"}))
        .unwrap();
    assert!(
        reply.get("indexedSince").is_some_and(Value::is_null),
        "{reply}"
    );
    assert!(reply["refreshing"].is_boolean(), "{reply}");
}

fn plant_body_pages(world: &World) {
    let body = format!("the wombat roster lists {EMAIL} and {SECRET}");
    plant_page_with(world, &world.work, "Alpha Report", WORK_LINK, now(), &body);
    plant_page_with(world, &world.other, "Beta Report", OTHER_LINK, now(), &body);
    plant_page_with(world, &world.misc, "Gamma Report", MISC_LINK, now(), &body);
}

#[test]
fn a_page_is_found_by_words_in_its_body_scoped_and_masked_for_agents() {
    let world = world("scope-body", "work", true);
    plant_body_pages(&world);
    let _daemon = Daemon::start(&world.root, &world.home);
    let steps = as_session(
        &world,
        &[&["search", "wombat", "--kind", "pages", "--json"]],
    );
    let search = parsed(&steps[0]);
    assert_eq!(titles(&search, "pages"), ["Alpha Report"], "{search}");
    let snippet = search["pages"][0]["snippet"].as_str().unwrap();
    assert!(snippet.contains("wombat"), "{snippet}");
    for leaked in [
        SECRET,
        "Zk9wQ1xV7bN3mT5pL8cD2fG6hJ4sA0rE",
        EMAIL,
        "carol.jones",
    ] {
        assert!(!snippet.contains(leaked), "{leaked} leaked: {snippet}");
    }
    assert!(snippet.contains("c***@p***.com"), "{snippet}");
    for range in search["pages"][0]["highlights"].as_array().unwrap() {
        let (start, end) = (
            range[0].as_u64().unwrap() as usize,
            range[1].as_u64().unwrap() as usize,
        );
        assert_eq!(snippet[start..end].to_lowercase(), "wombat", "{snippet}");
    }
}

#[test]
fn a_content_hit_from_another_org_is_dropped() {
    let world = world("scope-body-org", "misc", true);
    plant_body_pages(&world);
    let _daemon = Daemon::start(&world.root, &world.home);
    let steps = as_session(&world, &[&["search", "wombat", "--json"]]);
    let search = parsed(&steps[0]);
    assert_eq!(titles(&search, "pages"), ["Gamma Report"], "{search}");
}

#[test]
fn the_app_finds_every_page_by_its_body_unmasked() {
    let world = world("scope-body-app", "work", true);
    plant_body_pages(&world);
    let _daemon = Daemon::start_for_app(&world.root, &world.home);
    let mut client = common::connect(&world.root, ClientKind::App);
    let reply = client
        .call(
            ops::SEARCH_QUERY,
            json!({"text": "wombat", "kind": "pages"}),
        )
        .unwrap();
    assert_eq!(titles(&reply, "pages").len(), 3, "{reply}");
    let snippet = reply["pages"][0]["snippet"].as_str().unwrap();
    assert!(snippet.contains(EMAIL), "{snippet}");
}
