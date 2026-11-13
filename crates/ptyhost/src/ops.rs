use crate::host::HostedInfo;
use crate::server::{object, Attachments, PROTOCOL_VERSION};
use crate::state::HostSettings;
use crate::wire::{int, SessionId};
use crate::HostError;
use serde_json::{json, Map, Value};
use std::path::Path;

fn missing(key: &str) -> HostError {
    HostError::Refused(format!("Thiếu \"{key}\"."))
}

fn string<'a>(message: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    message.get(key).and_then(Value::as_str)
}

fn session_of(message: &Map<String, Value>, key: &str) -> Result<SessionId, HostError> {
    string(message, key)
        .and_then(SessionId::parse)
        .ok_or_else(|| missing(key))
}

fn clamp_u16(value: Option<i64>, fallback: u16) -> u16 {
    value.map_or(fallback, |v| v.clamp(0, i64::from(u16::MAX)) as u16)
}

pub(crate) fn perform(
    connection: &impl Attachments,
    op: &str,
    message: &Map<String, Value>,
) -> Result<Map<String, Value>, HostError> {
    let host = connection.host();
    match op {
        "hello" => Ok(object(json!({
            "version": PROTOCOL_VERSION,
            "pid": std::process::id(),
            "running": host.running_count(),
        }))),
        "status" => Ok(object(json!({
            "running": host.running_count(),
            "total": host.list().len(),
        }))),
        "list" => {
            let sessions: Vec<Value> = host.list().iter().map(info_json).collect();
            Ok(object(json!({ "sessions": sessions })))
        }
        "spawn" => {
            let cwd = Path::new(string(message, "cwd").ok_or_else(|| missing("cwd"))?);
            let info = match string(message, "kind").unwrap_or("claude") {
                "claude" => host.spawn_claude(cwd, string(message, "model"))?,
                "command" => {
                    let executable =
                        string(message, "executable").ok_or_else(|| missing("executable"))?;
                    let arguments: Vec<String> = message
                        .get("arguments")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    host.spawn_command(Path::new(executable), &arguments, cwd)?
                }
                _ => return Err(HostError::Refused("kind không hợp lệ.".to_string())),
            };
            Ok(object(info_json(&info)))
        }
        "resume" => {
            let (Some(session), Some(cwd)) = (string(message, "sessionId"), string(message, "cwd"))
            else {
                return Err(HostError::Refused("Thiếu sessionId hoặc cwd.".to_string()));
            };
            Ok(object(info_json(&host.resume(session, Path::new(cwd))?)))
        }
        "resize" => {
            let id = session_of(message, "session")?;
            if let Some(handle) = host.session(id) {
                handle.resize(
                    clamp_u16(int(message.get("columns")), 200),
                    clamp_u16(int(message.get("rows")), 50),
                );
            }
            Ok(Map::new())
        }
        "attach" => {
            connection.attach(session_of(message, "session")?)?;
            Ok(Map::new())
        }
        "detach" => {
            connection.detach(session_of(message, "session")?);
            Ok(Map::new())
        }
        "terminate" => {
            let id = session_of(message, "session")?;
            if message.get("confirm").and_then(Value::as_bool) != Some(true) {
                return Err(HostError::Refused(
                    "Cần confirm: true để dừng session.".to_string(),
                ));
            }
            host.terminate(id);
            Ok(Map::new())
        }
        "settings.get" => Ok(object(json!({
            "resumeOnStart": host.state.settings().resume_on_start
        }))),
        "settings.set" => {
            let mut settings: HostSettings = host.state.settings();
            if let Some(value) = message.get("resumeOnStart").and_then(Value::as_bool) {
                settings.resume_on_start = value;
            }
            host.state.set_settings(settings);
            Ok(object(json!({ "resumeOnStart": settings.resume_on_start })))
        }
        _ => Err(HostError::Refused(format!("Không có lệnh \"{op}\"."))),
    }
}

pub fn info_json(info: &HostedInfo) -> Value {
    let mut out = json!({
        "id": info.id.to_string(),
        "cwd": info.cwd,
        "pid": info.pid,
        "startedAt": info.started_at_ms,
        "running": info.running,
    });
    if let Some(map) = out.as_object_mut() {
        if let Some(model) = &info.model {
            map.insert("model".into(), json!(model));
        }
        if let Some(claude) = &info.claude_session_id {
            map.insert("claudeSessionId".into(), json!(claude));
        }
    }
    out
}

pub fn info_from(value: &Value) -> Option<HostedInfo> {
    let object = value.as_object()?;
    Some(HostedInfo {
        id: SessionId::parse(object.get("id")?.as_str()?)?,
        cwd: object.get("cwd")?.as_str()?.to_string(),
        model: object
            .get("model")
            .and_then(Value::as_str)
            .map(String::from),
        pid: int(object.get("pid"))
            .and_then(|p| i32::try_from(p).ok())
            .unwrap_or(0),
        started_at_ms: int(object.get("startedAt")).unwrap_or(0),
        running: object
            .get("running")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        claude_session_id: object
            .get("claudeSessionId")
            .and_then(Value::as_str)
            .map(String::from),
    })
}
