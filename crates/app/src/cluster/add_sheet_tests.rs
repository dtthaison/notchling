use notchling_engine::machines::registry::MachineOs;
use notchling_engine::machines::step::StepId;

use super::add::{Phase, Stop};
use super::add_plan::{Choice, PlanRow, PlanView};
use super::add_runs::Snap;
use super::add_sheet::{
    brief, derived_name, install, may_open, not_joined_line, parse_draft, view, AddPassword,
    Button, Draft,
};
use super::not_joined::{NotJoined, Reach, RouteLine};

fn draft(address: &str, name: &str) -> Draft {
    Draft {
        address: address.into(),
        name: name.into(),
        ..Draft::default()
    }
}

fn row(id: StepId, howto: bool) -> PlanRow {
    PlanRow {
        title: id.raw().to_owned(),
        id,
        why: "why".into(),
        reason: "missing".into(),
        display: "echo".into(),
        admin: false,
        howto,
        waiting: false,
    }
}

fn choice(on: bool) -> Choice {
    Choice {
        on,
        editable: true,
        reason: None,
    }
}

fn plan(needs_password: bool, blocked: Option<&str>) -> PlanView {
    PlanView {
        rows: vec![row(StepId::NODE_BINARY, false)],
        needs_password,
        admin_entry: None,
        root_confirm: false,
        can_be_master: choice(false),
        acting: choice(true),
        blocked: blocked.map(str::to_owned),
        after: Vec::new(),
    }
}

fn snap(phase: Phase, plan: Option<PlanView>) -> Snap {
    Snap {
        name: "m1".into(),
        host: "m1.tail1234.ts.net".into(),
        user: "sondoan".into(),
        phase,
        plan,
        rows: Vec::new(),
        log: vec!["copying".into()],
        sign_in: None,
    }
}

fn buttons(view: &super::add_sheet::AddView) -> Vec<(Button, bool)> {
    view.actions.iter().map(|a| (a.button, a.enabled)).collect()
}

#[test]
fn name_follows_the_host_until_it_is_typed() {
    assert_eq!(derived_name("agent@vps-sing-1", "me"), "vps-sing-1");
    assert_eq!(derived_name("m1.tail1234.ts.net", "me"), "m1");
    assert_eq!(derived_name("", "me"), "");
    let parsed = parse_draft(&draft("m1.tail1234.ts.net", ""), "sondoan", &[]).unwrap();
    assert_eq!(parsed.name, "m1");
    assert_eq!(parsed.target, "sondoan@m1.tail1234.ts.net");
    let typed = parse_draft(&draft("m1.tail1234.ts.net", "studio"), "sondoan", &[]).unwrap();
    assert_eq!(typed.name, "studio");
}

#[test]
fn address_errors_show_in_the_form_and_hold_check_back() {
    let none = view(None, &draft("", ""), 0, "me", &[]);
    assert!(
        none.form && none.error.is_none(),
        "an empty field is not an error"
    );
    assert_eq!(
        buttons(&none),
        [
            (Button::Cancel, true),
            (Button::FromTailscale, true),
            (Button::Add, false)
        ]
    );

    let root = view(None, &draft("root@vps", ""), 0, "me", &[]);
    assert!(root.error.as_deref().unwrap().contains("agent@vps-sing-1"));
    assert!(!root
        .actions
        .iter()
        .any(|a| a.button == Button::Add && a.enabled));

    let taken = view(None, &draft("m1", ""), 0, "me", &["m1".to_owned()]);
    assert!(taken.error.is_some(), "a name already in Machines");

    let ok = view(None, &draft("agent@vps-sing-1", ""), 0, "me", &[]);
    assert_eq!(ok.error, None);
    assert_eq!(
        ok.target.as_deref(),
        Some("User agent · Host vps-sing-1 · Port 22")
    );
    assert!(ok
        .actions
        .iter()
        .any(|a| a.button == Button::Add && a.enabled));
}

#[test]
fn a_bare_host_shows_this_macs_user() {
    let bare = view(None, &draft("m1", ""), 0, "sondoan", &[]);
    assert_eq!(
        bare.target.as_deref(),
        Some("User sondoan · Host m1 · Port 22")
    );
}

#[test]
fn checking_shows_the_target_and_only_closes() {
    let checking = view(
        Some(&snap(Phase::Checking, None)),
        &draft("", ""),
        0,
        "me",
        &[],
    );
    assert!(checking
        .busy
        .as_deref()
        .unwrap()
        .contains("sondoan@m1.tail1234.ts.net"));
    assert_eq!(buttons(&checking), [(Button::Close, true)]);
}

