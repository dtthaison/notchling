use notchling_engine::machines::addresses::AddressKind;

use super::add_rig_tests::Rig;
use super::add_sheet::{derived_name, machine_view, parse_draft, seen_line, Button, Draft};
use super::machine_add::{MachineAdd, Phase};

const ME: &str = "sondoan";

fn draft(address: &str) -> Draft {
    Draft {
        address: address.into(),
        ..Draft::default()
    }
}

fn saved_json(typed: &str) -> serde_json::Value {
    let rig = Rig::new(vec![]);
    let mut add = MachineAdd::new();
    let probe = add
        .begin(&rig.parts(), &draft(typed), None)
        .unwrap_or_else(|why| panic!("{typed}: {why}"));
    let answer = probe.run(&rig.engine);
    add.answered(&rig.parts(), answer);
    assert!(matches!(add.phase(), Phase::Saved { .. }), "{typed}");
    let name = add.name().expect("a name").to_owned();
    serde_json::to_value(rig.saved(&name).expect("saved")).expect("json")
}

#[test]
fn a_pasted_command_saves_what_user_host_port_saves() {
    let typed = saved_json("deploy@app-1.example:234");
    for pasted in [
        "ssh -p 234 deploy@app-1.example",
        "ssh deploy@app-1.example -p 234",
        "ssh -o Port=234 deploy@app-1.example",
        "ssh -l deploy -p 234 app-1.example",
        "ssh://deploy@app-1.example:234",
    ] {
        assert_eq!(saved_json(pasted), typed, "{pasted}");
    }
    assert!(typed["addresses"].is_array());
}

#[test]
fn a_plain_command_saves_what_user_host_saves() {
    let typed = saved_json("deploy@app-1.example");
    assert_eq!(saved_json("ssh deploy@app-1.example"), typed);
    assert!(typed.get("addresses").is_none());
}

#[test]
fn a_command_and_its_typed_form_parse_the_same() {
    let a = parse_draft(&draft("ssh -p 234 stackops@203.0.113.7"), ME, &[]).expect("a");
    let b = parse_draft(&draft("stackops@203.0.113.7:234"), ME, &[]).expect("b");
    assert_eq!(a, b);
    assert_eq!(a.target, "stackops@203.0.113.7");
    assert_eq!(a.port, Some(234));
}

#[test]
fn the_key_is_saved_with_the_machine() {
    let rig = Rig::new(vec![]);
    let mut add = MachineAdd::new();
    let probe = add
        .begin(
            &rig.parts(),
            &draft("ssh -i ~/.ssh/id_x deploy@app-1.example"),
            None,
        )
        .expect("begins");
    let answer = probe.run(&rig.engine);
    add.answered(&rig.parts(), answer);
    let saved = rig.saved("app-1").expect("saved");
    assert_eq!(saved.ssh_key(), Some("~/.ssh/id_x"));
}

#[test]
fn a_jump_names_a_machine_in_machines() {
    let known = vec!["oracle".to_owned()];
    let ok = parse_draft(&draft("ssh -J oracle deploy@10.1.2.3"), ME, &known).expect("jump");
    assert_eq!(ok.via.as_deref(), Some("oracle"));
    assert_eq!(ok.addresses.len(), 1);
    assert_eq!(ok.addresses[0].kind, AddressKind::Jump);
    assert_eq!(ok.addresses[0].via.as_deref(), Some("oracle"));
    let why = parse_draft(&draft("ssh -J elsewhere deploy@10.1.2.3"), ME, &known).unwrap_err();
    assert!(why.contains("elsewhere"), "{why}");
}

#[test]
fn the_understood_line_names_user_host_and_port() {
    let at = |typed: &str| seen_line(&parse_draft(&draft(typed), ME, &["oracle".into()]).unwrap());
    assert_eq!(
        at("ssh -p 234 stackops@203.0.113.7"),
        "User stackops · Host 203.0.113.7 · Port 234"
    );
    assert_eq!(
        at("ssh -i ~/.ssh/id_x -p 234 stackops@203.0.113.7"),
        "User stackops · Host 203.0.113.7 · Port 234 · Key ~/.ssh/id_x"
    );
    assert_eq!(
        at("ssh -J oracle stackops@10.1.2.3"),
        "User stackops · Host 10.1.2.3 · Port 22 · Via jump oracle"
    );
    assert_eq!(
        at("box.example"),
        "User sondoan · Host box.example · Port 22"
    );
}

#[test]
fn the_sheet_shows_what_was_understood_or_why_not() {
    let shown = machine_view(
        &Phase::Form,
        &draft("ssh -p 234 stackops@203.0.113.7"),
        ME,
        &[],
    );
    assert_eq!(
        shown.target.as_deref(),
        Some("User stackops · Host 203.0.113.7 · Port 234")
    );
    assert!(shown.error.is_none());
    assert!(shown.allows(Button::Add));
    for (typed, word) in [
        ("ssh -X stackops@203.0.113.7", "-X"),
        ("ssh stackops@203.0.113.7 ls", "ls"),
        ("ssh -p 0 stackops@203.0.113.7", "65535"),
        ("ssh -p 70000 stackops@203.0.113.7", "65535"),
        ("ssh -p 22", "user@host"),
    ] {
        let shown = machine_view(&Phase::Form, &draft(typed), ME, &[]);
        assert!(shown.target.is_none(), "{typed}");
        assert!(
            shown.error.as_deref().is_some_and(|e| e.contains(word)),
            "{typed}: {:?}",
            shown.error
        );
        assert!(!shown.allows(Button::Add), "{typed}");
    }
    let empty = machine_view(&Phase::Form, &draft(""), ME, &[]);
    assert!(empty.target.is_none() && empty.error.is_none());
}

#[test]
fn the_name_comes_from_the_host_of_a_command() {
    assert_eq!(derived_name("ssh -p 234 deploy@app-1.example", ME), "app-1");
    assert_eq!(
        derived_name("ssh -p 234 stackops@203.0.113.7", ME),
        "machine-7"
    );
    assert_eq!(derived_name("ssh -p", ME), "");
}

#[test]
fn root_is_still_refused_in_a_command() {
    assert!(parse_draft(&draft("ssh root@203.0.113.7"), ME, &[]).is_err());
}
