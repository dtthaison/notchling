use super::model::*;
use super::test_support::NOW_MS;
use notchling_protocol::generated as wire;
use std::collections::BTreeMap;

pub const SESSION_A: &str = "5b6d9a30-6f0e-4d0a-9b57-0c1f3a2e8d11";
pub const SESSION_B: &str = "c3a1e5f2-0b4d-4f6e-8a7c-2d9e1b0f4a65";
pub const SESSION_C: &str = "e8f0a7b4-3c2d-4a19-b6e5-7f1d0c9a2b38";
pub const SESSION_D: &str = "codex:0199e0c5-7a31-7c52-9d4e-5b8a1f2c3d60";
pub const SESSION_E: &str = "1f4b8c27-9d3e-4a60-8b15-6c0e2a7d9f43";

pub fn message_id(n: usize) -> String {
    format!("0A000000-0000-4000-8000-00000000000{n}")
}

pub fn permission() -> PendingPrompt {
    PendingPrompt {
        id: "req-0001".into(),
        session_id: SESSION_A.into(),
        kind: PromptKind::Permission,
        tool_name: "Bash".into(),
        summary: "rm -rf build".into(),
        created_ms: NOW_MS,
    }
}

pub fn question() -> PendingPrompt {
    PendingPrompt {
        id: "req-0002".into(),
        session_id: SESSION_C.into(),
        kind: PromptKind::Question(vec![PromptQuestion {
            header: "Database".into(),
            question: "Which database should the service use?".into(),
            options: vec!["SQLite".into(), "Postgres".into()],
            multi_select: false,
        }]),
        tool_name: "AskUserQuestion".into(),
        summary: "Which database should the service use?".into(),
        created_ms: NOW_MS,
    }
}

fn base(id: &str, name: &str, cwd: &str, control: Control, state: SessionState) -> Session {
    let mut s = Session::new(id, name, cwd, control, state);
    s.status_updated_ms = NOW_MS;
    s
}

pub fn sessions() -> Vec<Session> {
    let transcript = format!("/Users/example/.claude/projects/app/{SESSION_A}.jsonl");
    let mut a = base(
        SESSION_A,
        "Fix the launch crash",
        "/Users/example/src/app",
        Control::ConsolePty,
        SessionState::NeedsInput,
    );
    a.pid = Some(4100);
    a.started_ms = NOW_MS - 3_600_000.0;
    a.transcript_path = Some(transcript.clone());
    a.subagents = vec![SessionSubagent {
        info: Subagent {
            id: "agent-a1b2c3".into(),
            r#type: "Explore".into(),
            description: "Find the crash site".into(),
            model: "haiku".into(),
            depth: 1,
            finished: false,
            state: wire::SubagentState::Running,
            last_activity: NOW_MS - 5000.0,
        },
        transcript_path: Some(transcript),
    }];
    a.pending = Some(permission());
    a.activity = Some("Editing main.swift".into());
    a.facts = Some(Facts {
        context_tokens: Some(52_000),
        context_window: Some(400_000),
        model: Some("claude-opus-5-5".into()),
        limits: vec![
            Limit {
                label: "5h".into(),
                percent: 42.0,
                resets_at: Some(NOW_MS + 7_200_000.0),
            },
            Limit {
                label: "Week".into(),
                percent: 12.5,
                resets_at: None,
            },
        ],
    });
    a.last_message = Some(LastMessage {
        id: message_id(6),
        text: "The crash was a nil optional in main.swift.".into(),
    });

    let mut b = base(
        SESSION_B,
        "Review the diff",
        "/Users/example/src/notes",
        Control::ViewOnly,
        SessionState::Running,
    );
    b.pid = Some(3900);
    b.entrypoint = "desktop".into();
    b.started_ms = NOW_MS - 7_200_000.0;
    b.takeover = Some(TakeoverState::Stoppable { busy: true });

    let mut c = base(
        SESSION_C,
        "Pick a database",
        "/Users/example/src/app",
        Control::Job("job-91ab".into()),
        SessionState::NeedsInput,
    );
    c.entrypoint = "claude-vscode".into();
    c.kind = "background".into();
    c.started_ms = NOW_MS - 600_000.0;
    c.pending = Some(question());

    let mut d = base(
        SESSION_D,
        "Port the parser",
        "/Users/example/src/parser",
        Control::Job("codex".into()),
        SessionState::Done,
    );
    d.entrypoint = "codex".into();
    d.kind = "codex".into();
    d.status_updated_ms = NOW_MS - 90_000.0;
    d.started_ms = NOW_MS - 1_800_000.0;
    d.tool = "codex".into();

    let mut e = base(
        SESSION_E,
        "Old experiment",
        "/Users/example/src/lab",
        Control::ViewOnly,
        SessionState::Exited,
    );
    e.entrypoint = "sdk".into();
    e.status_updated_ms = NOW_MS - 86_400_000.0;
    e.started_ms = NOW_MS - 90_000_000.0;
    e.takeover = Some(TakeoverState::Available);
    vec![a, b, c, d, e]
}

