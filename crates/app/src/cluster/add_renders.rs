use notchling_engine::machines::step::StepId;

use super::add::{Phase, Stop};
use super::add_parts::AddParts;
use super::add_plan::{Choice, PlanRow, PlanView};
use super::add_runs::{AddRuns, Snap};
use super::add_source::AddSource;
use super::feed::ClusterFeed;
use super::model::ClusterModel;
use super::renders::Shown;
use super::view::ClusterSheetView;
use crate::machines::data::Setup;

const HOST: &str = "m2.tail1234.ts.net";
const USER: &str = "sondoan";

pub(super) fn adds(which: Shown) -> bool {
    matches!(
        which,
        Shown::Add | Shown::AddPlan | Shown::AddRunning | Shown::NotJoined | Shown::AddStopped
    )
}

pub(super) fn feed_for(which: Shown, model: ClusterModel) -> ClusterFeed {
    if !adds(which) {
        return ClusterFeed::fixed(model);
    }
    let parts = AddParts::live(&Setup::for_render(None));
    ClusterFeed::fixed_adding(model, AddSource::over(parts), AddRuns::fixed(snaps(which)))
}

fn row(id: StepId, title: &str, why: &str, reason: &str, display: &str, admin: bool) -> PlanRow {
    PlanRow {
        id,
        title: title.into(),
        why: why.into(),
        reason: reason.into(),
        display: display.into(),
        admin,
        howto: false,
        waiting: false,
    }
}

fn plan() -> PlanView {
    let rows = vec![
        row(
            StepId::STAYS_AWAKE,
            "Stays Awake",
            "A machine that sleeps drops out of the cluster.",
            "sleep is on after 10 minutes",
            "sudo -S /usr/bin/pmset -a sleep 0 disksleep 0 autorestart 1",
            true,
        ),
        row(
            StepId::NODE_BINARY,
            "Notchling Installed",
            "The Notchling program has to be on the machine.",
            "not installed",
            "scp -O /Applications/Notchling.app/Contents/MacOS/notchling sondoan@m2.tail1234.ts.net:.local/share/notchling/Notchling.app/Contents/MacOS/notchling.new\nssh sondoan@m2.tail1234.ts.net shasum -a 256 .local/share/notchling/Notchling.app/Contents/MacOS/notchling.new\nssh sondoan@m2.tail1234.ts.net mv notchling.new notchling",
            false,
        ),
        row(
            StepId::NODE_SERVICE,
            "Notchling Service",
            "The service keeps Notchling running after a restart.",
            "no service file",
            "sudo -S install -m 644 -o root -g wheel dev.notchling.node.plist /Library/LaunchDaemons/dev.notchling.node.plist\nsudo -S launchctl bootstrap system /Library/LaunchDaemons/dev.notchling.node.plist",
            true,
        ),
        row(
            StepId::NODE_JOINED,
            "Joined the Cluster",
            "It joins this cluster without asking again, because you started it here.",
            "not in a cluster",
            "ssh sondoan@m2.tail1234.ts.net notchling cluster join --token-stdin",
            false,
        ),
    ];
    PlanView {
        rows,
        needs_password: true,
        root_confirm: false,
        can_be_master: Choice {
            on: false,
            editable: false,
            reason: Some("FileVault is off".into()),
        },
        acting: Choice {
            on: true,
            editable: true,
            reason: None,
        },
        blocked: None,
    }
}

fn snap(phase: Phase, plan: Option<PlanView>, log: &[&str]) -> Snap {
    Snap {
        name: "m2".into(),
        host: HOST.into(),
        user: USER.into(),
        phase,
        plan,
        rows: Vec::new(),
        log: log.iter().map(|l| (*l).to_owned()).collect(),
    }
}

fn snaps(which: Shown) -> Vec<Snap> {
    match which {
        Shown::AddPlan => vec![snap(Phase::Plan, Some(plan()), &[])],
        Shown::AddRunning => vec![snap(
            Phase::Running {
                step: 3,
                of: 4,
                title: "Notchling Service".into(),
            },
            None,
            &[
                "Stays Awake: done",
                "Notchling Installed: copied, hash matches",
                "launchctl bootstrap system /Library/LaunchDaemons/dev.notchling.node.plist",
            ],
        )],
        Shown::AddStopped => vec![snap(
            Phase::Stopped(
                Stop::new("Notchling Service: the admin password was refused", true).with_howto(
                    String::from("Type the password of sondoan on m2, then Check Again."),
                ),
            ),
            None,
            &["Stays Awake: done", "sudo: 1 incorrect password attempt"],
        )],
        _ => Vec::new(),
    }
}

impl ClusterSheetView {
    pub(super) fn preview_add(&mut self, which: Shown, cx: &mut gpui::Context<Self>) {
        let parts = self.feed.add_source().parts();
        let flow = match which {
            Shown::Add => None,
            Shown::AddPlan | Shown::AddRunning | Shown::AddStopped => Some("m2".to_owned()),
            _ => return,
        };
        let sheet = super::add_view::AddSheet::new(parts, flow, cx);
        if which == Shown::Add {
            let typed = format!("{USER}@{HOST}");
            sheet
                .address
                .update(cx, |editor, cx| editor.set_text(&typed, cx));
            sheet
                .name
                .update(cx, |editor, cx| editor.set_text("m2", cx));
        }
        self.add.sheet = Some(sheet);
        self.add.tall = true;
        if which == Shown::AddPlan {
            if let Some(sheet) = self.add.sheet.as_mut() {
                sheet.commands.insert(StepId::NODE_BINARY.raw().to_owned());
                sheet.password.field().push_str("xxxxxxxxxx");
            }
        }
    }
}
