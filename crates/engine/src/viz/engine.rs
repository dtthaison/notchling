use super::facts::{self, TranscriptFacts};
use super::suggest::{git_branch, VizSignals};
use super::web_url;
use crate::bridge::connection::lock;
use crate::bridge::model::Session;
use notchling_protocol::generated as wire;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

pub const MAX_SUBAGENT_FILES: usize = 40;
const SUBAGENT_TAIL_BYTES: u64 = 1_000_000;
const MAX_CALLS_SHOWN: usize = 300;
const MAX_TURNS_SHOWN: usize = 40;
const MAX_CHART_POINTS: usize = 300;
const GIT_TTL_SECS: f64 = 10.0;
const RECENT_MERMAID_SECS: f64 = 30.0 * 60.0;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitSnapshot {
    pub branch: Option<String>,
    pub diff_stat: Option<String>,
}

impl GitSnapshot {
    pub fn with_branch(cwd: &str, diff_stat: Option<String>) -> GitSnapshot {
        GitSnapshot {
            branch: git_branch(cwd),
            diff_stat,
        }
    }
}

pub enum Scan {
    Found(wire::EntitySpec),
    NoTables,
    Unsupported,
}

pub trait Schemas: Send + Sync {
    fn scan(&self, cwd: &str) -> Scan;
}

pub struct NoSchemas;

