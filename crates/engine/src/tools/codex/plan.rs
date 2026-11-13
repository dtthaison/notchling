use std::path::{Path, PathBuf};

pub const WORKSPACE_WRITE: &str = "workspace-write";

pub const MAX_PROMPT_BYTES: usize = 100 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pick {
    pub model: Option<String>,
    pub effort: Option<String>,
}

pub struct Turn<'a> {
    pub prompt: &'a str,
    pub session: Option<&'a str>,
    pub folder: &'a str,
    pub pick: &'a Pick,
    pub network: bool,
    pub codex_home: Option<&'a Path>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: Vec<(String, Option<String>)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub signaled: bool,
}

pub fn arguments(
    prompt: &str,
    session: Option<&str>,
    folder: &str,
    pick: &Pick,
    sandbox: &str,
    network: bool,
) -> Vec<String> {
    let mut common: Vec<String> = vec![
        "--json".into(),
        "--skip-git-repo-check".into(),
        "-c".into(),
        format!("sandbox_mode=\"{sandbox}\""),
    ];
    if network && sandbox == WORKSPACE_WRITE {
        common.push("-c".into());
        common.push("sandbox_workspace_write.network_access=true".into());
    }
    if let Some(model) = &pick.model {
        common.push("-m".into());
        common.push(model.clone());
    }
    if let Some(effort) = &pick.effort {
        common.push("-c".into());
        common.push(format!("model_reasoning_effort=\"{effort}\""));
    }
    let mut args: Vec<String> = vec!["exec".into()];
    if session.is_some() {
        args.push("resume".into());
    }
    args.extend(common);
    if session.is_none() {
        args.push("-C".into());
        args.push(folder.into());
    }
    if prompt.starts_with('-') {
        args.push("--".into());
    }
    if let Some(session) = session {
        args.push(session.into());
    }
    args.push(prompt.into());
    args
}

pub fn turn_spec(codex: &Path, turn: &Turn<'_>, path: Option<&str>) -> TurnSpec {
    let mut env = vec![("NO_COLOR".to_string(), None)];
    if let Some(path) = path {
        env.push(("PATH".to_string(), Some(path.to_string())));
    }
    if let Some(home) = turn.codex_home {
        env.push((
            "CODEX_HOME".to_string(),
            Some(home.to_string_lossy().into_owned()),
        ));
    }
    TurnSpec {
        program: codex.to_path_buf(),
        args: arguments(
            turn.prompt,
            turn.session,
            turn.folder,
            turn.pick,
            WORKSPACE_WRITE,
            turn.network,
        ),
        cwd: turn.folder.to_string(),
        env,
    }
}

pub fn failure_of(exit: &ExitInfo, last_error: &str) -> Option<String> {
    if exit.signaled || exit.code == Some(0) || exit.code == Some(130) {
        return None;
    }
    if !last_error.is_empty() {
        return Some(last_error.to_string());
    }
    Some(match exit.code {
        Some(code) => format!("Codex exited with code {code}."),
        None => "Codex exited without a status.".to_string(),
    })
}

pub fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('-')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

pub fn valid_model(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 64
        && model.starts_with(|c: char| c.is_ascii_alphanumeric())
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub fn valid_effort(effort: &str) -> bool {
    !effort.is_empty()
        && effort.len() <= 24
        && effort
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

const TOOL_DIRS: [&str; 3] = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"];

pub fn augmented_path(current: &str, home: &Path) -> String {
    let local = home
        .join(".local")
        .join("bin")
        .to_string_lossy()
        .into_owned();
    let mut parts: Vec<String> = current
        .split(':')
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect();
    for extra in TOOL_DIRS.iter().map(|d| d.to_string()).chain([local]) {
        if !parts.contains(&extra) {
            parts.push(extra);
        }
    }
    parts.join(":")
}

pub fn find_binary(home: &Path, path: &str, is_file: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let usual = [
        home.join(".local").join("bin").join("codex"),
        PathBuf::from("/opt/homebrew/bin/codex"),
        PathBuf::from("/usr/local/bin/codex"),
        home.join(".npm-global").join("bin").join("codex"),
    ];
    usual
        .into_iter()
        .chain(
            path.split(':')
                .filter(|dir| Path::new(dir).is_absolute())
                .map(|dir| Path::new(dir).join("codex")),
        )
        .find(|candidate| is_file(candidate))
}

pub fn find_fixture_binary(root: &Path, is_file: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    Some(root.join(".local").join("bin").join("codex")).filter(|c| is_file(c))
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
