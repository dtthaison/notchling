use super::ledger::Ledger;
use super::tracker::{Root, ANTIGRAVITY, CLAUDE_CODE, CODEX};
use crate::catalog::Catalog;
use crate::pages::clock::{civil_at, text};
use crate::sessions::testutil::TempDir;
use std::path::PathBuf;
use std::sync::Arc;

pub const NOW: f64 = 1_800_000_000.0;
pub const DAY: f64 = 86_400.0;
pub const HOUR: f64 = 3_600.0;

pub fn utc(_: f64) -> i64 {
    0
}

pub fn iso(ts: f64) -> String {
    format!("{}Z", text(civil_at(ts, 0), true).replace(' ', "T"))
}

#[derive(Clone, Copy)]
pub struct Use {
    pub input: u64,
    pub output: u64,
    pub read: u64,
    pub w5: u64,
    pub w1: u64,
    pub split: bool,
}

pub const NONE: Use = Use {
    input: 0,
    output: 0,
    read: 0,
    w5: 0,
    w1: 0,
    split: true,
};

pub fn line(id: &str, ts: f64, model: &str, u: Use, extra: &str) -> String {
    let creation = if u.split {
        format!(
            r#","cache_creation_input_tokens":{},"cache_creation":{{"ephemeral_5m_input_tokens":{},"ephemeral_1h_input_tokens":{}}}"#,
            u.w5 + u.w1,
            u.w5,
            u.w1
        )
    } else {
        format!(r#","cache_creation_input_tokens":{}"#, u.w5 + u.w1)
    };
    format!(
        r#"{{"type":"assistant","timestamp":"{}","requestId":"r-{id}"{extra},"message":{{"id":"{id}","model":"{model}","usage":{{"input_tokens":{},"output_tokens":{},"cache_read_input_tokens":{}{creation}}},"content":[{{"type":"text","text":"SECRET-REPLY-TEXT"}},{{"type":"tool_use","name":"Bash","input":{{"command":"SECRET-COMMAND"}}}}]}}}}"#,
        iso(ts),
        u.input,
        u.output,
        u.read
    ) + "\n"
}

pub struct World {
    pub tmp: TempDir,
}

impl World {
    pub fn new(tag: &str) -> World {
        World {
            tmp: TempDir::new(tag),
        }
    }

    pub fn main(&self, session: &str, text: &str) -> PathBuf {
        self.append(&format!("projects/-tmp-demo/{session}.jsonl"), text)
    }

    pub fn sub(&self, session: &str, agent: &str, meta: &str, text: &str) -> PathBuf {
        let rel = format!("projects/-tmp-demo/{session}/subagents/{agent}");
        if !meta.is_empty() {
            self.tmp.write(&format!("{rel}.meta.json"), meta);
        }
        self.append(&format!("{rel}.jsonl"), text)
    }

    pub fn append(&self, rel: &str, text: &str) -> PathBuf {
        use std::io::Write;
        let path = self.tmp.path(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("open");
        file.write_all(text.as_bytes()).expect("write");
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(NOW);
        file.set_modified(at).expect("mtime");
        path
    }

    pub fn overwrite(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.tmp.path(rel);
        std::fs::write(&path, text).expect("write");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open");
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(NOW);
        file.set_modified(at).expect("mtime");
        path
    }

    pub fn roots(&self) -> Vec<Root> {
        vec![
            Root {
                dir: self.tmp.path("projects"),
                channel: CLAUDE_CODE,
            },
            Root {
                dir: self.tmp.path("codex"),
                channel: CODEX,
            },
            Root {
                dir: self.tmp.path("brain"),
                channel: ANTIGRAVITY,
            },
        ]
    }

    pub fn ledger(&self) -> Ledger {
        Ledger::in_memory(self.roots(), Arc::new(Catalog::built_in()))
            .expect("ledger")
            .with_zone(utc)
    }

    pub fn file_ledger(&self) -> Ledger {
        Ledger::open(
            &self.tmp.path("data/daemon/usage.db"),
            self.roots(),
            Arc::new(Catalog::built_in()),
        )
        .expect("ledger")
        .with_zone(utc)
    }
}
