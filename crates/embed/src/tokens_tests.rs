use crate::check_strings::check_strings;
use crate::scratch::Scratch;
use crate::testing::TINY;
use crate::tokens::Tokens;
use std::path::{Path, PathBuf};

#[test]
fn cut_to_max_keeps_the_specials() {
    let tokens = Tokens::from_tokenizer_slice(TINY.as_bytes()).expect("tiny tokenizer");
    assert_eq!(tokens.cls_id(), 0);
    assert_eq!(tokens.pad_id(), 3);
    assert_eq!(tokens.encode("abc ab c", 512), vec![0, 8, 7, 6, 1]);
    assert_eq!(tokens.encode("abc abc abc ab", 4), vec![0, 8, 8, 1]);
    assert_eq!(tokens.encode("abc", 2), vec![0, 1]);
    assert_eq!(tokens.encode("", 8), vec![0, 1]);
}

#[test]
fn the_native_format_keeps_ids_and_specials() {
    let json = Tokens::from_tokenizer_slice(TINY.as_bytes()).expect("tiny tokenizer");
    let native = Tokens::from_native_slice(&json.to_native_bytes()).expect("native");
    assert_eq!(native.cls_id(), 0);
    assert_eq!(native.pad_id(), 3);
    for text in ["abc ab c", "abc abc abc ab", "abc</s>ab", ""] {
        for max in [3, 4, 512] {
            assert_eq!(
                native.encode(text, max),
                json.encode(text, max),
                "{text:?} {max}"
            );
        }
    }
    assert!(Tokens::from_native_slice(b"not a tokenizer").is_err());
}

#[test]
fn tiny_tokenizer_matches_tokenizers() {
    let ours = Tokens::from_tokenizer_slice(TINY.as_bytes()).expect("tiny tokenizer");
    let theirs = tokenizers::Tokenizer::from_bytes(TINY.as_bytes()).expect("tokenizers");
    for text in ["abc ab c", "a b c abc", "abc</s>ab", "cab  ba\tabc"] {
        let want = theirs
            .encode(text, true)
            .expect("encode")
            .get_ids()
            .to_vec();
        assert_eq!(ours.encode(text, usize::MAX), want, "{text:?}");
    }
}

#[test]
fn there_are_200_check_strings() {
    let strings = check_strings();
    assert_eq!(strings.len(), 200);
    assert_eq!(strings[199].chars().count(), 32_000);
    assert!(strings.iter().any(|s| s.contains("\r\n")));
    assert!(strings.iter().any(|s| s.contains('\t')));
    assert!(strings.iter().any(|s| s.contains('\u{323}')));
}

fn data_texts() -> Vec<String> {
    fn walk(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::String(s) => out.push(s.clone()),
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            serde_json::Value::Object(o) => o.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/embed-eval/data");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("data dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&f).expect("read")).expect("json");
        walk(&v, &mut out);
    }
    out
}

fn parity(file: &Path) -> Result<usize, String> {
    let ours = Tokens::from_tokenizer_json(file)?;
    let mut theirs = tokenizers::Tokenizer::from_file(file).map_err(|e| e.to_string())?;
    theirs.with_padding(None);
    theirs.with_truncation(None).map_err(|e| e.to_string())?;
    let mut texts = check_strings();
    texts.extend(data_texts());
    for text in &texts {
        let want = theirs
            .encode(text.as_str(), true)
            .map_err(|e| e.to_string())?
            .get_ids()
            .to_vec();
        let got = ours.encode(text, usize::MAX);
        if got != want {
            let at = got.iter().zip(&want).take_while(|(a, b)| a == b).count();
            let shown: String = text.chars().take(80).collect();
            return Err(format!(
                "{}: first difference at id {at} in {shown:?}",
                file.display()
            ));
        }
    }
    Ok(texts.len())
}

#[test]
#[ignore = "needs NOTCHLING_EMBED_MODELS"]
fn kitoken_matches_tokenizers_on_the_check_strings() {
    let root =
        PathBuf::from(std::env::var("NOTCHLING_EMBED_MODELS").expect("NOTCHLING_EMBED_MODELS"));
    let granite = root.join("ibm-granite__granite-embedding-97m-multilingual-r2/tokenizer.json");
    let n = parity(&granite).expect("granite-r2 parity");
    println!("granite-r2: {n} texts identical");
    let stat = root.join("sentence-transformers__static-similarity-mrl-multilingual-v1/0_StaticEmbedding/tokenizer.json");
    match parity(&stat) {
        Ok(n) => println!("static-mrl: {n} texts identical"),
        Err(e) => panic!("static-mrl differs: {e}"),
    }
}

fn tiny() -> Tokens {
    Tokens::from_tokenizer_slice(TINY.as_bytes()).expect("tiny tokenizer")
}

#[test]
fn the_native_file_round_trips() {
    let dir = Scratch::new("nl-tokens-test").expect("scratch");
    let file = dir.join("tokenizer.notchling");
    tiny().write_native_file(&file).expect("write");
    let back = Tokens::from_native_file(&file).expect("read");
    assert_eq!(back.encode("abc ab c", 512), vec![0, 8, 7, 6, 1]);
    assert!(!dir.join("tokenizer.notchling.part").exists());
}

#[test]
fn a_stale_part_file_is_replaced() {
    let dir = Scratch::new("nl-tokens-test").expect("scratch");
    let file = dir.join("tokenizer.notchling");
    std::fs::write(dir.join("tokenizer.notchling.part"), b"half a file").expect("stale");
    tiny().write_native_file(&file).expect("write");
    assert!(Tokens::from_native_file(&file).is_ok());
    assert!(!dir.join("tokenizer.notchling.part").exists());
}

#[cfg(unix)]
#[test]
fn the_native_file_is_private_to_its_owner() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new("nl-tokens-test").expect("scratch");
    let file = dir.join("tokenizer.notchling");
    tiny().write_native_file(&file).expect("write");
    let mode = std::fs::metadata(&file).expect("meta").permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[cfg(unix)]
#[test]
fn a_link_at_the_target_is_replaced_not_followed() {
    let dir = Scratch::new("nl-tokens-test").expect("scratch");
    let victim = dir.join("victim");
    std::fs::write(&victim, b"keep me").expect("victim");
    let file = dir.join("tokenizer.notchling");
    std::os::unix::fs::symlink(&victim, &file).expect("link");
    tiny().write_native_file(&file).expect("write");
    assert_eq!(std::fs::read(&victim).expect("victim"), b"keep me");
    assert!(!std::fs::symlink_metadata(&file)
        .expect("meta")
        .file_type()
        .is_symlink());
    assert!(Tokens::from_native_file(&file).is_ok());
}

#[cfg(unix)]
#[test]
fn a_link_at_the_part_file_is_never_followed() {
    let dir = Scratch::new("nl-tokens-test").expect("scratch");
    let victim = dir.join("victim");
    std::fs::write(&victim, b"keep me").expect("victim");
    let file = dir.join("tokenizer.notchling");
    std::os::unix::fs::symlink(&victim, dir.join("tokenizer.notchling.part")).expect("link");
    tiny().write_native_file(&file).expect("write");
    assert_eq!(std::fs::read(&victim).expect("victim"), b"keep me");
    assert!(Tokens::from_native_file(&file).is_ok());
}

#[test]
fn a_failed_write_leaves_no_part_file() {
    let dir = Scratch::new("nl-tokens-test").expect("scratch");
    let file = dir.join("no-such-folder/tokenizer.notchling");
    assert!(tiny().write_native_file(&file).is_err());
    assert!(!dir.join("no-such-folder").exists());
}