fn msg(n: usize, role: wire::ChatRole, text: &str, offset_s: f64) -> ChatMessage {
    ChatMessage {
        id: message_id(n),
        role,
        text: text.into(),
        time: NOW_MS + offset_s * 1000.0,
        tool_summary: None,
        queued: false,
        images: Vec::new(),
        kind: None,
        steps: None,
        output: None,
    }
}

fn step(name: &str, output: Option<&str>, diff: Option<Vec<&str>>) -> wire::ToolStep {
    wire::ToolStep {
        name: name.into(),
        summary: "main.swift".into(),
        output: output.map(String::from),
        path: Some("/Users/example/src/app/main.swift".into()),
        diff: diff.map(|rows| rows.into_iter().map(String::from).collect()),
    }
}

pub fn models() -> Vec<ModelChoice> {
    [
        ("claude-fable-5-2", "Fable 5.2"),
        ("claude-opus-5-5", "Opus 5.5"),
    ]
    .into_iter()
    .map(|(id, name)| ModelChoice {
        id: id.into(),
        name: name.into(),
    })
    .collect()
}

pub fn chat() -> Vec<ChatMessage> {
    use wire::ChatRole::*;
    let mut first = msg(1, User, "The app crashes on launch. Can you look?", -300.0);
    first.images = vec![wire::Image {
        id: "b-4096-0".into(),
        media_type: "image/png".into(),
        width: Some(1170),
        height: Some(2532),
        origin: wire::ImageOrigin::User,
        bytes: 245_760,
    }];
    let mut tools = msg(3, Tools, "", -270.0);
    tools.tool_summary = Some("Read 1 file, edited 1 file".into());
    tools.steps = Some(vec![
        step("Read", Some("import AppKit\n"), None),
        step(
            "Edit",
            None,
            Some(vec![" let a = 1", "-let b = a!", "+let b = a ?? 0"]),
        ),
    ]);
    let mut shell = msg(4, User, "!git status --short", -200.0);
    shell.kind = Some(wire::ChatKind::Shell);
    shell.output = Some(" M main.swift\n".into());
    let mut queued = msg(5, User, "Also add a test", -100.0);
    queued.queued = true;
    vec![
        first,
        msg(2, Assistant, "I'll read `main.swift` first.", -280.0),
        tools,
        shell,
        queued,
        msg(
            6,
            Assistant,
            "The crash was a nil optional in main.swift.",
            -10.0,
        ),
    ]
}

pub fn compaction() -> ChatMessage {
    let mut m = msg(7, wire::ChatRole::Assistant, "Conversation compacted.", 0.0);
    m.kind = Some(wire::ChatKind::Compaction);
    m
}

