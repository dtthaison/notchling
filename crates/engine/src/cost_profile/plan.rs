use super::action::*;
use super::detect::{detect, ours, Detected, OLD_RULE_REF, ORCHESTRATOR_FILE, PSTACK_RULE};
use super::diff::unified;
use super::import::{apply_to, import_rules};
use super::keys::claude_keys_with;
use super::manifest::Part;
use super::model::Profile;
use super::paths::pointer_id;
use super::pstack;
use super::render::{self as r, Features};
use super::safe::{is_backup_of, read_in};
use crate::catalog::Tool;
use crate::install::jsonc::is_blank;
use std::path::Path;

pub const ID_RULE: &str = "claude.rule";
pub const ID_BUILDER: &str = "claude.agent.builder";
pub const ID_READER: &str = "claude.agent.reader";
pub const ID_REVIEWER: &str = "claude.agent.reviewer";
pub const ID_ORCHESTRATOR: &str = "claude.orchestrator";
pub const ID_SETTINGS: &str = "claude.settings";
pub const ID_PSTACK: &str = "claude.pstack";
pub const ID_OWN_RULES: &str = "claude.rules.replaced.";
pub const LEGACY_ID_OWN_RULES: &str = "claude.rules.replaced";
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
    let current = match read_in(env.roots, path) {
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
        if entry.is_some_and(|e| e.released) && !forced {
            a.state = State::Edited;
            a.kind = Kind::Skip;
            a.reason = Some("kept".to_string());
            return a;
        }
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
    match read_in(env.roots, path) {
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
    let current = read_in(env.roots, path).ok().flatten()?;
    let entry = env.manifest.get(id).filter(|e| !e.removed);
    if let (true, Some(backup)) = (
        opts.force.contains(id),
        entry
            .and_then(|e| e.backup.as_ref())
            .filter(|b| is_backup_of(path, b)),
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
    claude_settings(env, opts, profile, &settings, out);
    claude_optional(env, opts, profile, found, out);
}

fn claude_settings(
    env: &Env,
    opts: &Options,
    profile: &Profile,
    path: &Path,
    out: &mut Vec<Action>,
) {
    let mut a = Action::new(ID_SETTINGS, Tool::Claude, path, Part::Keys);
    let current = match read_in(env.roots, path) {
        Ok(Some(text)) => text,
        Ok(None) => {
            out.push(a.skip("no-file"));
            return;
        }
        Err(e) => {
            out.push(a.skip(&format!("unreadable: {e}")));
            return;
        }
    };
    let prior = env
        .manifest
        .get(ID_SETTINGS)
        .map(|e| e.keys.clone())
        .unwrap_or_default();
    let blank = is_blank(&current);
    let text = if blank {
        "{}\n".to_string()
    } else {
        current.clone()
    };
    let forced = opts.force.contains(ID_SETTINGS);
    match claude_keys_with(&text, profile, env.avail, &prior, forced) {
        Err(why) => out.push(a.skip(&format!("refused: {why}"))),
        Ok(plan) => {
            a.diff = unified(&path.to_string_lossy(), &current, &plan.text);
            a.notes = plan
                .skipped
                .iter()
                .map(|s| format!("{}: {}", s.path.join("."), s.why))
                .collect();
            a.keys = plan.recs;
            a.before = Some(current.clone());
            a.after = Some(plan.text.clone());
            let seen = env.manifest.get(ID_SETTINGS).is_some();
            if plan.text == current && !blank {
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
        let existing = read_in(env.roots, &path).ok().flatten();
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
    let rule_written = out
        .iter()
        .find(|x| x.id == ID_RULE)
        .is_some_and(|x| x.kind != Kind::Skip);
    let after_rule = |mut a: Action| {
        if !rule_written {
            a.kind = Kind::Skip;
            a.reason = Some("replacement-skipped".to_string());
        } else if a.kind.writes() || a.kind == Kind::Offer {
            a.requires = vec![ID_RULE.to_string()];
        }
        a
    };
    for own in &found.own_rules {
        let name = own.file_name().and_then(|n| n.to_str()).unwrap_or("rule");
        let id = format!("{ID_OWN_RULES}{name}");
        let action = remove_action(env, &id, Tool::Claude, own, CONSENT_RULES, opts);
        out.push(after_rule(action));
    }
    for file in &found.pointers {
        let Some(id) = pointer_id(env.roots, file) else {
            continue;
        };
        let Ok(Some(text)) = read_in(env.roots, file) else {
            continue;
        };
        let new = text.replace(OLD_RULE_REF, r::RULE_NAME);
        let action = file_action(
            env,
            opts,
            &id,
            Tool::Claude,
            file,
            &new,
            Some(CONSENT_RULES),
            true,
        );
        out.push(after_rule(action));
    }
}

fn mark_duplicates(actions: &mut [Action]) {
    let mut seen: Vec<String> = Vec::new();
    for a in actions.iter_mut() {
        if seen.contains(&a.id) {
            a.kind = Kind::Skip;
            a.reason = Some("duplicate-id".to_string());
        } else {
            seen.push(a.id.clone());
        }
    }
}

pub fn plan(env: &Env, profile: &Profile, opts: &Options) -> Plan {
    let found = detect(env.roots);
    let mut profile = profile.clone();
    let mut report = Vec::new();
    for own in &found.own_rules {
        if let Ok(Some(text)) = read_in(env.roots, own) {
            let outcome = import_rules(&text, &profile);
            report.extend(outcome.report.clone());
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
    mark_duplicates(&mut actions);
    Plan {
        actions,
        profile,
        report,
        detected: found,
    }
}
