use super::power::{background_thread, parse_power_supply, state};

fn entries(rows: &[(&str, &str)]) -> Vec<(String, String)> {
    rows.iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

#[test]
fn linux_parsing_finds_battery() {
    let on_battery = entries(&[
        ("BAT0/type", "Battery\n"),
        ("AC/type", "Mains\n"),
        ("AC/online", "0\n"),
    ]);
    assert!(parse_power_supply(&on_battery));
    let plugged = entries(&[
        ("BAT0/type", "Battery\n"),
        ("AC/type", "Mains\n"),
        ("AC/online", "1\n"),
    ]);
    assert!(!parse_power_supply(&plugged));
    let usb = entries(&[
        ("BAT0/type", "Battery"),
        ("usb/type", "USB"),
        ("usb/online", "1"),
    ]);
    assert!(!parse_power_supply(&usb));
}

#[test]
fn a_desktop_is_never_on_battery() {
    assert!(!parse_power_supply(&[]));
    assert!(!parse_power_supply(&entries(&[
        ("AC/type", "Mains"),
        ("AC/online", "0")
    ])));
}

#[test]
fn state_never_panics() {
    let _ = state();
}

#[test]
fn a_thread_can_go_to_the_background_and_back() {
    std::thread::spawn(|| {
        let _ = background_thread(true);
        let _ = background_thread(false);
    })
    .join()
    .unwrap();
}
