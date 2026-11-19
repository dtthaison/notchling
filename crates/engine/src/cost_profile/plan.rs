use super::action::*;
use super::detect::{detect, ours, Detected, OLD_RULE_REF, ORCHESTRATOR_FILE, PSTACK_RULE};
use super::diff::unified;
use super::import::{apply_to, import_rules};
use super::keys::claude_keys;
use super::manifest::Part;
use super::model::Profile;
use super::pstack;
use super::render::{self as r, Features};
use crate::catalog::Tool;
use crate::install::file::{load, State as FileState};
use std::path::Path;

pub const ID_RULE: &str = "claude.rule";
pub const ID_BUILDER: &str = "claude.agent.builder";
pub const ID_READER: &str = "claude.agent.reader";
pub const ID_REVIEWER: &str = "claude.agent.reviewer";
pub const ID_ORCHESTRATOR: &str = "claude.orchestrator";
pub const ID_SETTINGS: &str = "claude.settings";
pub const ID_PSTACK: &str = "claude.pstack";
pub const ID_OWN_RULES: &str = "claude.rules.replaced";
pub const ID_POINTER: &str = "claude.pointer.";

fn gate(mut a: Action, opts: &Options) -> Action {
    if a.kind.writes() && !opts.allows(a.needs) {
        a.kind = Kind::Offer;
    }
    a
}

#[allow(clippy::too_many_arguments)]
pub fn file_action(
    env: &Env,
    opts: &Options,
    id: &str,
    tool: Tool,
    path: &Path,
    new_text: &str,
    needs: Option<&'static str>,
    takeover: bool,
) -> Action {
    let mut a = Action::new(id, tool, path, Part::File);
    a.needs = needs;
    let current = match read_text(path) {
        Ok(t) => t,
        Err(why) => return a.skip(&format!("unreadable: {why}")),
    };
    let entry = env.manifest.get(id).filter(|e| !e.removed);
    let forced = opts.force.contains(id);
    a.after = Some(new_text.to_string());
    a.diff = unified(
        &path.to_string_lossy(),
        current.as_deref().unwrap_or(""),
        new_text,
    );
    a.before = current.clone();
    let Some(text) = current else {
        a.kind = Kind::Create;
        a.state = if entry.is_some() {
            State::Missing
        } else {
            State::Fresh
        };
        return gate(a, opts);
    };
    let mine = entry.is_some() || ours(&text);
    if mine {
        let unedited = entry.is_none_or(|e| e.hash.as_deref() == Some(hash(&text).as_str()));
        if !unedited && !forced {
            a.state = State::Edited;
            a.kind = Kind::Skip;
            a.reason = Some("edited".to_string());
            return a;
        }
        if text == new_text {
            a.kind = Kind::Unchanged;
            a.state = State::InStep;
        } else {
            a.kind = Kind::Update;
            a.state = if unedited {
                State::Outdated
            } else {
                State::Edited
            };
        }
        return gate(a, opts);
    }
    a.state = State::NotOurs;
    if takeover {
        a.kind = Kind::Update;
        return gate(a, opts);
    }
    a.skip("not-ours")
}

fn remove_action(
    env: &Env,
    id: &str,
    tool: Tool,
    path: &Path,
    needs: &'static str,
    opts: &Options,
) -> Action {
    let mut a = Action::new(id, tool, path, Part::File);
    a.needs = Some(needs);
    match read_text(path) {
        Ok(Some(text)) => {
            a.diff = unified(&path.to_string_lossy(), &text, "");
            a.before = Some(text);
            a.kind = Kind::Remove;
            a.state = State::NotOurs;
            gate(a, opts)
        }
        Ok(None) => {
            a.kind = Kind::Unchanged;
            a.state = if env.manifest.get(id).is_some() {
                State::InStep
            } else {
                State::Fresh
            };
            a
        }
        Err(why) => a.skip(&format!("unreadable: {why}")),
    }
}

fn tool_off(
    profile: &Profile,
    tool: Tool,
    actions: &mut Vec<Action>,
    ids: &[(&str, &Path)],
) -> bool {
    if profile.is_on(tool) {
        return false;
    }
    for (id, path) in ids {
        actions.push(Action::new(id, tool, path, Part::File).skip("tool-off"));
    }
    true
}

