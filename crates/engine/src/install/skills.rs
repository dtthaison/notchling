use super::hooks::Want;
use crate::delegate::skill::SkillTool;
use std::fs;
use std::path::{Path, PathBuf};

pub const MARK: &str = ".notchling-skill";
const MARK_TEXT: &str = "Written by Notchling. Notchling updates and removes this skill; delete this file to keep the folder as it is.\n";
const SKILL_FILE: &str = "SKILL.md";

pub const CONTRACT: &str = "## Exit codes and `--json` for every Notchling command

Run the command with `help --json` to read every command, its flags and its exit codes. The codes are the same for every command (`secret` passes its own command's code through):

- 0: done.
- 1: Notchling tried and it didn't work, or the request was withdrawn. Read the error; retry if it makes sense.
- 2: the command line or a value is wrong; nothing changed.
- 3: Notchling isn't running, didn't answer, or is older than this command. Ask the user to open it.
- 4: refused: a switch is off, only the user may do it, or it isn't yours. Follow `error.ask`; when `error.never` is true, stop.
- 5: the user said no, dismissed or cancelled. Stop; don't ask again at once.
- 6: the request is saved and still waits for the user; its id is in the output. A command that waits can take minutes: run it in the background.
- 7: a limit was reached. Wait `error.retryAfterMs`, then retry.
- 124: no answer in time; the request is closed. Ask again only if you still need it.
- 125: `secret` only: Notchling stopped before running your command; read stderr.

With `--json`, stdout holds exactly one JSON value and stderr stays empty. A failure is `{\"error\": {\"exit\", \"code\", \"reason\", \"message\", \"ask\", \"never\", ...}}`. Without `--json`, every failure line ends with the code's name, such as `(denied)`: read the name when this text is out of date.
";

pub const TOOL_MISSING: &str = "the tool is not installed here";

#[derive(Clone, Debug)]
pub struct ToolRoots {
    pub claude: PathBuf,
    pub codex: PathBuf,
    pub antigravity: PathBuf,
}

impl ToolRoots {
    pub fn root(&self, tool: SkillTool) -> &Path {
        match tool {
            SkillTool::Claude => &self.claude,
            SkillTool::Codex => &self.codex,
            SkillTool::Antigravity => &self.antigravity,
        }
    }
}

pub fn tool_name(tool: SkillTool) -> &'static str {
    match tool {
        SkillTool::Claude => "claude",
        SkillTool::Codex => "codex",
        SkillTool::Antigravity => "antigravity",
    }
}

