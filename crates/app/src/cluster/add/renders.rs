use notchling_engine::machines::node_updates::{UpdateRow, UpdateState};
use notchling_engine::machines::step::{CheckResult, StepId};

use notchling_engine::machines::addresses::{AddressKind, MachineAddress, Route};
use notchling_engine::machines::reach::{Choice as Reached, Pick};
use notchling_engine::machines::tailscale::{parse_networks, parse_status};

use gpui::AppContext;

use crate::cluster::add::{Phase, Stop};
use crate::cluster::add_parts::AddParts;
use crate::cluster::add_plan::{Choice, PlanRow, PlanView};
use crate::cluster::add_runs::{AddRuns, Snap};
use crate::cluster::add_source::{AddSource, Offer};
use crate::cluster::addresses_form::RowMeta;
use crate::cluster::feed::ClusterFeed;
use crate::cluster::from_tailscale::{FromTailscale, TestState};
use crate::cluster::model::ClusterModel;
use crate::cluster::renders::{Shown, RENDER_NOW};
use crate::cluster::view::ClusterSheetView;
use crate::machines::data::Setup;
use notchling_engine::machines::ssh_key::{KeyIssue, KeyStatus};
use notchling_engine::machines::ssh_test::SshTest;

const HOST: &str = "m2.tail1234.ts.net";
const USER: &str = "sondoan";

pub(in crate::cluster) fn adds(which: Shown) -> bool {
    matches!(
        which,
        Shown::Add
            | Shown::AddPlan
            | Shown::AddRunning
            | Shown::NotJoined
            | Shown::AddStopped
            | Shown::Update
            | Shown::Machines
            | Shown::MachinesAdd
            | Shown::MachinesAddresses
            | Shown::MachinesTailscale
            | Shown::MachinesTest
            | Shown::MachinesEdit
            | Shown::MachinesOrganization
            | Shown::MachinesNetwork
    )
}

fn tailscale_pick() -> Pick {
    let address = MachineAddress::new(AddressKind::TailscaleName, "app-1.tail1234.ts.net")
        .on_tailnet("person@example.com");
    Pick::Reached(Reached {
        index: 0,
        route: Route::direct(&address.host, "deploy"),
        address,
    })
}

const TAILSCALE_STATUS: &str = r#"{
  "BackendState": "Running",
  "Self": {"HostName": "studio-mac", "DNSName": "studio-mac.tail1234.ts.net.", "OS": "macOS", "UserID": 1001, "TailscaleIPs": ["100.64.0.10"], "Online": true},
  "CurrentTailnet": {"Name": "person@example.com", "MagicDNSSuffix": "tail1234.ts.net", "MagicDNSEnabled": true},
  "Peer": {
    "nodekey:aa": {"ID": "n1", "HostName": "app-1", "DNSName": "app-1.tail1234.ts.net.", "OS": "macOS", "UserID": 1001, "TailscaleIPs": ["100.64.0.11"], "Online": true},
    "nodekey:bb": {"ID": "n2", "HostName": "oracle-sg", "DNSName": "oracle-sg.tail1234.ts.net.", "OS": "linux", "UserID": 1003, "TailscaleIPs": ["100.64.0.12"], "Online": false, "Tags": ["tag:server"]},
    "nodekey:cc": {"ID": "n3", "HostName": "mini-2", "DNSName": "mini-2.tail1234.ts.net.", "OS": "macOS", "UserID": 1001, "TailscaleIPs": ["100.64.0.13"], "Online": true},
    "nodekey:dd": {"ID": "n4", "HostName": "build-box", "DNSName": "build-box.tail1234.ts.net.", "OS": "linux", "UserID": 1002, "TailscaleIPs": ["100.64.0.14"], "Online": true},
    "nodekey:e0": {"ID": "n5", "HostName": "edge-3", "DNSName": "edge-3.tail1234.ts.net.", "OS": "linux", "UserID": 1001, "TailscaleIPs": ["100.64.0.15"], "Online": true},
    "nodekey:e1": {"ID": "n6", "HostName": "nas-4", "DNSName": "nas-4.tail1234.ts.net.", "OS": "linux", "UserID": 1001, "TailscaleIPs": ["100.64.0.16"], "Online": true},
    "nodekey:e2": {"ID": "n7", "HostName": "lab-5", "DNSName": "lab-5.tail1234.ts.net.", "OS": "macOS", "UserID": 1001, "TailscaleIPs": ["100.64.0.17"], "Online": true},
    "nodekey:e3": {"ID": "n8", "HostName": "pi-6", "DNSName": "pi-6.tail1234.ts.net.", "OS": "linux", "UserID": 1001, "TailscaleIPs": ["100.64.0.18"], "Online": true},
    "nodekey:e4": {"ID": "n9", "HostName": "dev-7", "DNSName": "dev-7.tail1234.ts.net.", "OS": "macOS", "UserID": 1001, "TailscaleIPs": ["100.64.0.19"], "Online": true}
  },
  "User": {
    "1001": {"ID": 1001, "LoginName": "person@example.com", "DisplayName": "Person"},
    "1002": {"ID": 1002, "LoginName": "friend@example.com", "DisplayName": "Friend"},
    "1003": {"ID": 1003, "LoginName": "tagged-devices", "DisplayName": "Tagged"}
  }
}"#;

