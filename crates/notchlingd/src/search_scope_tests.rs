use super::*;
use crate::pages_ops::{PagesOps, PagesSetup};
use notchling_engine::pages::{PageLibrary, RecordContext};
use notchling_engine::sessions::Roots;
use notchling_local_api::{ClientKind, Tier};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("nl-scope-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.path(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Folders {
    work: String,
    other: String,
    solo: bool,
}

impl Folders {
    fn new(dir: &Dir) -> Folders {
        Folders {
            work: dir.path("work").to_string_lossy().into_owned(),
            other: dir.path("other").to_string_lossy().into_owned(),
            solo: false,
        }
    }

    fn claim(&self, path: &str) -> Option<String> {
        if path.starts_with(&self.work) {
            Some("work".into())
        } else if path.starts_with(&self.other) {
            Some("other".into())
        } else {
            None
        }
    }
}

impl Placing for Folders {
    fn org_of_folder(&self, folder: &str) -> String {
        self.claim(folder).unwrap_or_else(|| "personal".into())
    }

    fn claimed_by(&self, path: &str) -> Option<String> {
        self.claim(path)
    }

    fn only_personal(&self) -> bool {
        self.solo
    }
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn ops_for(dir: &Dir) -> PagesOps {
    PagesOps::new(PagesSetup {
        pages_dir: dir.path("data/pages"),
        writable: false,
        roots: Roots {
            main_dir: dir.path("claude"),
            lanes_file: dir.path("data/lanes.json"),
        },
        codex_sessions: dir.path("codex/sessions"),
        cache: Some(dir.path("data/daemon/search.db")),
    })
}

fn save_page(dir: &Dir, folder: &str, title: &str, link: &str, at: f64) {
    let file = format!("{}.html", title.replace(' ', "-"));
    dir.write(
        &format!("{folder}/app/{file}"),
        &format!("<title>{title}</title>"),
    );
    let event = json!({
        "tool_name": "Artifact",
        "cwd": dir.path(&format!("{folder}/app")),
        "tool_input": {"file_path": file},
        "tool_response": format!("Published {link}"),
    });
    PageLibrary::writable(dir.path("data/pages"))
        .record(
            &event,
            &RecordContext {
                account: "me@example.com",
                now: at,
                home: &dir.0,
            },
        )
        .unwrap()
        .unwrap();
}

fn strand(dir: &Dir) {
    let path = dir.path("data/pages/pages.json");
    let mut list: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for entry in list["pages"].as_array_mut().unwrap() {
        let map = entry.as_object_mut().unwrap();
        map.remove("cwd");
        map.insert("source".into(), json!("/private/tmp/scratch/old.html"));
    }
    std::fs::write(&path, list.to_string()).unwrap();
}

fn save_session(dir: &Dir, folder: &str, name: &str, first: &str, later: &str, ago: f64) {
    let path = dir.write(
        &format!("claude/projects/-scope/{name}.jsonl"),
        &format!(
            "{}\n{}\n",
            json!({"type": "user", "cwd": dir.path(&format!("{folder}/app")),
                "message": {"role": "user", "content": first}}),
            json!({"type": "assistant",
                "message": {"role": "assistant", "content": [{"type": "text", "text": later}]}}),
        ),
    );
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs_f64(now() - ago))
        .unwrap();
}

fn scoped(ops_: &PagesOps, folders: &Folders, org: &str, op: &str, args: Value) -> Value {
    ops_.call_scoped(op, &args, org, folders).unwrap()
}

fn titles(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["title"].as_str().unwrap().to_string())
        .collect()
}

const A: &str = "https://claude.ai/artifact/AaAaAaAaAaAaAaAaAaAaAa";
const B: &str = "https://claude.ai/artifact/BbBbBbBbBbBbBbBbBbBbBb";
const C: &str = "https://claude.ai/artifact/CcCcCcCcCcCcCcCcCcCcCc";

fn three_pages(dir: &Dir) {
    save_page(dir, "work", "Alpha Report", A, now());
    save_page(dir, "other", "Beta Report", B, now());
    save_page(dir, "misc", "Gamma Report", C, now());
}

