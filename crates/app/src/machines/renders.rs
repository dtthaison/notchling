use notchling_engine::machines::step::{CheckResult, StepId, SubRow};
use notchling_engine::versions::{parse_probe, Method, Tool, Version};

use super::data::Setup;
use super::forms::{AddForm, JobForm};
use super::model::{MachinesModel, Sheet, Tab};
use super::versions_feed::{MacSide, MacTool, MacVersionsError};
use crate::views::ViewSpec;
use notchling_engine::machines::forms::JobDraft;

pub const MACHINES: &str = "machines";
pub const MACHINES_FIX: &str = "machines-fix";
pub const MACHINES_ADD: &str = "machines-add";
pub const MACHINES_JOBS: &str = "machines-jobs";
pub const MACHINES_JOB_SHEET: &str = "machines-job-sheet";
pub const MACHINES_VERSIONS: &str = "machines-versions";
pub const MACHINES_VERSIONS_NO_DAEMON: &str = "machines-versions-no-daemon";
pub const MACHINES_SELFUPDATE: &str = "machines-selfupdate";
pub const MACHINES_DRIFT: &str = "machines-drift";

pub const WINDOW_SIZE: (f32, f32) = (900.0, 780.0);
pub const WINDOW_MIN: (f32, f32) = (680.0, 460.0);

