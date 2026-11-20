use notchling_engine::machines::registry::MachineAbilities;

use super::checked::checked_line;

fn at(seconds: i64) -> MachineAbilities {
    MachineAbilities {
        checked_at: Some(seconds),
        ..MachineAbilities::default()
    }
}

#[test]
fn the_age_reads_in_minutes() {
    assert_eq!(
        checked_line(Some(&at(1_000)), 1_180.0).as_deref(),
        Some("Checked 3 minutes ago")
    );
}

#[test]
fn a_fresh_check_reads_just_now() {
    assert_eq!(
        checked_line(Some(&at(1_000)), 1_020.0).as_deref(),
        Some("Checked just now")
    );
}

#[test]
fn hours_and_a_clock_behind_the_check() {
    assert_eq!(
        checked_line(Some(&at(1_000)), 1_000.0 + 7_300.0).as_deref(),
        Some("Checked 2 hours ago")
    );
    assert_eq!(
        checked_line(Some(&at(1_000)), 500.0).as_deref(),
        Some("Checked just now")
    );
}

#[test]
fn no_check_time_leaves_the_line_out() {
    assert_eq!(checked_line(Some(&MachineAbilities::default()), 5.0), None);
    assert_eq!(checked_line(None, 5.0), None);
}

#[test]
fn odd_clocks_and_times_never_panic() {
    assert_eq!(checked_line(Some(&at(1_000)), f64::INFINITY), None);
    assert_eq!(checked_line(Some(&at(1_000)), f64::NAN), None);
    assert!(checked_line(Some(&at(i64::MIN)), 1.0e18).is_some());
}