#[test]
fn install_waits_for_the_password_and_a_clear_plan() {
    let needs = snap(Phase::Plan, Some(plan(true, None)));
    let no_password = view(Some(&needs), &draft("", ""), 0, "me", &[]);
    assert!(!no_password.can_install());
    let typed = view(Some(&needs), &draft("", ""), 3, "me", &[]);
    assert!(typed.can_install());

    let blocked = snap(Phase::Plan, Some(plan(false, Some("Tailscale first"))));
    let held = view(Some(&blocked), &draft("", ""), 5, "me", &[]);
    assert!(!held.can_install(), "a blocked plan never runs");

    let free = snap(Phase::Plan, Some(plan(false, None)));
    let go = view(Some(&free), &draft("", ""), 0, "me", &[]);
    assert!(go.can_install());
    assert_eq!(
        buttons(&go),
        [
            (Button::Cancel, true),
            (Button::CheckAgain, true),
            (Button::Install, true)
        ]
    );
    assert!(go.plan_intro.as_deref().unwrap().contains("m1"));
}

#[test]
fn running_says_the_step_and_can_be_closed() {
    let running = snap(
        Phase::Running {
            step: 2,
            of: 5,
            title: "Notchling installed".into(),
        },
        None,
    );
    let shown = view(Some(&running), &draft("", ""), 0, "me", &[]);
    assert_eq!(shown.step.as_deref(), Some("Adding m1… (step 2 of 5)"));
    assert_eq!(shown.step_title.as_deref(), Some("Notchling installed"));
    assert_eq!(shown.log, ["copying"]);
    assert!(shown.note.is_some());
    assert_eq!(buttons(&shown), [(Button::Close, true)]);
}

#[test]
fn a_stop_offers_check_again_only_when_it_says_so() {
    let with = snap(Phase::Stopped(Stop::new("no key", true)), None);
    let again = view(Some(&with), &draft("", ""), 0, "me", &[]);
    assert_eq!(
        buttons(&again),
        [(Button::CheckAgain, true), (Button::Close, true)]
    );
    let without = snap(Phase::Stopped(Stop::new("too old", false)), None);
    let held = view(Some(&without), &draft("", ""), 0, "me", &[]);
    assert_eq!(buttons(&held), [(Button::Close, true)]);
    assert_eq!(held.stop.unwrap().reason, "too old");
}

#[test]
fn done_says_so_and_offers_done() {
    let done = view(Some(&snap(Phase::Done, None)), &draft("", ""), 0, "me", &[]);
    assert_eq!(done.done.as_deref(), Some("m1 is in the cluster."));
    assert_eq!(buttons(&done), [(Button::Done, true)]);
}

#[test]
fn the_cluster_sheet_rows_follow_the_phase() {
    assert!(brief("m1", &Phase::Address).is_none());
    let running = brief(
        "m1",
        &Phase::Running {
            step: 1,
            of: 3,
            title: String::new(),
        },
    )
    .unwrap();
    assert_eq!(running.text, "Adding m1… (step 1 of 3)");
    assert!(!running.problem && !running.dismiss);
    let stopped = brief("m1", &Phase::Stopped(Stop::new("no key", true))).unwrap();
    assert!(stopped.problem && stopped.dismiss);
    assert!(stopped.text.contains("no key"));
    let done = brief("m1", &Phase::Done).unwrap();
    assert!(done.dismiss && !done.problem);
}

#[test]
fn the_password_is_hidden_everywhere_it_is_printed() {
    let mut password = AddPassword::default();
    password.field().push_str("hunter2");
    assert_eq!(password.len(), 7);
    assert_eq!(password.dots(), "•••••••");
    let printed = format!(
        "{password:?} {:?}",
        view(None, &draft("", ""), password.len(), "me", &[])
    );
    assert!(!printed.contains("hunter2"));
    let taken = password.take().unwrap();
    assert_eq!(taken.expose(), "hunter2");
    assert!(password.is_empty() && password.take().is_none());
}

#[test]
fn not_in_the_cluster_lines_say_the_system() {
    let mac = NotJoined {
        name: "m1".into(),
        host: "m1".into(),
        user: "sondoan".into(),
        os: Some(MachineOs::MacOs),
        node_on: false,
        reach: Reach::Unchecked,
        checked_s: None,
        route: RouteLine::Hidden,
    };
    assert_eq!(not_joined_line(&mac), "sondoan@m1 · macOS");
    let linux = NotJoined {
        os: Some(MachineOs::Linux),
        ..mac.clone()
    };
    assert!(not_joined_line(&linux).ends_with("Linux"));
    let unknown = NotJoined { os: None, ..mac };
    assert_eq!(not_joined_line(&unknown), "sondoan@m1");
}

#[test]
fn a_busy_cluster_sheet_opens_no_add_sheet() {
    assert!(may_open(false, false));
    assert!(!may_open(true, false), "busy: no click opens it");
    assert!(!may_open(false, true), "one sheet at a time");
}