pub const VIEWS: [ViewSpec; 9] = [
    ViewSpec {
        name: MACHINES,
        size: WINDOW_SIZE,
        about: "the Machines window, tool off: two fixture machines, every check saying so",
    },
    ViewSpec {
        name: MACHINES_FIX,
        size: WINDOW_SIZE,
        about: "the Fix sheet over it: Stays awake's exact commands and the admin password field",
    },
    ViewSpec {
        name: MACHINES_ADD,
        size: WINDOW_SIZE,
        about: "the Add machine sheet, nothing checked yet",
    },
    ViewSpec {
        name: MACHINES_JOBS,
        size: WINDOW_SIZE,
        about: "the Jobs tab: the job cards with their Running switch and the morning sections",
    },
    ViewSpec {
        name: MACHINES_JOB_SHEET,
        size: WINDOW_SIZE,
        about: "the sd worker job sheet over the Jobs tab, editing the first machine's job",
    },
    ViewSpec {
        name: MACHINES_VERSIONS,
        size: WINDOW_SIZE,
        about: "each machine's Tool Versions rows against the Mac's, one behind, one Homebrew, one not installed",
    },
    ViewSpec {
        name: MACHINES_VERSIONS_NO_DAEMON,
        size: WINDOW_SIZE,
        about: "the same rows with no daemon: the Notchling isn't running banner and no marks",
    },
    ViewSpec {
        name: MACHINES_SELFUPDATE,
        size: WINDOW_SIZE,
        about: "the Self-Update Off steps: Claude Code off in its settings file, Codex on and unreadable places flagged",
    },
    ViewSpec {
        name: MACHINES_DRIFT,
        size: WINDOW_SIZE,
        about: "Tool Versions with drift: one worker ahead of the Mac and one behind, each row naming both versions",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Machines,
    Fix,
    Add,
    Jobs,
    JobSheet,
    Versions,
    VersionsNoDaemon,
    SelfUpdate,
    Drift,
}

pub fn shown(name: &str) -> Option<Shown> {
    Some(match name {
        MACHINES => Shown::Machines,
        MACHINES_FIX => Shown::Fix,
        MACHINES_ADD => Shown::Add,
        MACHINES_JOBS => Shown::Jobs,
        MACHINES_JOB_SHEET => Shown::JobSheet,
        MACHINES_VERSIONS => Shown::Versions,
        MACHINES_VERSIONS_NO_DAEMON => Shown::VersionsNoDaemon,
        MACHINES_SELFUPDATE => Shown::SelfUpdate,
        MACHINES_DRIFT => Shown::Drift,
        _ => return None,
    })
}

pub fn model(which: Shown) -> MachinesModel {
    let mut model = MachinesModel::new(Setup::for_render(None).parts());
    model.engine().refresh(None, true);
    model.ask_mac_versions();
    match which {
        Shown::Machines => {}
        Shown::Fix => model.request_set_up(&StepId::STAYS_AWAKE, "m1"),
        Shown::Add => {
            model.open_sheet(Sheet::Add(AddForm::new()));
        }
        Shown::Jobs => model.tab = Tab::Jobs,
        Shown::Versions => seed_versions(&model),
        Shown::VersionsNoDaemon => {
            seed_versions(&model);
            model.set_mac_versions(Err(MacVersionsError::NotRunning));
        }
        Shown::SelfUpdate => {
            seed_versions(&model);
            seed_selfupdate(&model);
        }
        Shown::Drift => {
            seed_versions(&model);
            model.set_mac_versions(Ok(drift_mac()));
        }
        Shown::JobSheet => {
            model.tab = Tab::Jobs;
            if let Some(m) = model.machine("m1") {
                model.open_sheet(Sheet::Job(JobForm::new(JobDraft::of(&m))));
            }
        }
    }
    model
}

fn seed_versions(model: &MachinesModel) {
    let probes = [
        (
            "m1",
            "claude.path=/home/w/.local/bin/claude\nclaude.link=/home/w/.local/share/claude/versions/2.1.323\nclaude.version=2.1.323 (Claude Code)\ncodex.path=/opt/homebrew/bin/codex\ncodex.link=../Cellar/codex/0.166.0/bin/codex\ncodex.version=codex-cli 0.166.0\nagy.path=\n",
        ),
        (
            "vps-sing-1",
            "claude.path=/home/w/.local/bin/claude\nclaude.link=/home/w/.local/share/claude/versions/2.1.324\nclaude.version=2.1.324 (Claude Code)\ncodex.path=\nagy.path=\n",
        ),
    ];
    for (name, text) in probes {
        let mut result = CheckResult::pass("tools found");
        result.detected.tools = Some(parse_probe(text, 1));
        model.engine().show(name, StepId::TOOL_VERSIONS, result);
    }
}

fn drift_mac() -> MacSide {
    let tool = |version: &str, method: Method| MacTool {
        installed: Version::parse(version),
        method,
        reference: Version::parse(version),
        usable: true,
        installed_failed: false,
    };
    MacSide {
        tools: [
            (Tool::Claude, tool("2.1.320", Method::Native)),
            (Tool::Codex, tool("0.165.0", Method::Brew)),
        ]
        .into(),
    }
}

fn seed_selfupdate(model: &MachinesModel) {
    let claude = CheckResult::pass("Self-update is off for Claude Code").rows(vec![
        SubRow::new("~/.claude/settings.json", true, "Self-update is off"),
        SubRow::new(
            "CLAUDE_CODE_PACKAGE_MANAGER_AUTO_UPDATE",
            true,
            "off in the login shell",
        ),
    ]);
    let codex =
        CheckResult::fail("Can't read ~/.codex/config.toml: too_big").rows(vec![SubRow::new(
            "Can't read ~/.codex/config.toml",
            false,
            "too_big",
        )]);
    model
        .engine()
        .show("m1", StepId::self_update(Tool::Claude), claude);
    model
        .engine()
        .show("m1", StepId::self_update(Tool::Codex), codex);
    let on =
        CheckResult::fail("Self-update is on in ~/.claude/settings.json").rows(vec![SubRow::new(
            "~/.claude/settings.json",
            false,
            "Will add: \"env\": { \"DISABLE_AUTOUPDATER\": \"1\" }",
        )]);
    model
        .engine()
        .show("vps-sing-1", StepId::self_update(Tool::Claude), on);
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::machines::engine_plan::{AdminNeed, PlanKind};

    #[test]
    fn every_view_has_its_data() {
        for spec in VIEWS {
            assert!(shown(spec.name).is_some(), "{}", spec.name);
        }
        assert_eq!(shown("chat"), None);
        let plain = model(Shown::Machines);
        assert_eq!(plain.names(), ["m1", "vps-sing-1"]);
        let results = plain.engine().results("m1");
        assert!(results[&StepId::REACHABLE]
            .state
            .text()
            .contains("Machines is off"));
        assert!(plain.parts.off.is_some());

        let fix = model(Shown::Fix);
        let Some(Sheet::Fix(prompt)) = &fix.sheet else {
            panic!("no Fix sheet");
        };
        assert_eq!(prompt.plan.kind, PlanKind::Fix(StepId::STAYS_AWAKE));
        assert_eq!(prompt.plan.admin, AdminNeed::Password);
        assert!(prompt.text().contains("pmset"), "{}", prompt.text());

        assert!(matches!(model(Shown::Add).sheet, Some(Sheet::Add(_))));
        assert_eq!(model(Shown::Jobs).tab, Tab::Jobs);
        let Some(Sheet::Job(form)) = model(Shown::JobSheet).sheet else {
            panic!("no job sheet");
        };
        assert_eq!(form.draft.repos[0].github, "me/app");
        assert!(!form.draft.is_new);
    }
}
