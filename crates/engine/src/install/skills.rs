//! Writing the `notchling-*` skills into the skills folders of Claude Code, Codex and Antigravity
//! CLI, and taking them out.
//!
//! - **The tool must be there.** A skill is no reason to create `~/.codex` for someone without
//!   Codex: when a tool's own folder is missing it is skipped.
//! - **Only folders we made are ours.** A folder is ours when it holds the [`MARK`] file we write
//!   into it. One without the mark (the user's own, or the one the Swift app wrote) is never
//!   written over or removed, and is reported. A link where the folder should be is refused too.
//! - **Removing deletes exactly the two files we wrote** (`SKILL.md` and the mark) and then the
//!   folder if it is empty; anything else the user put there stays, and so does the folder.
//! - **The text is an input.** Each skill is a [`SkillSpec`] with a function from the tool to
//!   its text, so the vault's text can come from where it is made.

use super::hooks::Want;
use crate::delegate::skill::SkillTool;
use std::fs;
use std::path::{Path, PathBuf};

/// The file that says a skill folder is ours.
pub const MARK: &str = ".notchling-skill";
const MARK_TEXT: &str = "Written by Notchling. Notchling updates and removes this skill; delete this file to keep the folder as it is.\n";
const SKILL_FILE: &str = "SKILL.md";

/// Each tool's own folder; its skills go in `<folder>/skills/<name>`.
#[derive(Clone, Debug)]
pub struct ToolRoots {
    /// `~/.claude` (or `CLAUDE_CONFIG_DIR`).
    pub claude: PathBuf,
    /// `~/.codex` (or `CODEX_HOME`).
    pub codex: PathBuf,
    /// `~/.gemini/config`.
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

/// A skill: its folder name, which tools get it, and its text for each.
pub struct SkillSpec {
    pub name: String,
    pub tools: Vec<SkillTool>,
    pub text: Box<dyn Fn(SkillTool) -> String + Send + Sync>,
}

/// What happens (or would) to one skill in one tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// The folder is made and written.
    Write,
    /// Our folder holds other text; it is rewritten.
    Update,
    Unchanged,
    Remove,
    /// Nothing there to remove.
    Absent,
    /// Left alone, for the reason given.
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

/// Whose a skill folder is.
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
            _ => done(Action::Skip, Some("the tool is not installed here")),
        };
    }
    match (owner(&dir), want) {
        (Owner::Other(why), _) => done(Action::Skip, Some(&format!("{why}; left alone"))),
        (Owner::Free, Want::Remove) => done(Action::Absent, None),
        (Owner::Ours, Want::Remove) => done(Action::Remove, None),
        (Owner::Free, _) => done(Action::Write, None),
        (Owner::Ours, _) => {
            let same =
                fs::read_to_string(dir.join(SKILL_FILE)).is_ok_and(|t| t == (spec.text)(tool));
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

/// What `want` does to each skill, in each tool it is for. Reads only.
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

/// What was done to one skill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillResult {
    pub plan: SkillPlan,
    /// An error, when the action could not be carried out.
    pub error: Option<String>,
}

/// Carries out [`plan_skills`]; one skill failing does not stop the others.
pub fn apply_skills(
    roots: &ToolRoots,
    specs: &[SkillSpec],
    wants: &[(String, Want)],
) -> Vec<SkillResult> {
    plan_skills(roots, specs, wants)
        .into_iter()
        .map(|plan| {
            let spec = specs.iter().find(|s| s.name == plan.name);
            let error = match (plan.action, spec) {
                (Action::Write | Action::Update, Some(spec)) => {
                    write_skill(&plan.dir, &(spec.text)(plan.tool)).err()
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
    // The mark first: a folder we started is ours even if the next write fails.
    notchling_platform::files::write_private_no_follow(&dir.join(MARK), MARK_TEXT.as_bytes())
        .map_err(|e| format!("the mark: {e}"))?;
    let file = dir.join(SKILL_FILE);
    if fs::symlink_metadata(&file).is_ok_and(|m| !m.is_file()) {
        return Err("SKILL.md is not a plain file; it was left alone".into());
    }
    write_text(&file, text).map_err(|e| say("SKILL.md", e))
}

/// An atomic write of a world-readable text file (a skill is read by the tool, not secret).
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
    // Only an empty folder goes; the user's own files in it stay.
    let _ = fs::remove_dir(dir);
    Ok(())
}
