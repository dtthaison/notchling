use super::shape::text_only;

#[test]
fn text_only_shapes() {
    for q in [
        "https://example.com/page",
        "claude.ai/artifact/KVBHDXYvY8f1b1AZG6PbAR",
        "claude.ai/code/artifact/0b1c2d3e",
        "KVBHDXYvY8f1b1AZG6PbAR",
        "crates/engine/src/search/mod.rs",
        "~/notes",
        "~",
        "\"switch accounts\"",
        "login \"exact phrase\" error",
        "a3f9c1e",
        "commit 0123abcdef12",
        "550e8400-e29b-41d4-a716-446655440000",
        "go to it",
        "ab",
        "",
    ] {
        assert!(text_only(q), "{q}");
    }
}

#[test]
fn natural_language_is_not_text_only() {
    for q in [
        "đăng nhập lỗi",
        "how do I switch accounts",
        "keep the mac awake",
        "default",
        "dang nhap",
        "internationalizations",
        "he said \"hi",
        "a / b problem with the notch",
    ] {
        assert!(!text_only(q), "{q}");
    }
}
