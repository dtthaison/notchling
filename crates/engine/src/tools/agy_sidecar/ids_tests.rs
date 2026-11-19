use super::ids::{is_uuid, valid_model};

#[test]
fn only_a_plain_uuid_is_a_conversation_id() {
    assert!(is_uuid("457369a9-35a1-40f8-92b9-26b6398b78a1"));
    assert!(is_uuid("457369A9-35A1-40F8-92B9-26B6398B78A1"));
    for bad in [
        "",
        "457369a9-35a1-40f8-92b9-26b6398b78a",
        "457369a9-35a1-40f8-92b9-26b6398b78a12",
        "457369a9_35a1_40f8_92b9_26b6398b78a1",
        "../457369a9-35a1-40f8-92b9-26b6398b78a1",
        "457369a9-35a1-40f8-92b9-26b6398b78a1/../x",
        "457369a9-35a1-40f8-92b9-26b6398b78g1",
        "457369a9-35a1-40f8-92b9-26b6398b78a1 ",
        "agy:457369a9-35a1-40f8-92b9-26b6398b78a1",
        "-457369a9-35a1-40f8-92b9-26b6398b78a",
    ] {
        assert!(!is_uuid(bad), "{bad:?}");
    }
}

#[test]
fn a_model_is_a_tier_or_a_plain_id_never_a_flag() {
    for good in [
        "flash_lite",
        "flash",
        "pro",
        "gemini-3.8-flash-high",
        "m_1.2",
    ] {
        assert!(valid_model(good), "{good}");
    }
    let long = "x".repeat(65);
    for bad in ["", "--model=x", "-pro", "a b", "a;b", "a/b", long.as_str()] {
        assert!(!valid_model(bad), "{bad}");
    }
}
