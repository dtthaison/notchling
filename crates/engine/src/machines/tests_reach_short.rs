use super::addresses::{AddressKind, MachineAddress};
use super::tests_reach::{login, ts, with, Rig, PERSON};

const FULL: &str = "oracle-sg.tail6e77ec.ts.net";
const SHORT: &str = "oracle-sg";

#[test]
fn the_short_name_is_tried_first_and_wins_when_it_answers() {
    let rig = Rig::new(&[], &[]);
    let m = with("oracle", vec![ts(FULL, PERSON)]);
    let pick = rig.router.pick(&m, std::slice::from_ref(&m));
    let choice = pick.choice().unwrap();
    assert_eq!(choice.route.host, SHORT);
    assert_eq!(choice.address.host, FULL);
    let probes = rig.probes();
    assert_eq!(probes.len(), 1);
    assert_eq!(login(&probes[0]), format!("agent@{SHORT}"));
}

#[test]
fn a_short_name_that_does_not_connect_falls_back_to_the_full_name() {
    let rig = Rig::new(&[SHORT], &[]);
    let m = with("oracle", vec![ts(FULL, PERSON)]);
    let pick = rig.router.pick(&m, std::slice::from_ref(&m));
    let choice = pick.choice().unwrap();
    assert_eq!(choice.route.host, FULL);
    assert_eq!(choice.index, 0);
    let probes = rig.probes();
    assert_eq!(probes.len(), 2);
    assert_eq!(login(&probes[0]), format!("agent@{SHORT}"));
    assert_eq!(login(&probes[1]), format!("agent@{FULL}"));
}

#[test]
fn a_short_name_that_refuses_still_gives_way_to_a_full_name_that_answers() {
    let rig = Rig::new(&[], &[SHORT]);
    let m = with("oracle", vec![ts(FULL, PERSON)]);
    let pick = rig.router.pick(&m, std::slice::from_ref(&m));
    assert_eq!(pick.choice().unwrap().route.host, FULL);
}

#[test]
fn both_names_down_moves_on_to_the_next_address() {
    let rig = Rig::new(&[SHORT, FULL], &[]);
    let ip = MachineAddress::new(AddressKind::TailscaleIp, "100.64.0.9").on_tailnet(PERSON);
    let m = with("oracle", vec![ts(FULL, PERSON), ip]);
    let pick = rig.router.pick(&m, std::slice::from_ref(&m));
    let choice = pick.choice().unwrap();
    assert_eq!(choice.index, 1);
    assert_eq!(choice.route.host, "100.64.0.9");
    assert_eq!(rig.probes().len(), 3);
}

#[test]
fn the_stored_address_keeps_the_full_name() {
    let rig = Rig::new(&[], &[]);
    let m = with("oracle", vec![ts(FULL, PERSON)]);
    rig.router.pick(&m, std::slice::from_ref(&m));
    assert_eq!(m.address_list()[0].host, FULL);
    assert_eq!(m.host, FULL);
}