pub fn commands() -> Vec<Command> {
    vec![
        Command {
            name: "compact".into(),
            description: "Summarise the conversation to free context".into(),
            argument_hint: Some("[instructions]".into()),
        },
        Command {
            name: "clear".into(),
            description: "Start a new conversation".into(),
            argument_hint: None,
        },
    ]
}

pub fn viz(include_entity: bool) -> BridgeViz {
    let spec = |kind| VizSpec {
        kind,
        empty: None,
        error: None,
        flow: None,
        diagram: None,
        entity: None,
        chart: None,
    };
    let mut flow = spec(wire::VizKind::Flow);
    flow.flow = Some(wire::FlowSpec {
        branch: Some("main".into()),
        diff_stat: Some("2 files changed, 14 insertions(+), 3 deletions(-)".into()),
        files: vec![wire::FlowFile {
            path: "/Users/example/src/app/main.swift".into(),
            name: "main.swift".into(),
            count: 3,
        }],
        turns: vec![wire::FlowTurn {
            index: 1,
            prompt: "Fix the crash on launch".into(),
            calls: vec![
                wire::FlowCall {
                    tool: "Read".into(),
                    target: "main.swift".into(),
                    subagent: false,
                    edit: false,
                    time: Some(1_793_541_400_000.0),
                },
                wire::FlowCall {
                    tool: "Edit".into(),
                    target: "main.swift".into(),
                    subagent: false,
                    edit: true,
                    time: None,
                },
            ],
        }],
        total_calls: 2,
    });
    let mut diagram = spec(wire::VizKind::Diagram);
    diagram.diagram = Some(wire::DiagramSpec {
        blocks: vec![wire::DiagramBlock {
            id: "d-1".into(),
            label: "Launch sequence".into(),
            source: "sequenceDiagram\n  A->>B: hi".into(),
            origin: "assistant".into(),
            time: Some(1_793_541_300_000.0),
        }],
    });
    let mut chart = spec(wire::VizKind::Chart);
    chart.chart = Some(wire::ChartSpec {
        tokens: vec![wire::UsagePoint {
            time: 1_793_541_300_000.0,
            input: 1200,
            output: 340,
            cache_creation: 0,
            cache_read: 9000,
        }],
        totals: wire::UsageTotals {
            input: 1200,
            output: 340,
            cache_creation: 0,
            cache_read: 9000,
        },
        tools: vec![
            wire::ChartCount {
                name: "Read".into(),
                count: 4,
            },
            wire::ChartCount {
                name: "Edit".into(),
                count: 2,
            },
        ],
        models: vec![wire::ModelCount {
            model: "claude-opus-5-5".into(),
            count: 6,
        }],
    });
    let mut specs = BTreeMap::from([
        ("flow".to_string(), flow),
        ("diagram".to_string(), diagram),
        ("chart".to_string(), chart),
    ]);
    if include_entity {
        let mut entity = spec(wire::VizKind::Entity);
        entity.entity = Some(wire::EntitySpec {
            mermaid: "erDiagram\n  USER ||--o{ ORDER : places".into(),
            tables: vec![
                wire::EntityTable {
                    id: "USER".into(),
                    name: "user".into(),
                    columns: 3,
                },
                wire::EntityTable {
                    id: "ORDER".into(),
                    name: "order".into(),
                    columns: 4,
                },
            ],
            links: vec![wire::EntityLink {
                from: "USER".into(),
                to: "ORDER".into(),
                inferred: false,
            }],
            files: vec![wire::EntityFile {
                path: "db/schema.sql".into(),
                tables: 2,
            }],
            table_count: 2,
            relation_count: 1,
            inferred_count: 0,
            warnings: vec!["no primary key on order_item".into()],
        });
        specs.insert("entity".into(), entity);
    }
    BridgeViz {
        suggested_tab: if include_entity { "entity" } else { "flow" }.into(),
        web_url: Some("http://localhost:3000".into()),
        specs,
        web_candidates: vec![
            "http://localhost:3000".into(),
            "http://localhost:5173".into(),
        ],
    }
}
