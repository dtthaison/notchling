use std::sync::{Arc, Mutex};

use notchling_engine::machines::registry::{AdminAccess, Machine, MachineAbilities, MachineOs};
use notchling_engine::machines::runner::{Channel, CommandResult};
use notchling_engine::machines::step::StepId;
use notchling_engine::machines::steps_tailscale_linux::SIGN_IN;

use super::wait;
use crate::cluster::add::{AddFlow, Phase, Progress};
use crate::cluster::add_rig_tests::{sd_machine, Rig};
use crate::cluster::add_runs::{AddRuns, SignInLink, Snap};
use crate::cluster::add_sheet::{sign_in_target, view, Button, Draft};

const LINK: &str = "https://login.tailscale.com/a/abc123DEF456ghi";
const SECRET_PART: &str = "abc123DEF456ghi";

fn linux_machine(name: &str) -> Machine {
    let mut m = sd_machine(name, "agent");
    m.os = Some(MachineOs::Linux);
    m.set_admin(AdminAccess::RootSsh);
    m.abilities = Some(MachineAbilities::default());
    m
}

fn logged_out(probe: &str) -> (String, i32) {
    let said = if probe.contains("uname -m") {
        "os=Linux\narch=x86_64\nuid=1001\nuser=agent\nhome=/home/agent\nversion=Ubuntu 24.04\ndisk_kb=90000000\ndocker=none"
    } else if probe.contains("id -nG") {
        return ("1001\nos=Linux\ngroups=agent\nsudo_l=1\n".into(), 0);
    } else if probe.contains("status --json --peers=false") {
        r#"{"BackendState":"NeedsLogin","Self":{"TailscaleIPs":[]}}"#
    } else {
        return (String::new(), 1);
    };
    (said.to_string(), 0)
}

fn running_snap(link: Option<&str>) -> Snap {
    Snap {
        name: "m1".into(),
        host: "m1.test".into(),
        user: "agent".into(),
        phase: Phase::Running {
            step: 1,
            of: 3,
            title: "Tailscale connected".into(),
        },
        plan: None,
        rows: Vec::new(),
        log: vec!["waiting".into()],
        sign_in: link.and_then(|l| SignInLink::new(l.to_owned())),
    }
}

fn buttons(snap: &Snap) -> Vec<(Button, bool)> {
    let draft = Draft::default();
    view(Some(snap), &draft, 0, "me", &[])
        .actions
        .iter()
        .map(|a| (a.button, a.enabled))
        .collect()
}

#[test]
fn a_running_row_offers_open_sign_in_page_only_with_a_link() {
    assert_eq!(
        buttons(&running_snap(Some(LINK))),
        [(Button::OpenSignIn, true), (Button::Close, true)]
    );
    assert_eq!(buttons(&running_snap(None)), [(Button::Close, true)]);
    assert_eq!(
        notchling_locale::cluster::add_open_sign_in(),
        "Open Sign-In Page"
    );
}

#[test]
fn only_a_tailscale_sign_in_link_is_kept_or_opened() {
    assert!(SignInLink::new(LINK.to_owned()).is_some());
    for bad in [
        "https://evil.example/a/abc",
        "https://login.tailscale.com.evil.example/a/abc",
        "http://login.tailscale.com/a/abc",
        "file:///etc/passwd",
        "https://login.tailscale.com/a/abc def",
        "",
    ] {
        assert!(SignInLink::new(bad.to_owned()).is_none(), "{bad}");
    }
    assert_eq!(sign_in_target(&running_snap(Some(LINK))), Some(LINK));
    assert_eq!(sign_in_target(&running_snap(None)), None);
}

#[test]
fn the_link_stays_out_of_debug_output() {
    let snap = running_snap(Some(LINK));
    for text in [format!("{snap:?}"), format!("{:?}", snap.sign_in)] {
        assert!(!text.contains(SECRET_PART), "{text}");
        assert!(!text.contains("login.tailscale.com"), "{text}");
    }
}

