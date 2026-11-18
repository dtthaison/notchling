use super::ids::{is_uuid, valid_model};
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const RUN_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_OUTPUT: u64 = 256 * 1024;
const MAX_TITLE: usize = 120;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiError {
    Missing,
    Failed(String),
    TimedOut,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Missing => write!(f, "agentapi was not found"),
            ApiError::Failed(why) => write!(f, "{why}"),
            ApiError::TimedOut => write!(f, "agentapi took too long"),
        }
    }
}

pub trait AgentApi: Send + Sync {
    fn run(&self, args: &[String]) -> Result<String, ApiError>;
}

pub fn new_conversation_args(
    text: &str,
    model: Option<&str>,
    title: Option<&str>,
) -> Result<Vec<String>, String> {
    let mut args = vec!["new-conversation".to_string()];
    if let Some(model) = model {
        if !valid_model(model) {
            return Err("that model name isn't one the sidecar will pass on".into());
        }
        args.push(format!("--model={model}"));
    }
    if let Some(title) = title.map(clean_title).filter(|t| !t.is_empty()) {
        args.push(format!("--title={title}"));
    }
    args.push("--".into());
    args.push(text.to_string());
    Ok(args)
}

pub fn send_message_args(conversation: &str, text: &str) -> Result<Vec<String>, String> {
    if !is_uuid(conversation) {
        return Err("that is not a conversation id".into());
    }
    Ok(vec![
        "send-message".into(),
        conversation.to_string(),
        text.to_string(),
    ])
}

fn clean_title(title: &str) -> String {
    title
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_TITLE)
        .collect()
}

pub fn new_conversation_id(output: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(output.trim())
        .map_err(|_| "agentapi's answer was not JSON".to_string())?;
    let id = value
        .pointer("/response/newConversation/conversationId")
        .and_then(Value::as_str)
        .ok_or_else(|| "agentapi's answer had no conversation id".to_string())?;
    if is_uuid(id) {
        Ok(id.to_string())
    } else {
        Err("agentapi's conversation id was not an id".into())
    }
}

pub fn message_was_sent(output: &str) -> bool {
    serde_json::from_str::<Value>(output.trim())
        .ok()
        .is_some_and(|v| v.pointer("/response/sendMessage").is_some())
}

pub struct RealAgentApi {
    binary: PathBuf,
    timeout: Duration,
}

impl RealAgentApi {
    pub fn new(binary: PathBuf) -> RealAgentApi {
        RealAgentApi {
            binary,
            timeout: RUN_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> RealAgentApi {
        self.timeout = timeout;
        self
    }

    pub fn find(path_var: Option<&str>, fallback: PathBuf) -> PathBuf {
        let found = path_var.and_then(|paths| {
            std::env::split_paths(paths)
                .map(|dir| dir.join("agentapi"))
                .find(|p| p.is_file())
        });
        found.unwrap_or(fallback)
    }
}

fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = (&mut pipe).take(MAX_OUTPUT).read_to_string(&mut text);
        text
    })
}

impl AgentApi for RealAgentApi {
    fn run(&self, args: &[String]) -> Result<String, ApiError> {
        let mut child = Command::new(&self.binary)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => ApiError::Missing,
                _ => ApiError::Failed(format!("agentapi could not start: {}", e.kind())),
            })?;
        let out = child.stdout.take().map(drain);
        let err = child.stderr.take().map(drain);
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() >= self.timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ApiError::TimedOut);
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(15)),
                Err(e) => return Err(ApiError::Failed(format!("agentapi: {}", e.kind()))),
            }
        };
        let stdout = out.and_then(|h| h.join().ok()).unwrap_or_default();
        let stderr = err.and_then(|h| h.join().ok()).unwrap_or_default();
        if status.success() {
            Ok(stdout)
        } else {
            let line = stderr
                .lines()
                .chain(stdout.lines())
                .find(|l| !l.trim().is_empty())
                .unwrap_or("it failed");
            Err(ApiError::Failed(format!(
                "agentapi: {}",
                line.chars().take(200).collect::<String>()
            )))
        }
    }
}
