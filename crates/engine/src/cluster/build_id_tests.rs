use super::build_id::accepted;

#[test]
fn build_id_rejects_odd_text() {
    for odd in [
        "",
        "abc",
        "abcdef",
        "ABCDEF0",
        "0a32aaca ",
        "0a32aaca\n",
        "0a32aaca-clean",
        "-dirty",
        "0a32aac;rm",
        "ghijklm",
        "0a32aaca-dirty-dirty",
        &"a".repeat(41),
    ] {
        assert_eq!(accepted(odd), None, "{odd:?}");
    }
    for fine in ["0a32aac", "0a32aaca1234", "0a32aaca-dirty", &"a".repeat(40)] {
        assert_eq!(accepted(fine), Some(fine));
    }
}