#[test]
fn return_does_what_the_button_does_only_when_it_is_enabled() {
    let empty = view(None, &draft("", ""), 0, "me", &[]);
    assert!(!empty.allows(Button::Add), "an empty address");
    let bad = view(None, &draft("root@vps", ""), 0, "me", &[]);
    assert!(!bad.allows(Button::Add), "an invalid address");
    let ok = view(None, &draft("agent@vps-sing-1", ""), 0, "me", &[]);
    assert!(ok.allows(Button::Add));
    assert!(ok.allows(Button::Cancel));

    let checking = snap(Phase::Checking, None);
    let busy = view(
        Some(&checking),
        &draft("agent@vps-sing-1", ""),
        0,
        "me",
        &[],
    );
    assert!(!busy.allows(Button::Add), "no Check while one runs");

    let blocked = snap(Phase::Plan, Some(plan(false, Some("Tailscale first"))));
    let held = view(Some(&blocked), &draft("", ""), 5, "me", &[]);
    assert!(!held.allows(Button::Install), "a blocked plan");
    let needs = snap(Phase::Plan, Some(plan(true, None)));
    let no_password = view(Some(&needs), &draft("", ""), 0, "me", &[]);
    assert!(!no_password.allows(Button::Install), "no password typed");
    let typed = view(Some(&needs), &draft("", ""), 4, "me", &[]);
    assert!(typed.allows(Button::Install));
    assert!(typed.allows(Button::CheckAgain));
}

#[test]
fn the_password_is_only_taken_when_the_run_starts() {
    let mut password = AddPassword::default();
    password.field().push_str("hunter2");
    let refused = install(&mut password, |typed| {
        assert_eq!(typed.unwrap().expose(), "hunter2");
        Err("The plan is blocked.".to_owned())
    });
    assert!(refused.is_err());
    assert_eq!(password.len(), 7, "a refusal keeps what was typed");
    let started = install(&mut password, |typed| {
        assert_eq!(typed.unwrap().expose(), "hunter2");
        Ok(())
    });
    assert!(started.is_ok());
    assert!(password.is_empty(), "a started run clears the field");
}

#[test]
fn the_admin_password_field_is_labelled() {
    assert_eq!(
        notchling_locale::cluster::add_password_label(),
        "Admin Password"
    );
    assert!(
        notchling_locale::cluster::add_password_placeholder("me@m1").starts_with("Admin Password")
    );
}

#[test]
fn the_add_form_walks_check_saved_and_stopped() {
    use super::add_sheet::machine_view;
    use super::machine_add::Phase as Added;

    let typed = draft("deploy@app-1.example", "");
    let form = machine_view(&Added::Form, &typed, "me", &[]);
    assert_eq!(form.title, "Add Machine");
    assert_eq!(
        buttons(&form),
        [
            (Button::Cancel, true),
            (Button::FromTailscale, true),
            (Button::Add, true)
        ]
    );
    let checking = machine_view(&Added::Checking, &typed, "me", &[]);
    assert!(!checking.form);
    assert!(checking
        .busy
        .as_deref()
        .is_some_and(|t| t.contains("deploy@app-1.example")));
    assert_eq!(buttons(&checking), [(Button::Close, true)]);

    let saved = machine_view(
        &Added::Saved {
            name: "app-1".into(),
            note: Some("a note".into()),
        },
        &typed,
        "me",
        &[],
    );
    let done = saved.done.clone().expect("a done line");
    assert!(
        done.contains("app-1") && done.contains("Join Cluster"),
        "{done}"
    );
    assert_eq!(saved.note.as_deref(), Some("a note"));
    assert_eq!(buttons(&saved), [(Button::Done, true)]);

    let stop = Stop::new("no route", true);
    let stopped = machine_view(&Added::Stopped(stop.clone()), &typed, "me", &[]);
    assert_eq!(stopped.stop, Some(stop));
    assert_eq!(
        buttons(&stopped),
        [(Button::CheckAgain, true), (Button::Close, true)]
    );
    let hard = machine_view(
        &Added::Stopped(Stop::new("taken", false)),
        &typed,
        "me",
        &[],
    );
    assert_eq!(buttons(&hard), [(Button::Close, true)]);
}

#[test]
fn a_bare_ipv6_address_and_a_bad_port_are_named_not_mangled() {
    let v6 = parse_draft(&draft("agent@fd7a:115c::1", ""), "me", &[]).unwrap();
    assert_eq!(v6.target, "agent@fd7a:115c::1");
    assert_eq!(derived_name("fd7a:115c::1", "me"), "machine-1");
    let bracketed = parse_draft(&draft("agent@[fd7a:115c::1]:2200", ""), "me", &[]).unwrap();
    assert_eq!(bracketed.addresses[0].port, Some(2200));
    let bad = parse_draft(&draft("agent@host:99999", ""), "me", &[]).unwrap_err();
    assert!(bad.to_lowercase().contains("port"), "{bad}");
    let zero = parse_draft(&draft("agent@host:0", ""), "me", &[]).unwrap_err();
    assert!(zero.to_lowercase().contains("port"), "{zero}");
    assert_eq!(derived_name("agent@host:99999", "me"), "");
}
