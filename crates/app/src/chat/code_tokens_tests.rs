use super::code_tokens::{highlight, lang_of, segments, Class, Lang, Span};
use std::path::Path;

fn found(lang: Lang, text: &str) -> Vec<(String, Class)> {
    highlight(lang, text)
        .into_iter()
        .map(|s| (text[s.start..s.end].to_owned(), s.class))
        .collect()
}

#[test]
fn a_file_name_picks_its_language() {
    let of = |name: &str| lang_of(Path::new(name));
    assert_eq!(of("a/app.rb"), Some(Lang::Ruby));
    assert_eq!(of("a/Gemfile"), Some(Lang::Ruby));
    assert_eq!(of("main.RS"), Some(Lang::Rust));
    assert_eq!(of("x.py"), Some(Lang::Python));
    assert_eq!(of("x.ts"), Some(Lang::Script));
    assert_eq!(of("x.tsx"), Some(Lang::Script));
    assert_eq!(of("x.go"), Some(Lang::Go));
    assert_eq!(of("x.swift"), Some(Lang::Swift));
    assert_eq!(of("x.sh"), Some(Lang::Shell));
    assert_eq!(of("x.zsh"), Some(Lang::Shell));
    assert_eq!(of("x.yml"), Some(Lang::Yaml));
    assert_eq!(of("x.yaml"), Some(Lang::Yaml));
    assert_eq!(of("Cargo.toml"), Some(Lang::Toml));
    assert_eq!(of("x.json"), Some(Lang::Json));
    assert_eq!(of("x.java"), Some(Lang::CLike));
    assert_eq!(of("x.css"), Some(Lang::Css));
    assert_eq!(of("x.sql"), Some(Lang::Sql));
    assert_eq!(of("notes.txt"), None);
    assert_eq!(of("README"), None);
}

#[test]
fn rust_has_keywords_strings_comments_and_numbers() {
    let got = found(Lang::Rust, "let s = \"hi\"; // note\nlet n = 42;\n");
    assert_eq!(
        got,
        vec![
            ("let".into(), Class::Keyword),
            ("\"hi\"".into(), Class::Str),
            ("// note".into(), Class::Comment),
            ("let".into(), Class::Keyword),
            ("42".into(), Class::Number),
        ]
    );
}

#[test]
fn a_keyword_inside_a_word_or_a_string_is_not_one() {
    assert_eq!(
        found(Lang::Rust, "letter = 1"),
        vec![("1".into(), Class::Number)]
    );
    assert_eq!(
        found(Lang::Rust, "\"let if\""),
        vec![("\"let if\"".into(), Class::Str)]
    );
}

#[test]
fn block_comments_and_triple_quotes_run_over_lines() {
    let got = found(Lang::CLike, "a /* x\ny */ b");
    assert_eq!(got, vec![("/* x\ny */".into(), Class::Comment)]);
    let got = found(Lang::Python, "x = \"\"\"a\nb\"\"\"\ny = 1");
    assert_eq!(
        got,
        vec![
            ("\"\"\"a\nb\"\"\"".into(), Class::Str),
            ("1".into(), Class::Number)
        ]
    );
}

#[test]
fn an_escaped_quote_does_not_end_a_string() {
    let got = found(Lang::Script, r#"const a = "say \"hi\""; b"#);
    assert_eq!(
        got,
        vec![
            ("const".into(), Class::Keyword),
            (r#""say \"hi\"""#.into(), Class::Str)
        ]
    );
}

#[test]
fn ruby_and_shell_comments_start_at_a_hash() {
    let got = found(Lang::Ruby, "def go # run\n  :ok\nend\n");
    assert_eq!(
        got,
        vec![
            ("def".into(), Class::Keyword),
            ("# run".into(), Class::Comment),
            ("end".into(), Class::Keyword)
        ]
    );
    let got = found(Lang::Shell, "echo $# # count\n");
    assert_eq!(got, vec![("# count".into(), Class::Comment)]);
}

#[test]
fn json_keys_differ_from_string_values() {
    let got = found(Lang::Json, "{\"a\": \"b\", \"n\": 1.5, \"t\": true}");
    assert_eq!(
        got,
        vec![
            ("\"a\"".into(), Class::Key),
            ("\"b\"".into(), Class::Str),
            ("\"n\"".into(), Class::Key),
            ("1.5".into(), Class::Number),
            ("\"t\"".into(), Class::Key),
            ("true".into(), Class::Keyword),
        ]
    );
}

#[test]
fn yaml_and_toml_keys_and_comments() {
    let got = found(Lang::Yaml, "name: web # the app\nports:\n  - 80\n");
    assert_eq!(
        got,
        vec![
            ("name".into(), Class::Key),
            ("# the app".into(), Class::Comment),
            ("ports".into(), Class::Key),
            ("80".into(), Class::Number),
        ]
    );
    let got = found(Lang::Toml, "[package]\nname = \"x\"\n");
    assert_eq!(
        got,
        vec![
            ("[package]".into(), Class::Key),
            ("name".into(), Class::Key),
            ("\"x\"".into(), Class::Str),
        ]
    );
}

#[test]
fn multibyte_text_never_splits_a_character() {
    let text = "let é = \"héllo\"; // ñ\n";
    for span in highlight(Lang::Rust, text) {
        assert!(text.is_char_boundary(span.start) && text.is_char_boundary(span.end));
    }
}

#[test]
fn segments_cover_a_range_with_gaps_and_clip_spans() {
    let spans = [
        Span {
            start: 2,
            end: 5,
            class: Class::Keyword,
        },
        Span {
            start: 8,
            end: 14,
            class: Class::Comment,
        },
    ];
    assert_eq!(
        segments(&spans, 0..10),
        vec![
            (2, None),
            (3, Some(Class::Keyword)),
            (3, None),
            (2, Some(Class::Comment))
        ]
    );
    assert_eq!(segments(&spans, 10..12), vec![(2, Some(Class::Comment))]);
    assert_eq!(segments(&[], 0..4), vec![(4, None)]);
}
