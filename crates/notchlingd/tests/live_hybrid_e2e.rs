#![cfg(unix)]

mod common;

use common::{fixture_root, home, scratch, Daemon};
use notchling_engine::settings::Settings;
use notchling_local_api::{ops, ClientKind};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const GRANITE_SOURCE: &str = "ibm-granite__granite-embedding-97m-multilingual-r2";
const STATIC_SOURCE: &str = "sentence-transformers__static-similarity-mrl-multilingual-v1";
const READY_WITHIN: Duration = Duration::from_secs(45 * 60);
const IDLE_EXIT: Duration = Duration::from_secs(6 * 60);

struct Models {
    granite: PathBuf,
    statics_tokenizer: PathBuf,
    statics_table: PathBuf,
}

fn skip(why: &str) -> Option<Models> {
    eprintln!("live hybrid check skipped: {why}");
    None
}

fn models() -> Option<Models> {
    if std::env::var("NOTCHLING_EMBED_REAL").as_deref() != Ok("1") {
        return skip("NOTCHLING_EMBED_REAL is not 1");
    }
    let Some(base) = std::env::var_os("NOTCHLING_EMBED_MODELS").map(PathBuf::from) else {
        return skip("NOTCHLING_EMBED_MODELS is not set");
    };
    let granite = base.join(GRANITE_SOURCE);
    for file in ["config.json", "model.safetensors", "tokenizer.json"] {
        if !granite.join(file).is_file() {
            return skip(&format!("{} is missing", granite.join(file).display()));
        }
    }
    let statics_tokenizer = base.join(STATIC_SOURCE).join("tokenizer.json");
    if !statics_tokenizer.is_file() {
        return skip(&format!("{} is missing", statics_tokenizer.display()));
    }
    let Some(statics_table) = std::env::var_os("NOTCHLING_EMBED_STATIC_TABLE").map(PathBuf::from)
    else {
        return skip("NOTCHLING_EMBED_STATIC_TABLE is not set");
    };
    if !statics_table.is_file() {
        return skip(&format!("{} is missing", statics_table.display()));
    }
    let helper = Path::new(env!("CARGO_BIN_EXE_notchling")).with_file_name("notchling-embed");
    if !helper.is_file() {
        return skip(&format!(
            "{} is missing: cargo build -p notchling-embed",
            helper.display()
        ));
    }
    Some(Models {
        granite,
        statics_tokenizer,
        statics_table,
    })
}

fn lay_out(data: &Path, m: &Models) {
    let granite = data.join("models/granite-97m-r2");
    let statics = data.join("models/static-mrl");
    std::fs::create_dir_all(&granite).unwrap();
    std::fs::create_dir_all(&statics).unwrap();
    for file in ["config.json", "model.safetensors", "tokenizer.json"] {
        std::fs::copy(m.granite.join(file), granite.join(file)).unwrap();
    }
    std::fs::copy(&m.statics_table, statics.join("table-512i8.bin")).unwrap();
    std::fs::copy(&m.statics_tokenizer, statics.join("tokenizer.json")).unwrap();
}

fn plant_session(root: &Path, name: &str, first: &str, later: &str) {
    let dir = root.join("claude/projects/-live");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.jsonl"));
    let user = json!({"type": "user", "cwd": "/work/live",
        "message": {"role": "user", "content": first}});
    let assistant = json!({"type": "assistant",
        "message": {"role": "assistant", "content": [{"type": "text", "text": later}]}});
    std::fs::write(&path, format!("{user}\n{assistant}\n")).unwrap();
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(600))
        .unwrap();
}

fn helper_running() -> bool {
    std::process::Command::new("pgrep")
        .args(["-x", "notchling-embed"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn titles(reply: &Value) -> Vec<String> {
    reply["sessions"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|r| r["title"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn unaccented_vietnamese_finds_the_accented_session_with_the_real_models() {
    let Some(models) = models() else { return };
    let dir = scratch("live-hybrid");
    let root = fixture_root(&dir.0);
    let home = home(&dir.0);
    let data = root.join("notchling");
    lay_out(&data, &models);
    Settings::open(&data.join("settings.json"))
        .unwrap()
        .set("semanticSearch", json!(true))
        .unwrap();
    plant_session(
        &root,
        "dang-nhap",
        "Sửa lỗi đăng nhập thất bại sau khi đổi mật khẩu",
        "Đã sửa phần đăng nhập: mật khẩu mới được lưu đúng.",
    );
    plant_session(
        &root,
        "ve-sinh",
        "Dọn dẹp thư mục cũ",
        "Đã xoá các tệp tạm trong thư mục build.",
    );
    let _daemon = Daemon::start_for_app(&root, &home);
    let mut client = common::connect(&root, ClientKind::App);
    client.set_timeout(Duration::from_secs(120));
    let query = json!({"text": "dang nhap", "mode": "hybrid"});
    let deadline = Instant::now() + READY_WITHIN;
    let mut reply = Value::Null;
    while Instant::now() < deadline {
        reply = client.call(ops::SEARCH_QUERY, query.clone()).unwrap();
        if reply["semantic"]["state"] == "ready" {
            break;
        }
        std::thread::sleep(Duration::from_secs(5));
    }
    assert_eq!(reply["semantic"]["state"], "ready", "{reply}");
    let found = titles(&reply);
    assert!(
        found.iter().any(|t| t.contains("đăng nhập")),
        "{found:?} in {reply}"
    );
    let status = client.call(ops::EMBED_STATUS, json!({})).unwrap();
    assert_eq!(status["state"], "ready", "{status}");
    if std::env::var("NOTCHLING_EMBED_LIVE_IDLE").as_deref() == Ok("1") {
        let until = Instant::now() + IDLE_EXIT;
        while helper_running() && Instant::now() < until {
            std::thread::sleep(Duration::from_secs(10));
        }
        assert!(!helper_running(), "the helper outlived its idle limit");
    }
}