#[test]
fn the_add_flow_shows_the_link_while_tailscale_waits_and_never_keeps_it() {
    let rig = Rig::real(vec![linux_machine("vps-sing-1")]);
    rig.answer_batches(logged_out);
    let runs = AddRuns::default();
    let seen: Arc<Mutex<Vec<Option<Snap>>>> = Arc::new(Mutex::new(Vec::new()));
    let (during, shown) = (runs.clone(), seen.clone());
    rig.runner.on_call(
        |a| a.last().is_some_and(|s| s.contains("tailscale up")),
        move |call| {
            if let Some(sink) = &call.on_output {
                sink(
                    Channel::Stderr,
                    &format!("To authenticate, visit:\n\n\t{LINK}\n"),
                );
            }
            shown.lock().unwrap().push(during.get("vps-sing-1"));
            CommandResult::ok("")
        },
    );
    let flow = AddFlow::for_machine(rig.parts(), "vps-sing-1").expect("registered");
    assert!(runs.begin(flow));
    wait(|| {
        matches!(
            runs.get("vps-sing-1").map(|s| s.phase().clone()),
            Some(Phase::Plan)
        )
    });
    let plan = runs.get("vps-sing-1").and_then(|s| s.plan).expect("a plan");
    let row = plan
        .rows
        .iter()
        .find(|r| r.id == StepId::NODE_TAILSCALE)
        .expect("Tailscale is in the plan");
    assert!(!row.howto, "{row:?}");
    assert_eq!(row.display, SIGN_IN);
    assert!(plan.blocked.is_none(), "{:?}", plan.blocked);
    let before = runs.get("vps-sing-1").and_then(|s| s.sign_in);
    assert!(before.is_none());
    runs.run("vps-sing-1", None).expect("runs");
    wait(|| {
        matches!(
            runs.get("vps-sing-1").map(|s| s.phase().clone()),
            Some(Phase::Stopped(_))
        )
    });
    let during = seen.lock().unwrap().clone();
    assert_eq!(during.len(), 1);
    let at = during[0].as_ref().expect("a snapshot");
    assert!(matches!(at.phase, Phase::Running { .. }), "{:?}", at.phase);
    assert_eq!(at.sign_in.as_ref().map(SignInLink::url), Some(LINK));
    let end = runs.get("vps-sing-1").expect("the flow");
    assert!(end.sign_in.is_none(), "the link ends with the Fix");
    for text in [
        format!("{end:?}"),
        end.log.join("\n"),
        format!("{:?}", end.phase),
        format!("{:?}", rig.registry.list),
    ] {
        assert!(!text.contains(SECRET_PART), "{text}");
        assert!(!text.contains("login.tailscale.com"), "{text}");
    }
    let _ = Progress {
        step: 0,
        of: 0,
        title: String::new(),
    };
}

#[test]
fn a_link_bumps_the_generation_so_the_view_repaints() {
    let rig = Rig::real(vec![linux_machine("vps-sing-1")]);
    rig.answer_batches(logged_out);
    let generation = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let runs = AddRuns::tied_to(generation.clone());
    let marks: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
    let (g, m) = (generation.clone(), marks.clone());
    rig.runner.on_call(
        |a| a.last().is_some_and(|s| s.contains("tailscale up")),
        move |call| {
            let before = g.load(std::sync::atomic::Ordering::SeqCst);
            if let Some(sink) = &call.on_output {
                sink(Channel::Stderr, &format!("visit {LINK}\n"));
            }
            m.lock()
                .unwrap()
                .push(g.load(std::sync::atomic::Ordering::SeqCst) - before);
            CommandResult::ok("")
        },
    );
    let flow = AddFlow::for_machine(rig.parts(), "vps-sing-1").expect("registered");
    assert!(runs.begin(flow));
    wait(|| {
        matches!(
            runs.get("vps-sing-1").map(|s| s.phase().clone()),
            Some(Phase::Plan)
        )
    });
    runs.run("vps-sing-1", None).expect("runs");
    wait(|| {
        matches!(
            runs.get("vps-sing-1").map(|s| s.phase().clone()),
            Some(Phase::Stopped(_))
        )
    });
    assert!(marks.lock().unwrap()[0] >= 1, "{:?}", marks.lock().unwrap());
}