fn orchestrator_base(
    env: &Env,
    opts: &Options,
    id: &str,
    path: &Path,
    features: Features,
) -> Option<String> {
    let current = read_text(path).ok().flatten()?;
    let entry = env.manifest.get(id).filter(|e| !e.removed);
    if let (true, Some(backup)) = (
        opts.force.contains(id),
        entry.and_then(|e| e.backup.as_ref()),
    ) {
        if let Ok(Some(original)) = read_text(backup) {
            return Some(r::orchestrator_in_place(&original, features));
        }
    }
    Some(r::orchestrator_in_place(&current, features))
}

fn claude_plan(
    env: &Env,
    opts: &Options,
    profile: &Profile,
    found: &Detected,
    out: &mut Vec<Action>,
) {
    let claude = &env.roots.claude;
    let rules = claude.join("rules");
    let agents = claude.join("agents");
    let settings = claude.join("settings.json");
    let rule_path = rules.join(r::RULE_NAME);
    let builder = agents.join(format!("{}.md", r::BUILDER));
    let reader = agents.join(format!("{}.md", r::READER));
    let reviewer = agents.join(format!("{}.md", r::REVIEWER));
    let ids = [
        (ID_RULE, rule_path.as_path()),
        (ID_BUILDER, builder.as_path()),
        (ID_READER, reader.as_path()),
        (ID_REVIEWER, reviewer.as_path()),
        (ID_SETTINGS, settings.as_path()),
    ];
    if !found.claude {
        for (id, path) in ids {
            out.push(Action::new(id, Tool::Claude, path, Part::File).skip("tool-missing"));
        }
        return;
    }
    if tool_off(profile, Tool::Claude, out, &ids) {
        return;
    }
    let (avail, features) = (env.avail, opts.features);
    let texts = [
        (
            ID_RULE,
            &rule_path,
            Some(r::claude_rule(profile, avail, features)),
        ),
        (ID_BUILDER, &builder, r::builder_agent(profile, avail)),
        (ID_READER, &reader, r::reader_agent(profile, avail)),
        (ID_REVIEWER, &reviewer, r::reviewer_agent(profile, avail)),
    ];
    for (id, path, text) in texts {
        match text {
            Some(text) => out.push(file_action(
                env,
                opts,
                id,
                Tool::Claude,
                path,
                &text,
                None,
                false,
            )),
            None => out.push(Action::new(id, Tool::Claude, path, Part::File).skip("no-model")),
        }
    }
    claude_settings(env, profile, &settings, out);
    claude_optional(env, opts, profile, found, out);
}

fn claude_settings(env: &Env, profile: &Profile, path: &Path, out: &mut Vec<Action>) {
    let mut a = Action::new(ID_SETTINGS, Tool::Claude, path, Part::Keys);
    let loaded = match load(path) {
        Ok(l) => l,
        Err(e) => {
            out.push(a.skip(&format!("unreadable: {e}")));
            return;
        }
    };
    if loaded.state == FileState::Missing {
        out.push(a.skip("no-file"));
        return;
    }
    let prior = env
        .manifest
        .get(ID_SETTINGS)
        .map(|e| e.keys.clone())
        .unwrap_or_default();
    let text = if loaded.state == FileState::Empty {
        "{}\n".to_string()
    } else {
        loaded.text.clone()
    };
    match claude_keys(&text, profile, env.avail, &prior) {
        Err(why) => out.push(a.skip(&format!("refused: {why}"))),
        Ok(plan) => {
            a.diff = unified(&path.to_string_lossy(), &loaded.text, &plan.text);
            a.notes = plan
                .skipped
                .iter()
                .map(|s| format!("{}: {}", s.path.join("."), s.why))
                .collect();
            a.keys = plan.recs;
            a.before = Some(loaded.text.clone());
            a.after = Some(plan.text.clone());
            let seen = env.manifest.get(ID_SETTINGS).is_some();
            if plan.text == loaded.text && loaded.state != FileState::Empty {
                a.kind = Kind::Unchanged;
                a.state = State::InStep;
            } else {
                a.kind = Kind::Update;
                a.state = if seen { State::Outdated } else { State::Fresh };
            }
            out.push(a);
        }
    }
}