impl Schemas for NoSchemas {
    fn scan(&self, _: &str) -> Scan {
        Scan::Unsupported
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VizBundle {
    pub flow: wire::VizSpec,
    pub diagram: wire::VizSpec,
    pub chart: wire::VizSpec,
    pub signals: VizSignals,
    pub web_url: Option<String>,
    pub web_candidates: Vec<String>,
    pub device_hints: Vec<String>,
}

struct CachedFacts {
    modified: Option<SystemTime>,
    size: u64,
    facts: TranscriptFacts,
}

type GitReader = Box<dyn Fn(&str) -> GitSnapshot + Send + Sync>;
type Clock = Box<dyn Fn() -> f64 + Send + Sync>;
type Listening = Box<dyn Fn(u16) -> bool + Send + Sync>;

pub struct VizEngine {
    now: Clock,
    git: GitReader,
    schemas: Box<dyn Schemas>,
    is_listening: Listening,
    home: PathBuf,
    facts_cache: Mutex<HashMap<String, CachedFacts>>,
    git_cache: Mutex<HashMap<String, (f64, GitSnapshot)>>,
}

impl VizEngine {
    pub fn new(now: Clock, git: GitReader) -> VizEngine {
        VizEngine {
            now,
            git,
            schemas: Box::new(NoSchemas),
            is_listening: Box::new(web_url::tcp_is_listening),
            home: PathBuf::new(),
            facts_cache: Mutex::new(HashMap::new()),
            git_cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_home(mut self, home: PathBuf) -> VizEngine {
        self.home = home;
        self
    }

    pub fn with_schemas(mut self, schemas: Box<dyn Schemas>) -> VizEngine {
        self.schemas = schemas;
        self
    }

    pub fn with_listening(mut self, is_listening: Listening) -> VizEngine {
        self.is_listening = is_listening;
        self
    }

    pub fn build(&self, session: &Session, cwd_override: Option<&str>) -> VizBundle {
        let cwd = cwd_override.unwrap_or(&session.cwd);
        let main = match &session.transcript_path {
            Some(path) => self.facts(path, true, "main", facts::TAIL_BYTES),
            None => TranscriptFacts::default(),
        };
        let mut combined = main.clone();
        for agent in session.subagents.iter().take(MAX_SUBAGENT_FILES) {
            let Some(path) = &agent.transcript_path else {
                continue;
            };
            let origin = if agent.info.description.is_empty() {
                &agent.info.r#type
            } else {
                &agent.info.description
            };
            combined.merge(self.facts(path, false, origin, SUBAGENT_TAIL_BYTES));
        }

        let now = (self.now)();
        let signals = VizSignals {
            has_recent_mermaid: combined
                .mermaid
                .iter()
                .any(|block| block.time.is_some_and(|t| now - t < RECENT_MERMAID_SECS)),
            has_localhost_url: !main.local_urls.is_empty(),
        };
        let urls: Vec<String> = main
            .local_urls
            .iter()
            .chain(&combined.local_urls)
            .cloned()
            .collect();
        let subagents: Vec<wire::Subagent> =
            session.subagents.iter().map(|s| s.info.clone()).collect();
        VizBundle {
            flow: self.flow_spec(&main, &combined, cwd),
            diagram: Self::diagram_spec(&combined),
            chart: Self::chart_spec(&combined, &subagents),
            signals,
            web_url: web_url::resolve(&urls, cwd, &self.home, &*self.is_listening),
            web_candidates: web_url::live_candidates(
                &urls,
                cwd,
                &self.home,
                &*self.is_listening,
                5,
            ),
            device_hints: combined.device_hints,
        }
    }

    fn facts(&self, path: &str, is_main: bool, origin: &str, tail: u64) -> TranscriptFacts {
        let meta = std::fs::metadata(path).ok();
        let modified = meta.as_ref().and_then(|m| m.modified().ok());
        let size = meta.as_ref().map_or(0, |m| m.len());
        if let Some(cached) = lock(&self.facts_cache).get(path) {
            if cached.modified == modified && cached.size == size {
                return cached.facts.clone();
            }
        }
        let parsed = facts::analyze_file(path, is_main, origin, tail);
        lock(&self.facts_cache).insert(
            path.to_string(),
            CachedFacts {
                modified,
                size,
                facts: parsed.clone(),
            },
        );
        parsed
    }

    fn git_snapshot(&self, cwd: &str) -> GitSnapshot {
        let now = (self.now)();
        if let Some((at, snapshot)) = lock(&self.git_cache).get(cwd) {
            if now - at < GIT_TTL_SECS {
                return snapshot.clone();
            }
        }
        let snapshot = (self.git)(cwd);
        lock(&self.git_cache).insert(cwd.to_string(), (now, snapshot.clone()));
        snapshot
    }

    pub fn flow_spec(
        &self,
        main: &TranscriptFacts,
        combined: &TranscriptFacts,
        cwd: &str,
    ) -> wire::VizSpec {
        let keep = main.turns.len().saturating_sub(MAX_TURNS_SHOWN);
        let mut turns: Vec<wire::FlowTurn> = main.turns[keep..].to_vec();
        let mut budget = MAX_CALLS_SHOWN;
        for index in (0..turns.len()).rev() {
            let calls = &mut turns[index].calls;
            if calls.len() > budget {
                let cut = calls.len() - budget;
                calls.drain(..cut);
            }
            budget -= turns[index].calls.len();
            if budget == 0 {
                turns.drain(..index);
                break;
            }
        }
        turns.retain(|t| !(t.calls.is_empty() && t.prompt.is_empty()));
        let total_calls: usize = main.turns.iter().map(|t| t.calls.len()).sum();
        let git = self.git_snapshot(cwd);
        if total_calls == 0 && git.diff_stat.is_none() {
            return with_empty(
                wire::VizKind::Flow,
                "Chưa có công cụ nào chạy",
                Some("Dòng thời gian hiện khi Claude đọc, sửa file hoặc chạy lệnh."),
            );
        }
        let mut files: Vec<wire::FlowFile> = combined
            .file_edits
            .iter()
            .map(|(path, count)| wire::FlowFile {
                path: path.clone(),
                name: crate::transcript::text::last_path_component(path).to_string(),
                count: *count,
            })
            .collect();
        files.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
        let mut spec = blank(wire::VizKind::Flow);
        spec.flow = Some(wire::FlowSpec {
            branch: git.branch,
            diff_stat: git.diff_stat,
            files,
            turns,
            total_calls: total_calls as i64,
        });
        spec
    }

    pub fn diagram_spec(facts: &TranscriptFacts) -> wire::VizSpec {
        let mut ordered: Vec<(usize, &facts::MermaidBlock)> =
            facts.mermaid.iter().enumerate().collect();
        ordered.sort_by(
            |(left_at, left), (right_at, right)| match (left.time, right.time) {
                (Some(a), Some(b)) => b.total_cmp(&a).then(right_at.cmp(left_at)),
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, None) => right_at.cmp(left_at),
            },
        );
        if ordered.is_empty() {
            return with_empty(
                wire::VizKind::Diagram,
                "Chưa có sơ đồ nào",
                Some("Khi Claude trả lời bằng khối ```mermaid, sơ đồ hiện ở đây."),
            );
        }
        let blocks = ordered
            .iter()
            .enumerate()
            .map(|(index, (_, block))| {
                let first_line = block
                    .source
                    .split('\n')
                    .find(|l| !l.is_empty())
                    .unwrap_or("mermaid");
                wire::DiagramBlock {
                    id: format!("{}-{index}", block.message_id),
                    label: format!("{}. {first_line} ({})", index + 1, block.origin),
                    source: block.source.clone(),
                    origin: block.origin.clone(),
                    time: block.time.map(|t| t * 1000.0),
                }
            })
            .collect();
        let mut spec = blank(wire::VizKind::Diagram);
        spec.diagram = Some(wire::DiagramSpec { blocks });
        spec
    }

    pub fn entity_spec(&self, cwd: &str) -> wire::VizSpec {
        if cwd.is_empty() {
            return with_empty(wire::VizKind::Entity, "Session chưa có thư mục", None);
        }
        match self.schemas.scan(cwd) {
            Scan::Found(entity) => {
                let mut spec = blank(wire::VizKind::Entity);
                spec.entity = Some(entity);
                spec
            }
            Scan::NoTables => with_empty(
                wire::VizKind::Entity,
                "Không thấy schema trong thư mục này",
                Some("Console đọc Prisma (*.prisma), SQL (CREATE TABLE trong *.sql) và Drift (class ... extends Table)."),
            ),
            Scan::Unsupported => with_empty(
                wire::VizKind::Entity,
                "Máy này chưa đọc được schema",
                Some("Tab Entity chưa có trên bản Console này."),
            ),
        }
    }

    pub fn chart_spec(facts: &TranscriptFacts, subagents: &[wire::Subagent]) -> wire::VizSpec {
        let mut points: Vec<wire::UsagePoint> = facts
            .usage
            .iter()
            .filter(|p| p.time > 0.0)
            .cloned()
            .collect();
        points.sort_by(|a, b| a.time.total_cmp(&b.time));
        let sum = |pick: fn(&wire::UsagePoint) -> i64| points.iter().map(pick).sum::<i64>();
        let totals = wire::UsageTotals {
            input: sum(|p| p.input),
            output: sum(|p| p.output),
            cache_creation: sum(|p| p.cache_creation),
            cache_read: sum(|p| p.cache_read),
        };
        if points.len() > MAX_CHART_POINTS {
            let step = points.len() as f64 / MAX_CHART_POINTS as f64;
            points = (0..MAX_CHART_POINTS)
                .map(|n| points[(n as f64 * step) as usize].clone())
                .collect();
        }
        let mut tools: Vec<wire::ChartCount> = facts
            .tool_counts
            .iter()
            .map(|(name, count)| wire::ChartCount {
                name: name.clone(),
                count: *count,
            })
            .collect();
        tools.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
        tools.truncate(12);
        let mut by_model: HashMap<&str, i64> = HashMap::new();
        for agent in subagents.iter().filter(|a| !a.model.is_empty()) {
            *by_model.entry(agent.model.as_str()).or_default() += 1;
        }
        let mut models: Vec<wire::ModelCount> = by_model
            .into_iter()
            .map(|(model, count)| wire::ModelCount {
                model: model.to_string(),
                count,
            })
            .collect();
        models.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.model.cmp(&b.model)));
        if points.is_empty() && tools.is_empty() {
            return with_empty(
                wire::VizKind::Chart,
                "Chưa có số liệu",
                Some("Biểu đồ token và công cụ hiện khi session đã trả lời ít nhất một lần."),
            );
        }
        let mut spec = blank(wire::VizKind::Chart);
        spec.chart = Some(wire::ChartSpec {
            tokens: points,
            totals,
            tools,
            models,
        });
        spec
    }
}

fn blank(kind: wire::VizKind) -> wire::VizSpec {
    wire::VizSpec {
        kind,
        empty: None,
        error: None,
        flow: None,
        diagram: None,
        entity: None,
        chart: None,
    }
}

fn with_empty(kind: wire::VizKind, title: &str, detail: Option<&str>) -> wire::VizSpec {
    let mut spec = blank(kind);
    spec.empty = Some(wire::VizMessage {
        title: title.to_string(),
        detail: detail.map(String::from),
    });
    spec
}