pub struct SkillSpec {
    pub name: String,
    pub tools: Vec<SkillTool>,
    pub text: Box<dyn Fn(SkillTool) -> String + Send + Sync>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Write,
    Update,
    Unchanged,
    Remove,
    Absent,
    Skip,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Write => "write",
            Action::Update => "update",
            Action::Unchanged => "unchanged",
            Action::Remove => "remove",
            Action::Absent => "absent",
            Action::Skip => "skip",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillPlan {
    pub name: String,
    pub tool: SkillTool,
    pub dir: PathBuf,
    pub action: Action,
    pub reason: Option<String>,
}

enum Owner {
    Free,
    Ours,
    Other(&'static str),
}

fn owner(dir: &Path) -> Owner {
    let Ok(meta) = fs::symlink_metadata(dir) else {
        return Owner::Free;
    };
    if meta.file_type().is_symlink() {
        return Owner::Other("it is a link");
    }
    if !meta.is_dir() {
        return Owner::Other("it is not a folder");
    }
    let mark = fs::symlink_metadata(dir.join(MARK)).is_ok_and(|m| m.is_file());
    if mark {
        return Owner::Ours;
    }
    let swift = fs::read_to_string(dir.join(SKILL_FILE))
        .is_ok_and(|t| t.lines().any(|l| l.trim().starts_with("name: notchling-")));
    if swift {
        Owner::Other("the Swift app wrote it")
    } else {
        Owner::Other("it is not one Notchling made")
    }
}

fn skill_dir(roots: &ToolRoots, tool: SkillTool, name: &str) -> PathBuf {
    roots.root(tool).join("skills").join(name)
}

pub fn rendered(spec: &SkillSpec, tool: SkillTool) -> String {
    let text = (spec.text)(tool);
    if spec.name == "notchling" || spec.name.starts_with("notchling-") {
        format!("{}\n{CONTRACT}", text.trim_end())
    } else {
        text
    }
}

fn plan_one(roots: &ToolRoots, spec: &SkillSpec, tool: SkillTool, want: Want) -> SkillPlan {
    let dir = skill_dir(roots, tool, &spec.name);
    let done = |action, reason: Option<&str>| SkillPlan {
        name: spec.name.clone(),
        tool,
        dir: dir.clone(),
        action,
        reason: reason.map(str::to_string),
    };
    if !roots.root(tool).is_dir() {
        return match want {
            Want::Remove => done(Action::Absent, None),
            _ => done(Action::Skip, Some(TOOL_MISSING)),
        };
    }
    match (owner(&dir), want) {
        (Owner::Other(why), _) => done(Action::Skip, Some(&format!("{why}; left alone"))),
        (Owner::Free, Want::Remove) => done(Action::Absent, None),
        (Owner::Ours, Want::Remove) => done(Action::Remove, None),
        (Owner::Free, _) => done(Action::Write, None),
        (Owner::Ours, _) => {
            let same =
                fs::read_to_string(dir.join(SKILL_FILE)).is_ok_and(|t| t == rendered(spec, tool));
            done(
                if same {
                    Action::Unchanged
                } else {
                    Action::Update
                },
                None,
            )
        }
    }
}

pub fn plan_skills(
    roots: &ToolRoots,
    specs: &[SkillSpec],
    wants: &[(String, Want)],
) -> Vec<SkillPlan> {
    let mut out = Vec::new();
    for spec in specs {
        let Some(&(_, want)) = wants.iter().find(|(n, _)| *n == spec.name) else {
            continue;
        };
        if want == Want::Leave {
            continue;
        }
        for &tool in &spec.tools {
            out.push(plan_one(roots, spec, tool, want));
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillResult {
    pub plan: SkillPlan,
    pub error: Option<String>,
}

pub const STALE: &str = "stale";

pub fn part_id(tool: SkillTool, name: &str) -> String {
    format!("skill:{}:{name}", tool_name(tool))
}

pub fn skill_texts(spec: &SkillSpec, plan: &SkillPlan) -> (Option<String>, Option<String>) {
    let before = fs::read_to_string(plan.dir.join(SKILL_FILE)).ok();
    let after = match plan.action {
        Action::Write | Action::Update | Action::Unchanged => Some(rendered(spec, plan.tool)),
        _ => None,
    };
    (before, after)
}

pub fn apply_skills(
    roots: &ToolRoots,
    specs: &[SkillSpec],
    wants: &[(String, Want)],
) -> Vec<SkillResult> {
    apply_skills_only(roots, specs, wants, None)
}

pub type Only = std::collections::BTreeMap<String, Option<String>>;

pub fn apply_skills_only(
    roots: &ToolRoots,
    specs: &[SkillSpec],
    wants: &[(String, Want)],
    only: Option<&Only>,
) -> Vec<SkillResult> {
    plan_skills(roots, specs, wants)
        .into_iter()
        .filter(|plan| only.is_none_or(|o| o.contains_key(&part_id(plan.tool, &plan.name))))
        .map(|plan| {
            let spec = specs.iter().find(|s| s.name == plan.name);
            let writes = matches!(plan.action, Action::Write | Action::Update | Action::Remove);
            let counts = writes || plan.action == Action::Skip;
            let stale = only
                .and_then(|o| o.get(&part_id(plan.tool, &plan.name)))
                .filter(|_| counts)
                .is_some_and(|expect| {
                    let now = fs::read_to_string(plan.dir.join(SKILL_FILE))
                        .ok()
                        .map(|t| super::file::text_hash(&t));
                    plan.action == Action::Skip || now != *expect
                });
            let error = match (plan.action, spec) {
                _ if stale => Some(STALE.to_string()),
                (Action::Write | Action::Update, Some(spec)) => {
                    write_skill(&plan.dir, &rendered(spec, plan.tool)).err()
                }
                (Action::Remove, _) => remove_skill(&plan.dir).err(),
                _ => None,
            };
            SkillResult { plan, error }
        })
        .collect()
}

fn write_skill(dir: &Path, text: &str) -> Result<(), String> {
    let say = |what: &str, e: std::io::Error| format!("{what}: {e}");
    fs::create_dir_all(dir).map_err(|e| say("the folder", e))?;
    notchling_platform::files::write_private_no_follow(&dir.join(MARK), MARK_TEXT.as_bytes())
        .map_err(|e| format!("the mark: {e}"))?;
    let file = dir.join(SKILL_FILE);
    if fs::symlink_metadata(&file).is_ok_and(|m| !m.is_file()) {
        return Err("SKILL.md is not a plain file; it was left alone".into());
    }
    write_text(&file, text).map_err(|e| say("SKILL.md", e))
}

fn write_text(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let temp = path.with_file_name(format!(".SKILL.md.notchling-{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn remove_skill(dir: &Path) -> Result<(), String> {
    for name in [SKILL_FILE, MARK] {
        let file = dir.join(name);
        match fs::symlink_metadata(&file) {
            Ok(m) if m.is_file() => {
                fs::remove_file(&file).map_err(|e| format!("{name}: {e}"))?;
            }
            Ok(_) => return Err(format!("{name} is not a plain file; it was left alone")),
            Err(_) => {}
        }
    }
    let _ = fs::remove_dir(dir);
    Ok(())
}
