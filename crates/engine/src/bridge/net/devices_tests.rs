use super::*;
use crate::sessions::testutil::TempDir;

/// A `devices.json` as the Swift app writes it (`JSONEncoder`, `.prettyPrinted, .sortedKeys`,
/// `.iso8601`), with the tokens below.
const SWIFT: &str = include_str!("testdata/devices.swift.json");
const IPHONE_TOKEN: &str = "Xq3v9yR2mT7pL0wK5sJ8nB4cF6hD1gA_zE-uY2iO3pQ";
const PIXEL_TOKEN: &str = "Pk7nW2qR9sT4vX6yZ1aB3cD5eF8gH0jK-mN_oP2rS4t";
const NOW: i64 = 1_800_000_000_000;

fn swift_store(dir: &TempDir) -> DeviceStore {
    let file = dir.write("bridge/devices.json", SWIFT);
    DeviceStore::open(file)
}

#[test]
fn a_phone_paired_with_the_swift_app_authenticates() {
    let dir = TempDir::new("devices-swift");
    let store = swift_store(&dir);
    let devices = store.devices();
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0].name, "iPhone của Triết");
    assert_eq!(
        devices[0].paired_at,
        parse_iso8601("2026-10-02T08:30:00Z").unwrap()
    );
    assert_eq!(devices[0].last_seen, parse_iso8601("2026-11-07T21:14:05Z"));
    assert_eq!(devices[1].name, "Pixel 9 / work");
    assert_eq!(devices[1].last_seen, None);

    let iphone = store.authenticate(IPHONE_TOKEN).unwrap();
    assert_eq!(iphone.id, "6D0F2A58-3B77-4C0E-9A41-5E8B1C7D2F90");
    let pixel = store.authenticate(PIXEL_TOKEN).unwrap();
    assert_eq!(pixel.platform, "android");
    assert!(store.authenticate(&format!("{IPHONE_TOKEN}x")).is_none());
    assert!(store.authenticate("").is_none());
}

#[test]
fn what_rust_writes_is_byte_for_byte_what_swift_wrote() {
    let devices = decode(SWIFT.as_bytes()).unwrap();
    assert_eq!(String::from_utf8(encode(&devices)).unwrap(), SWIFT);
}

#[test]
fn a_change_keeps_the_swift_format_and_the_other_records() {
    let dir = TempDir::new("devices-change");
    let store = swift_store(&dir);
    let (device, token) = store.register("Phone/Two", "ios", NOW + 123).unwrap();
    let text = std::fs::read_to_string(store.file()).unwrap();
    assert!(
        text.starts_with(&SWIFT[..SWIFT.len() - 2]),
        "old records unchanged"
    );
    assert!(text.contains("\"name\" : \"Phone\\/Two\""));
    assert!(text.contains(&format!("\"pairedAt\" : \"{}\"", iso8601(NOW))));
    assert!(!text.contains(&token), "the token is never written");
    assert!(text.contains(&sha256_hex(token.as_bytes())));

    store.revoke(&device.id).unwrap();
    assert_eq!(std::fs::read_to_string(store.file()).unwrap(), SWIFT);
}

#[test]
fn keeps_only_the_hash_and_survives_a_restart() {
    let dir = TempDir::new("devices-new");
    let file = dir.path("bridge/devices.json");
    let store = DeviceStore::open(file.clone());
    assert!(!file.exists(), "opening writes nothing");
    let (device, token) = store.register("iPhone của Triết", "ios", NOW).unwrap();
    assert_eq!(token.len(), 43);
    assert_eq!(store.authenticate(&token).unwrap().id, device.id);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(&dir.path("bridge")), 0o700);
    }
    let reloaded = DeviceStore::open(file);
    assert_eq!(reloaded.devices()[0].name, "iPhone của Triết");
    assert_eq!(
        reloaded.authenticate(&token).unwrap().id,
        device.id,
        "survives a restart"
    );
}

#[test]
fn revoke_removes_the_device_and_its_token() {
    let dir = TempDir::new("devices-revoke");
    let store = DeviceStore::open(dir.path("bridge/devices.json"));
    let (first, first_token) = store.register("a", "ios", NOW).unwrap();
    let (_, second_token) = store.register("b", "android", NOW).unwrap();
    assert!(store.revoke(&first.id).unwrap().is_some());
    assert!(store.authenticate(&first_token).is_none());
    assert!(store.authenticate(&second_token).is_some());
    assert!(
        store.revoke(&first.id).unwrap().is_none(),
        "second revoke finds nothing"
    );
    assert_eq!(
        DeviceStore::open(store.file().to_path_buf())
            .devices()
            .len(),
        1
    );
}

#[test]
fn another_writers_change_is_seen_and_kept() {
    let dir = TempDir::new("devices-two");
    let file = dir.path("bridge/devices.json");
    let ours = DeviceStore::open(file.clone());
    let theirs = DeviceStore::open(file.clone());
    let (_, a) = ours.register("a", "ios", NOW).unwrap();
    let (_, b) = theirs.register("b", "ios", NOW).unwrap();
    assert!(ours.authenticate(&b).is_some(), "the other writer's phone");
    assert!(
        theirs.authenticate(&a).is_some(),
        "not lost by the other write"
    );
    assert_eq!(read_devices(&file).unwrap().len(), 2);
}

#[test]
fn an_unreadable_file_is_never_written_over() {
    let dir = TempDir::new("devices-bad");
    let file = dir.write("bridge/devices.json", "[{\"id\": 1}]");
    let store = DeviceStore::open(file.clone());
    assert!(store.devices().is_empty());
    assert!(matches!(
        store.register("a", "ios", NOW),
        Err(StoreError::Unreadable(_))
    ));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "[{\"id\": 1}]");
    assert!(matches!(
        read_devices(&file),
        Err(StoreError::Unreadable(_))
    ));
}

#[test]
fn touch_writes_at_most_once_a_minute() {
    let dir = TempDir::new("devices-touch");
    let store = swift_store(&dir);
    let id = "9B3E7C14-2A60-4D85-8F1E-0C5D7A9B3E62";
    store.touch(id, NOW).unwrap();
    let on_disk = |s: &DeviceStore| read_devices(s.file()).unwrap()[1].last_seen;
    assert_eq!(on_disk(&store), Some(NOW));
    store.touch(id, NOW + 30_500).unwrap();
    assert_eq!(on_disk(&store), Some(NOW), "not again within the minute");
    assert_eq!(
        store.devices()[1].last_seen,
        Some(NOW + 30_500),
        "in memory"
    );
    store.touch(id, NOW + 61_000).unwrap();
    assert_eq!(on_disk(&store), Some(NOW + 61_000));
    store.touch("nobody", NOW).unwrap();
}