#[cfg(unix)]
#[test]
fn a_page_is_placed_by_its_cwd_then_by_its_source() {
    let dir = Dir::new("place");
    let folders = Folders::new(&dir);
    let inside = format!("{}/app", folders.work);
    let outside = "/somewhere/else";
    assert_eq!(page_org(&folders, &inside, "").as_deref(), Some("work"));
    assert_eq!(
        page_org(&folders, outside, &format!("{inside}/x.html")).as_deref(),
        Some("personal")
    );
    assert_eq!(
        page_org(&folders, "", &format!("{inside}/x.html")).as_deref(),
        Some("work")
    );
    assert_eq!(
        page_org(
            &folders,
            "not/absolute",
            &format!("{}/x.html", folders.other)
        )
        .as_deref(),
        Some("other")
    );
    assert_eq!(page_org(&folders, "", "/private/tmp/x.html"), None);
    assert_eq!(page_org(&folders, "", ""), None);
    let solo = Folders {
        solo: true,
        ..Folders::new(&dir)
    };
    assert_eq!(
        page_org(&solo, "", "/private/tmp/x.html").as_deref(),
        Some("personal")
    );
}

#[test]
fn a_page_from_an_org_folder_is_not_found_by_a_personal_session() {
    let dir = Dir::new("personal");
    three_pages(&dir);
    let (ops_, folders) = (ops_for(&dir), Folders::new(&dir));
    for op_args in [
        (ops::SEARCH_QUERY, json!({"text": "report"})),
        (ops::PAGES_LIST, json!({})),
        (ops::PAGES_FIND, json!({"query": ""})),
        (ops::PAGES_FIND, json!({"query": A})),
        (ops::PAGES_FIND, json!({"query": B})),
    ] {
        let reply = scoped(&ops_, &folders, "personal", op_args.0, op_args.1.clone());
        let found = titles(&reply, "pages");
        assert!(
            found.iter().all(|t| t == "Gamma Report"),
            "{:?}: {found:?}",
            op_args
        );
    }
}

#[test]
fn a_page_from_an_org_folder_is_found_by_its_org_session() {
    let dir = Dir::new("work");
    three_pages(&dir);
    let (ops_, folders) = (ops_for(&dir), Folders::new(&dir));
    let search = scoped(
        &ops_,
        &folders,
        "work",
        ops::SEARCH_QUERY,
        json!({"text": "report"}),
    );
    assert_eq!(titles(&search, "pages"), ["Alpha Report"]);
    assert_eq!(search["organization"], "work");
    let list = scoped(&ops_, &folders, "work", ops::PAGES_LIST, json!({}));
    assert_eq!(titles(&list, "pages"), ["Alpha Report"]);
    let show = scoped(
        &ops_,
        &folders,
        "work",
        ops::PAGES_FIND,
        json!({"query": A}),
    );
    assert!(show["pages"][0]["describe"]
        .as_str()
        .unwrap()
        .contains("Alpha Report"));
    for foreign in [B, C] {
        let none = scoped(
            &ops_,
            &folders,
            "work",
            ops::PAGES_FIND,
            json!({"query": foreign}),
        );
        assert!(none["pages"].as_array().unwrap().is_empty(), "{none}");
    }
}

#[test]
fn an_unplaceable_page_is_dropped_for_agents_when_other_orgs_exist() {
    let dir = Dir::new("strand");
    three_pages(&dir);
    strand(&dir);
    let (ops_, folders) = (ops_for(&dir), Folders::new(&dir));
    for org in ["work", "personal"] {
        let list = scoped(&ops_, &folders, org, ops::PAGES_LIST, json!({}));
        assert!(
            list["pages"].as_array().unwrap().is_empty(),
            "{org}: {list}"
        );
        let search = scoped(
            &ops_,
            &folders,
            org,
            ops::SEARCH_QUERY,
            json!({"text": "report"}),
        );
        assert!(search["pages"].as_array().unwrap().is_empty(), "{org}");
    }
    let app = ops_.call(ops::PAGES_LIST, &json!({})).unwrap();
    assert_eq!(app["pages"].as_array().unwrap().len(), 3, "{app}");
}

#[test]
fn an_unplaceable_page_is_personal_when_personal_is_the_only_org() {
    let dir = Dir::new("solo");
    three_pages(&dir);
    strand(&dir);
    let ops_ = ops_for(&dir);
    let folders = Folders {
        solo: true,
        ..Folders::new(&dir)
    };
    let list = scoped(&ops_, &folders, "personal", ops::PAGES_LIST, json!({}));
    assert_eq!(list["pages"].as_array().unwrap().len(), 3, "{list}");
}