const TAILSCALE_LIST: &str = "ID    Tailnet              Account\na1b2  person@example.com   person@example.com*\nc3d4  work.example         work@example.com\n";

fn preview_tests(model: &mut FromTailscale) {
    let usable = KeyStatus {
        issue: None,
        public: false,
    };
    for host in [
        "edge-3.tail1234.ts.net",
        "nas-4.tail1234.ts.net",
        "pi-6.tail1234.ts.net",
    ] {
        model.toggle(host);
    }
    model.set_key(
        "build-box.tail1234.ts.net",
        Some("~/.ssh/id_ci"),
        Some(usable),
    );
    model.set_key(
        "edge-3.tail1234.ts.net",
        Some("~/.ssh/id_edge.pub"),
        Some(KeyStatus {
            issue: None,
            public: true,
        }),
    );
    model.set_key(
        "nas-4.tail1234.ts.net",
        Some("~/keys/nas"),
        Some(KeyStatus {
            issue: Some(KeyIssue::Missing),
            public: false,
        }),
    );
    let done = |model: &mut FromTailscale, host: &str, result: SshTest| {
        model.tests.insert(host.to_owned(), TestState::Done(result));
    };
    done(
        model,
        "mini-2.tail1234.ts.net",
        SshTest::Connected {
            user: "sondoan".into(),
        },
    );
    done(model, "build-box.tail1234.ts.net", SshTest::Denied);
    done(
        model,
        "edge-3.tail1234.ts.net",
        SshTest::NewHost {
            user: "sondoan".into(),
        },
    );
    model.request_test("nas-4.tail1234.ts.net");
    model.request_test("pi-6.tail1234.ts.net");
}

fn from_tailscale(existing: Vec<notchling_engine::machines::registry::Machine>) -> FromTailscale {
    let mut model = FromTailscale::new(USER, existing);
    model.loaded(
        parse_status(TAILSCALE_STATUS),
        parse_networks(TAILSCALE_LIST),
    );
    model
}

fn offer() -> Offer {
    let busy = "2 shell job(s) running; the update restarts the daemon";
    let rows = [
        ("m1", UpdateState::Ready),
        (
            "vps-sing-1",
            UpdateState::NeedsBuild("Build the Linux nodes first: tools/build-node.sh".into()),
        ),
        ("m3", UpdateState::Busy(busy.into())),
    ];
    let rows = rows
        .into_iter()
        .map(|(machine, state)| UpdateRow {
            machine: machine.into(),
            state,
        })
        .collect();
    (Some("a1b2c3d".into()), rows)
}

