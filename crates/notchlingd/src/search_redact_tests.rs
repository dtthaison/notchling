use super::*;

fn hidden(secret: &str) {
    let text = format!("before {secret} after");
    let masked = mask(&text);
    assert!(!masked.contains(secret), "{secret} survived: {masked}");
    assert!(masked.starts_with("before "), "{masked}");
    assert!(masked.ends_with(" after"), "{masked}");
}

#[test]
fn every_token_shape_is_masked() {
    for secret in [
        concat!("sk-ant-", "api03-Zk9wQ1xV7bN3mT5pL8cD2fG6hJ4sA0rE"),
        concat!("sk-", "proj1234567890abcdefghijklmnop"),
        concat!("gh", "p_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789"),
        concat!("gh", "o_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789"),
        concat!("github", "_pat_11ABCDEFG0abcdefghijklmnop_qrstuvwxyz"),
        concat!("xo", "xb-1234567890-abcdefghijklmnop"),
        concat!("AK", "IAIOSFODNN7EXAMPLE"),
        concat!("AI", "zaSyA-1234567890abcdefghijklmnopqrstuv"),
        concat!(
            "eyJhbGciOiJIUzI1NiJ9",
            ".eyJzdWIiOiIxMjM0NTY3ODkwIn0",
            ".dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk"
        ),
        "Zm9vYmFyYmF6cXV4MTIzNDU2Nzg5MGFiY2RlZg==",
        "0123456789abcdef0123456789abcdef",
    ] {
        hidden(secret);
    }
}

#[test]
fn a_known_prefix_stays_so_the_kind_is_readable() {
    assert_eq!(
        mask(concat!(
            "key sk-ant-",
            "api03-Zk9wQ1xV7bN3mT5pL8cD2fG6hJ4sA0rE end"
        )),
        "key sk-ant-*** end"
    );
    assert_eq!(
        mask(concat!("gh", "p_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789")),
        "ghp_***"
    );
    assert_eq!(mask("id 0123456789abcdef0123456789abcdef."), "id ***.");
}

#[test]
fn an_email_takes_the_mask_email_form_and_keeps_the_sentence() {
    assert_eq!(
        mask("write to carol.jones@private-corp.example.com."),
        "write to c***@p***.com."
    );
    assert_eq!(mask("a@b.co, x@y.org"), "a***@b***.co, x***@y***.org");
    let masked = mask("mail (bob+tag@sub.example.io) now");
    assert!(!masked.contains("bob"), "{masked}");
    assert!(masked.contains("b***@s***.io"), "{masked}");
}

#[test]
fn ordinary_text_is_left_alone() {
    for text in [
        "the quick brown fox jumps over 12345 lazy dogs",
        "sk-learn is a library",
        "see crates/notchlingd/src/search_scope_tests for it",
        "version 1.2.3 of abcdefghijklmnopqrstuvwxyzabcdefghij",
        "user@localhost and @mention and name@",
        "012345678901234567890123456789",
    ] {
        assert_eq!(mask(text), text);
    }
}

#[test]
fn a_run_needs_both_letters_and_digits_and_32_characters() {
    assert_eq!(mask(&"a".repeat(40)), "a".repeat(40));
    assert_eq!(mask(&"7".repeat(40)), "7".repeat(40));
    let thirty_one = format!("a1{}", "b".repeat(29));
    assert_eq!(mask(&thirty_one), thirty_one);
    let thirty_two = format!("a1{}", "b".repeat(30));
    assert_eq!(mask(&thirty_two), "***");
}

#[test]
fn highlights_after_a_masked_span_shift_and_a_range_over_it_is_dropped() {
    let secret = concat!("sk-ant-", "api03-Zk9wQ1xV7bN3mT5pL8cD2fG6hJ4sA0rE");
    let text = format!("zebra {secret} and zebra again, zebra");
    let find = |from: usize| {
        let at = text[from..].find("zebra").unwrap() + from;
        at..at + 5
    };
    let first = find(0);
    let second = find(first.end + secret.len());
    let third = find(second.end);
    let inside = text.find(secret).unwrap() + 2..text.find(secret).unwrap() + 6;
    let (masked, marks) = mask_marked(
        &text,
        &[first.clone(), inside, second.clone(), third.clone()],
    );
    assert_eq!(masked, "zebra sk-ant-*** and zebra again, zebra");
    assert_eq!(marks.len(), 3, "{marks:?}");
    for range in &marks {
        assert_eq!(&masked[range.clone()], "zebra", "{masked} {marks:?}");
    }
    assert_eq!(marks[0], 0..5);
}

#[test]
fn text_with_nothing_to_mask_keeps_its_marks() {
    let whole = 0..7;
    let (masked, marks) = mask_marked("nothing here", std::slice::from_ref(&whole));
    assert_eq!(masked, "nothing here");
    assert_eq!(marks, [whole]);
}

#[test]
fn marks_in_multibyte_text_stay_on_boundaries() {
    let text = "đăng nhập carol@example.com đăng nhập";
    let last = text.rfind("đăng").unwrap();
    let marks = [0..6, last..last + 6];
    let (masked, shifted) = mask_marked(text, &marks);
    assert_eq!(masked, "đăng nhập c***@e***.com đăng nhập");
    for range in &shifted {
        assert_eq!(&masked[range.clone()], "đăng");
    }
}