#[test]
fn a_shell_gets_no_session_for_pages_and_search() {
    let dir = Dir::new("shell");
    three_pages(&dir);
    let ops_ = ops_for(&dir);
    let caller = Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: None,
        pid: Some(1),
    };
    for (op, args) in [
        (ops::PAGES_LIST, json!({})),
        (ops::PAGES_FIND, json!({"query": A})),
        (ops::SEARCH_QUERY, json!({"text": "report"})),
    ] {
        let reply = call(None, &ops_, &caller, op, &args).unwrap();
        assert_eq!(reply["reason"], "no_session", "{op}: {reply}");
        assert_eq!(reply["pages"], json!([]), "{op}");
        assert!(!reply.to_string().contains("Report"), "{op}");
    }
}

#[test]
fn own_hits_behind_forty_other_org_hits_are_found() {
    let dir = Dir::new("pool");
    for n in 0..45 {
        save_session(
            &dir,
            "other",
            &format!("other-{n}"),
            &format!("needle in the other haystack {n}"),
            "nothing else",
            60.0,
        );
    }
    save_session(
        &dir,
        "work",
        "mine",
        "plain talk",
        "somewhere a needle is mentioned",
        7200.0,
    );
    let (ops_, folders) = (ops_for(&dir), Folders::new(&dir));
    let reply = scoped(
        &ops_,
        &folders,
        "work",
        ops::SEARCH_QUERY,
        json!({"text": "needle"}),
    );
    assert_eq!(titles(&reply, "sessions"), ["plain talk"], "{reply}");
}

#[test]
fn limit_is_applied_after_scoping_and_capped_at_200_sessions_and_20_pages() {
    let dir = Dir::new("cap");
    for n in 0..22 {
        save_page(
            &dir,
            "work",
            &format!("Cap Report {n}"),
            &format!("https://claude.ai/artifact/Cap{n:019}"),
            now() - f64::from(n),
        );
    }
    for n in 0..210 {
        save_session(
            &dir,
            "work",
            &format!("cap-{n}"),
            &format!("cap needle {n}"),
            "ok",
            60.0 + f64::from(n),
        );
    }
    let (ops_, folders) = (ops_for(&dir), Folders::new(&dir));
    let len = |args: Value| {
        let reply = scoped(&ops_, &folders, "work", ops::SEARCH_QUERY, args);
        (
            reply["sessions"].as_array().unwrap().len(),
            reply["pages"].as_array().unwrap().len(),
        )
    };
    assert_eq!(len(json!({"text": "cap"})), (30, 20));
    assert_eq!(len(json!({"text": "cap", "limit": 2})), (2, 2));
    assert_eq!(len(json!({"text": "cap", "limit": 500})), (200, 20));
    assert_eq!(len(json!({"text": "cap", "limit": 25})), (25, 20));
}

#[test]
fn since_applies_to_sessions_modified_and_pages_updated() {
    let dir = Dir::new("since");
    save_page(&dir, "work", "Old Report", A, now() - 40.0 * 86_400.0);
    save_page(&dir, "work", "New Report", B, now() - 3600.0);
    save_session(&dir, "work", "old", "old needle", "ok", 5.0 * 86_400.0);
    save_session(&dir, "work", "new", "new needle", "ok", 600.0);
    let (ops_, folders) = (ops_for(&dir), Folders::new(&dir));
    let ask = |since: Option<f64>| {
        let mut args = json!({});
        if let Some(since) = since {
            args["since"] = json!(since);
        }
        let reply = scoped(&ops_, &folders, "work", ops::SEARCH_QUERY, args);
        (titles(&reply, "pages"), titles(&reply, "sessions"))
    };
    let (pages, sessions) = ask(None);
    assert_eq!(pages.len(), 2);
    assert_eq!(sessions.len(), 2);
    let (pages, sessions) = ask(Some(now() - 86_400.0));
    assert_eq!(pages, ["New Report"]);
    assert_eq!(sessions, ["new needle"]);
    let (pages, sessions) = ask(Some(now() + 60.0));
    assert!(pages.is_empty() && sessions.is_empty());
}

#[test]
fn kind_narrows_the_answer() {
    let dir = Dir::new("kind");
    save_page(&dir, "work", "Needle Report", A, now());
    save_session(&dir, "work", "s", "needle talk", "ok", 60.0);
    let (ops_, folders) = (ops_for(&dir), Folders::new(&dir));
    let only = |kind: &str| {
        let reply = scoped(
            &ops_,
            &folders,
            "work",
            ops::SEARCH_QUERY,
            json!({"text": "needle", "kind": kind}),
        );
        (
            titles(&reply, "pages").len(),
            titles(&reply, "sessions").len(),
        )
    };
    assert_eq!(only("pages"), (1, 0));
    assert_eq!(only("sessions"), (0, 1));
}