pub(in crate::cluster) fn feed_for(which: Shown, model: ClusterModel) -> ClusterFeed {
    if !adds(which) {
        return ClusterFeed::fixed(model);
    }
    if which == Shown::Update {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/machines-node");
        let parts = AddParts::live(&Setup::for_render(Some(&dir)));
        let source = AddSource::over(parts).with_offer(offer());
        return ClusterFeed::fixed_adding(model, source, AddRuns::fixed(Vec::new()));
    }
    if matches!(
        which,
        Shown::Machines
            | Shown::MachinesAdd
            | Shown::MachinesAddresses
            | Shown::MachinesTailscale
            | Shown::MachinesTest
            | Shown::MachinesEdit
            | Shown::MachinesOrganization
            | Shown::MachinesNetwork
    ) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/machines-sheet");
        let parts = AddParts::live(&Setup::for_render(Some(&dir)));
        parts.engine.show(
            "app-1",
            StepId::REACHABLE,
            CheckResult::pass("answers over SSH"),
        );
        parts.engine.show(
            "build-2",
            StepId::REACHABLE,
            CheckResult::fail("Connection timed out"),
        );
        let now_s = RENDER_NOW / 1000;
        let mut source = AddSource::over(parts)
            .with_checked("app-1", now_s - 120)
            .with_checked("build-2", now_s - 3 * 3600);
        if which == Shown::MachinesNetwork {
            source = source.with_pick("app-1", tailscale_pick()).with_pick(
                "build-2",
                Pick::WrongNetwork {
                    wanted: "work.example".into(),
                    active: "person@example.com".into(),
                },
            );
        }
        return ClusterFeed::fixed_adding(model, source, AddRuns::fixed(Vec::new()));
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

fn howto(id: StepId, title: &str, why: &str, reason: &str, display: &str) -> PlanRow {
    PlanRow {
        howto: true,
        ..row(id, title, why, reason, display, false)
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
        howto(
            StepId::NODE_GIT,
            "Git",
            "Jobs that work on a repository need git.",
            "Command Line Tools aren't installed",
            "On m2, at its screen, run xcode-select --install and accept the Command Line Tools. Then check again.",
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
        admin_entry: None,
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
        after: vec![howto(
            StepId::NODE_CODEX_LOGIN,
            "Codex Signed In",
            "Jobs that use Codex need it signed in on the machine.",
            "not signed in",
            "In your own Terminal on m2, run codex login and sign in.",
        )],
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
        sign_in: None,
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
    pub(in crate::cluster) fn preview_add(&mut self, which: Shown, cx: &mut gpui::Context<Self>) {
        if which == Shown::Update {
            self.preview_update(cx);
            return;
        }
        let parts = self.feed.add_source().parts();
        let flow = match which {
            Shown::Add
            | Shown::MachinesAdd
            | Shown::MachinesAddresses
            | Shown::MachinesTailscale
            | Shown::MachinesTest
            | Shown::MachinesEdit
            | Shown::MachinesOrganization => None,
            Shown::AddPlan | Shown::AddRunning | Shown::AddStopped => Some("m2".to_owned()),
            _ => return,
        };
        if which == Shown::MachinesEdit {
            self.preview_edit(cx);
            return;
        }
        if matches!(
            which,
            Shown::MachinesTailscale | Shown::MachinesTest | Shown::MachinesOrganization
        ) {
            let mut existing = parts.as_ref().map(|p| p.registry.all()).unwrap_or_default();
            existing.push(notchling_engine::machines::registry::Machine::new(
                "oracle",
                "oracle-sg.tail1234.ts.net",
                USER,
            ));
            if which == Shown::MachinesTest {
                for host in [
                    "app-1.tail1234.ts.net",
                    "dev-7.tail1234.ts.net",
                    "lab-5.tail1234.ts.net",
                ] {
                    existing.push(notchling_engine::machines::registry::Machine::new(
                        host, host, USER,
                    ));
                }
            }
            let mut model = from_tailscale(existing);
            model.toggle("mini-2.tail1234.ts.net");
            model.toggle("build-box.tail1234.ts.net");
            model.set_user("build-box.tail1234.ts.net", "ci");
            if which == Shown::MachinesOrganization {
                model.set_organizations(Ok(serde_json::json!({
                    "organizations": [
                        {"id": "personal", "name": "Personal", "personal": true},
                        {"id": "org-acme", "name": "Acme", "personal": false}
                    ],
                    "colors": ["grey"]
                })));
                model.next_organization();
            }
            if which == Shown::MachinesTest {
                preview_tests(&mut model);
            }
            self.start_tailscale(model, None, cx);
            self.tailscale_preview_users(cx);
            return;
        }
        let sheet = crate::cluster::add_view::AddSheet::new(parts, flow, cx);
        if which == Shown::MachinesAddresses {
            sheet.address.update(cx, |editor, cx| {
                editor.set_text("deploy@app-3.tail1234.ts.net", cx)
            });
            sheet
                .name
                .update(cx, |editor, cx| editor.set_text("app-3", cx));
        }
        if which == Shown::MachinesAdd {
            sheet
                .address
                .update(cx, |editor, cx| editor.set_text("deploy@app-3.example", cx));
            sheet
                .name
                .update(cx, |editor, cx| editor.set_text("app-3", cx));
        }
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
        if which == Shown::MachinesAddresses {
            self.preview_addresses(cx);
        }
        self.add.tall = true;
        if which == Shown::AddPlan {
            if let Some(sheet) = self.add.sheet.as_mut() {
                sheet.password.field().push_str("xxxxxxxxxx");
            }
        }
    }

    fn preview_edit(&mut self, cx: &mut gpui::Context<Self>) {
        use crate::cluster::address_rows_view::Rows;
        use crate::cluster::edit_key::EditTest;
        use notchling_engine::machines::ssh_test::Why;
        self.open_edit_addresses("app-1", cx);
        self.tailnets = vec!["person@example.com".into(), "work.example".into()];
        let more = [
            ("192.168.1.20", AddressKind::Lan, None),
            ("10.0.0.7:2222", AddressKind::Public, None),
        ];
        let mut made = Vec::new();
        for (text, kind, via) in more {
            let meta = RowMeta {
                kind: Some(kind),
                tailnet: None,
                via: via.map(str::to_owned),
            };
            made.push(self.row_new(Rows::Edit, text, meta, cx));
        }
        if let Some(sheet) = self.edit.as_mut() {
            sheet.rows.extend(made);
            sheet
                .key
                .update(cx, |editor, cx| editor.set_text("~/.ssh/id_deploy", cx));
            sheet.test = EditTest::Done(
                1,
                vec![
                    (
                        "app-1.example".into(),
                        SshTest::WrongNetwork {
                            wanted: "work.example".into(),
                            active: "person@example.com".into(),
                        },
                    ),
                    (
                        "192.168.1.20".into(),
                        SshTest::Connected {
                            user: "deploy".into(),
                        },
                    ),
                    ("10.0.0.7:2222".into(), SshTest::Unreachable(Why::TimedOut)),
                ],
            );
        }
    }

    fn preview_addresses(&mut self, cx: &mut gpui::Context<Self>) {
        use crate::cluster::address_rows_view::Rows;
        self.tailnets = vec!["person@example.com".into(), "work.example".into()];
        self.via_names = vec!["app-1".into(), "build-2".into()];
        let rows = [
            ("192.168.1.20", AddressKind::Lan, None),
            ("deploy@app-3.example.org:2222", AddressKind::Public, None),
            ("10.0.0.7", AddressKind::Jump, Some("app-1")),
        ];
        let mut made = Vec::new();
        for (text, kind, via) in rows {
            let meta = RowMeta {
                kind: Some(kind),
                tailnet: None,
                via: via.map(str::to_owned),
            };
            made.push(self.row_new(Rows::Add, text, meta, cx));
        }
        if let Some(sheet) = self.add.sheet.as_mut() {
            if let Some(first) = sheet.rows.first_mut() {
                first.meta = RowMeta {
                    kind: Some(AddressKind::TailscaleName),
                    tailnet: Some("person@example.com".into()),
                    via: None,
                };
            }
            sheet.rows.extend(made);
        }
    }

    fn tailscale_preview_users(&mut self, cx: &mut gpui::Context<Self>) {
        let chosen: Vec<(String, String)> = self
            .tailscale
            .model
            .as_ref()
            .map(|m| {
                m.chosen
                    .iter()
                    .map(|(h, u)| (h.clone(), u.clone()))
                    .collect()
            })
            .unwrap_or_default();
        for (host, user) in chosen {
            let editor = cx.new(|cx| notchling_kit::ui::TextEditor::new("", 1, cx).single_line());
            editor.update(cx, |editor, cx| editor.set_text(&user, cx));
            self.tailscale.users.insert(host, editor);
        }
    }
}