fn claude_optional(
    env: &Env,
    opts: &Options,
    profile: &Profile,
    found: &Detected,
    out: &mut Vec<Action>,
) {
    let claude = &env.roots.claude;
    let rules = claude.join("rules");
    let agents = claude.join("agents");
    let features = opts.features;
    let orchestrator = agents.join(ORCHESTRATOR_FILE);
    if found.orchestrator.is_some() {
        if let Some(text) = orchestrator_base(env, opts, ID_ORCHESTRATOR, &orchestrator, features) {
            out.push(file_action(
                env,
                opts,
                ID_ORCHESTRATOR,
                Tool::Claude,
                &orchestrator,
                &text,
                Some(CONSENT_ORCHESTRATOR),
                true,
            ));
        }
    } else if env.manifest.get(ID_ORCHESTRATOR).is_some() {
        if let Some(text) = orchestrator_base(env, opts, ID_ORCHESTRATOR, &orchestrator, features) {
            out.push(file_action(
                env,
                opts,
                ID_ORCHESTRATOR,
                Tool::Claude,
                &orchestrator,
                &text,
                None,
                false,
            ));
        }
    } else {
        let path = agents.join(format!("{}.md", r::ORCHESTRATOR));
        let text = r::orchestrator_agent(profile, features);
        out.push(file_action(
            env,
            opts,
            "claude.orchestrator.new",
            Tool::Claude,
            &path,
            &text,
            Some(CONSENT_ORCHESTRATOR_NEW),
            false,
        ));
    }
    if found.pstack {
        let path = rules.join(PSTACK_RULE);
        let existing = read_text(&path).ok().flatten();
        let text = pstack::render(profile, env.avail, existing.as_deref());
        out.push(file_action(
            env,
            opts,
            ID_PSTACK,
            Tool::Claude,
            &path,
            &text,
            Some(CONSENT_PSTACK),
            true,
        ));
    }
    if let Some(own) = found.own_rules.first() {
        out.push(remove_action(
            env,
            ID_OWN_RULES,
            Tool::Claude,
            own,
            CONSENT_RULES,
            opts,
        ));
        for other in found.own_rules.iter().skip(1) {
            let name = other.file_name().and_then(|n| n.to_str()).unwrap_or("rule");
            out.push(
                Action::new(
                    &format!("claude.rules.left.{name}"),
                    Tool::Claude,
                    other,
                    Part::File,
                )
                .skip("another-model-rule"),
            );
        }
    }
    for file in &found.pointers {
        let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let id = format!("{ID_POINTER}{name}");
        let Ok(Some(text)) = read_text(file) else {
            continue;
        };
        let new = text.replace(OLD_RULE_REF, r::RULE_NAME);
        out.push(file_action(
            env,
            opts,
            &id,
            Tool::Claude,
            file,
            &new,
            Some(CONSENT_RULES),
            true,
        ));
    }
}

pub fn plan(env: &Env, profile: &Profile, opts: &Options) -> Plan {
    let found = detect(env.roots);
    let mut profile = profile.clone();
    let mut report = Vec::new();
    if let Some(own) = found.own_rules.first() {
        if let Ok(Some(text)) = read_text(own) {
            let outcome = import_rules(&text, &profile);
            report = outcome.report.clone();
            if opts.allows(Some(CONSENT_RULES)) {
                let source = own.file_name().and_then(|n| n.to_str()).unwrap_or("");
                apply_to(&mut profile, &outcome, source);
            }
        }
    }
    let mut actions = Vec::new();
    claude_plan(env, opts, &profile, &found, &mut actions);
    super::plan_other::codex_plan(env, opts, &profile, &found, &mut actions);
    super::plan_other::agy_plan(env, opts, &profile, &found, &mut actions);
    Plan {
        actions,
        profile,
        report,
        detected: found,
    }
}