#[test]
fn the_agent_window_is_14_days() {
    let dir = Dir::new("window");
    let folders = Folders::new(&dir);
    let reach = Reach {
        org: "work",
        placing: &folders,
        now: 1_000_000_000.0,
    };
    let inside = format!("{}/app", folders.work);
    assert!(reach.session(&inside, 1_000_000_000.0 - 13.0 * 86_400.0));
    assert!(!reach.session(&inside, 1_000_000_000.0 - 15.0 * 86_400.0));
    assert!(!reach.session("", 1_000_000_000.0));
    assert!(!reach.session(&format!("{}/app", folders.other), 1_000_000_000.0));
}

const SECRET: &str = concat!("sk-ant-", "api03-Zk9wQ1xV7bN3mT5pL8cD2fG6hJ4sA0rE");
const EMAIL: &str = "carol.jones@private-corp.example.com";

#[test]
fn agent_snippets_mask_emails_and_tokens_and_the_app_gets_them_unmasked() {
    let dir = Dir::new("mask");
    save_session(
        &dir,
        "work",
        "secrets",
        &format!("deploy notes for {EMAIL}"),
        &format!("zebra: {SECRET} {EMAIL} done"),
        60.0,
    );
    let (ops_, folders) = (ops_for(&dir), Folders::new(&dir));
    let reply = scoped(
        &ops_,
        &folders,
        "work",
        ops::SEARCH_QUERY,
        json!({"text": "zebra"}),
    );
    let row = &reply["sessions"][0];
    let snippet = row["snippet"].as_str().unwrap();
    for leaked in [SECRET, "Zk9wQ1xV7bN3mT5pL8cD2fG6hJ4sA0rE", "carol.jones"] {
        assert!(!snippet.contains(leaked), "{leaked}: {snippet}");
        assert!(!row["title"].as_str().unwrap().contains(leaked));
    }
    assert!(snippet.contains("c***@p***.com"), "{snippet}");
    assert_eq!(reply["masked"], true);
    assert_eq!(reply["window_days"], 14);
    for mark in row["highlights"].as_array().unwrap() {
        let (s, e) = (
            mark[0].as_u64().unwrap() as usize,
            mark[1].as_u64().unwrap() as usize,
        );
        assert_eq!(snippet[s..e].to_lowercase(), "zebra");
    }
    let app = ops_
        .call(ops::SEARCH_QUERY, &json!({"text": "zebra"}))
        .unwrap();
    let plain = app["sessions"][0]["snippet"].as_str().unwrap();
    assert!(plain.contains(SECRET) && plain.contains(EMAIL), "{plain}");
    assert!(app.get("masked").is_none() && app.get("organization").is_none());
}

#[test]
fn agent_page_rows_mask_the_account_email_and_the_app_gets_it_whole() {
    let dir = Dir::new("account");
    save_page(&dir, "work", "Alpha Report", A, now());
    let (ops_, folders) = (ops_for(&dir), Folders::new(&dir));
    let seen = |value: &Value| value.to_string();
    for (op, args) in [
        (ops::SEARCH_QUERY, json!({"text": "alpha"})),
        (ops::PAGES_LIST, json!({})),
        (ops::PAGES_FIND, json!({"query": "alpha"})),
    ] {
        let agent = scoped(&ops_, &folders, "work", op, args.clone());
        assert_eq!(agent["pages"].as_array().unwrap().len(), 1, "{op}");
        assert!(!seen(&agent).contains("me@example.com"), "{op}: {agent}");
        assert_eq!(agent["pages"][0]["account"], "m***@e***.com", "{op}");
        let app = ops_.call(op, &args).unwrap();
        assert_eq!(app["pages"][0]["account"], "me@example.com", "{op}");
    }
}

#[test]
fn bad_since_and_kind_arguments_are_refused_or_ignored_as_before() {
    let dir = Dir::new("bad");
    let (ops_, folders) = (ops_for(&dir), Folders::new(&dir));
    let err = ops_
        .call_scoped(
            ops::SEARCH_QUERY,
            &json!({"since": "yesterday"}),
            "work",
            &folders,
        )
        .unwrap_err();
    assert_eq!(err.code, notchling_local_api::ErrorCode::BadRequest);
    assert!(ops_
        .call(ops::SEARCH_QUERY, &json!({"kind": "folders"}))
        .is_ok());
}
