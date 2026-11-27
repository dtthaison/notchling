use super::fold_table::{DROPPED, TABLE};
use super::*;
use rusqlite::Connection;
use std::collections::BTreeMap;

fn sqlite_folds() -> BTreeMap<char, Option<char>> {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE VIRTUAL TABLE t USING fts5(x, tokenize='unicode61 remove_diacritics 2');
         CREATE VIRTUAL TABLE v USING fts5vocab(t, 'row');",
    )
    .unwrap();
    let chars: Vec<char> = (0x80u32..0x2500).filter_map(char::from_u32).collect();
    let tx = db.unchecked_transaction().unwrap();
    for (i, c) in chars.iter().enumerate() {
        tx.execute("INSERT INTO t (x) VALUES (?1)", [format!("k{i}k{c}k")])
            .unwrap();
    }
    tx.commit().unwrap();
    let mut query = db.prepare("SELECT term FROM v").unwrap();
    let terms: Vec<String> = query
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut out = BTreeMap::new();
    for term in terms {
        let Some((index, rest)) = term.strip_prefix('k').and_then(|t| t.split_once('k')) else {
            continue;
        };
        let (Ok(index), Some(kept)) = (index.parse::<usize>(), rest.strip_suffix('k')) else {
            continue;
        };
        let mut it = kept.chars();
        let folded = it.next();
        if it.next().is_none() {
            out.insert(chars[index], folded);
        }
    }
    out
}

fn base_lower(c: char) -> Option<char> {
    c.to_lowercase().next()
}

fn runs(dropped: &[char]) -> Vec<(char, char)> {
    let mut out: Vec<(char, char)> = Vec::new();
    for &c in dropped {
        match out.last_mut() {
            Some(last) if last.1 as u32 + 1 == c as u32 => last.1 = c,
            _ => out.push((c, c)),
        }
    }
    out
}

#[test]
fn fold_matches_sqlite_below_0x2500() {
    let sqlite = sqlite_folds();
    assert!(sqlite.len() > 2000, "{} code points compared", sqlite.len());
    let mut wrong = Vec::new();
    for (&c, &folded) in &sqlite {
        let expected = folded.map(|f| if f == 'đ' { 'd' } else { f });
        if fold_char(c) != expected {
            wrong.push(c);
        }
    }
    if !wrong.is_empty() {
        let table: Vec<(char, char)> = sqlite
            .iter()
            .filter_map(|(&c, &f)| f.filter(|&f| Some(f) != base_lower(c)).map(|f| (c, f)))
            .collect();
        let dropped: Vec<char> = sqlite
            .iter()
            .filter_map(|(&c, f)| f.is_none().then_some(c))
            .collect();
        println!("pub(crate) const TABLE: &[(char, char)] = &{table:?};");
        println!(
            "pub(crate) const DROPPED: &[(char, char)] = &{:?};",
            runs(&dropped)
        );
    }
    assert!(wrong.is_empty(), "{} code points differ", wrong.len());
    assert!(TABLE.windows(2).all(|w| w[0].0 < w[1].0));
    assert!(DROPPED.windows(2).all(|w| w[0].1 < w[1].0));
}

#[test]
fn d_stroke_folds_to_d() {
    assert_eq!(fold("Đăng nhập"), "dang nhap");
    assert_eq!(fold("ĐỒNG"), "dong");
    assert_eq!(fold_char('đ'), Some('d'));
}

#[test]
fn stacked_and_precomposed_fold_alike() {
    assert_eq!(fold("đa\u{0306}ng"), fold("đăng"));
    assert_eq!(fold("đa\u{0306}ng"), "dang");
}

#[test]
fn words_split_on_punctuation() {
    assert_eq!(words("phím-tắt, Login!"), ["phim", "tat", "login"]);
    assert!(words("  -- ").is_empty());
}
