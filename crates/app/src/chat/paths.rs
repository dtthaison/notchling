use std::path::{Path, PathBuf};

use crate::chat::drops::percent_decoded;
use crate::chat::markdown::Span;

pub trait Probe {
    fn exists(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    fn home(&self) -> Option<&Path>;
}

pub struct Disk {
    pub home: Option<PathBuf>,
}

impl Disk {
    pub fn new() -> Disk {
        Disk {
            home: notchling_platform::paths::home()
                .or_else(|| std::env::var_os("HOME").map(PathBuf::from)),
        }
    }
}

impl Default for Disk {
    fn default() -> Disk {
        Disk::new()
    }
}

impl Probe for Disk {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }
    fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }
}

fn splits(c: char) -> bool {
    c.is_whitespace() || "()[]<>\"'`,;|".contains(c)
}

fn is_sep(b: u8) -> bool {
    b == b'/' || b == b'\\'
}

fn is_drive_any(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

fn is_drive_abs(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && is_sep(b[2])
}

fn split_unc(s: &str) -> Option<(&str, &str, &str)> {
    let after = s.strip_prefix("\\\\")?;
    let sep = ['\\', '/'];
    let i = after.find(sep)?;
    let server = &after[..i];
    if server.is_empty() || server == "." || server == ".." {
        return None;
    }
    let tail = &after[i + 1..];
    if tail.starts_with(sep) {
        return None;
    }
    let (share, rest) = match tail.find(sep) {
        Some(j) => (&tail[..j], &tail[j + 1..]),
        None => (tail, ""),
    };
    if share.is_empty() || share == "." || share == ".." {
        return None;
    }
    Some((server, share, rest))
}

fn is_abs_token(s: &str) -> bool {
    s.starts_with('/') || is_drive_abs(s) || split_unc(s).is_some()
}

fn is_windows_style(s: &str) -> bool {
    is_drive_any(s) || s.contains('\\')
}

fn is_bad_or_bare_root(token: &str) -> bool {
    if matches!(token, "/" | "~" | "~/" | "~\\") {
        return true;
    }
    if is_drive_abs(token) && token[3..].trim_matches(['/', '\\']).is_empty() {
        return true;
    }
    if is_drive_any(token) && !is_drive_abs(token) {
        return true;
    }
    if token.starts_with('\\') {
        return match split_unc(token) {
            Some((_, _, rest)) => rest.trim_matches(['/', '\\']).is_empty(),
            None => true,
        };
    }
    false
}

fn fold_rooted<'a>(rest: &'a str, sep: &[char]) -> Vec<&'a str> {
    let mut parts = Vec::new();
    for part in rest.split(sep) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts
}

fn fold_relative<'a>(rest: &'a str, sep: &[char]) -> Vec<&'a str> {
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split(sep) {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|last| *last != "..") {
                    parts.pop();
                } else {
                    parts.push("..");
                }
            }
            p => parts.push(p),
        }
    }
    parts
}

fn join_str(base: &str, rel: &str) -> PathBuf {
    let sep = if is_windows_style(base) { '\\' } else { '/' };
    let base = base.trim_end_matches(['/', '\\']);
    if base.is_empty() {
        PathBuf::from(format!("{sep}{rel}"))
    } else {
        PathBuf::from(format!("{base}{sep}{rel}"))
    }
}

pub fn tokens(text: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut push = |from: usize, to: usize| {
        let token = text[from..to].trim_end_matches(['.', ':', '!', '?']);
        if token.contains(['/', '\\']) {
            out.push((from..from + token.len(), token));
        }
    };
    for (at, c) in text.char_indices() {
        match (splits(c), start) {
            (true, Some(from)) => {
                push(from, at);
                start = None;
            }
            (false, None) => start = Some(at),
            _ => {}
        }
    }
    if let Some(from) = start {
        push(from, text.len());
    }
    out
}

pub fn normalized(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some((server, share, rest)) = split_unc(&s) {
        let parts = fold_rooted(rest, &['/', '\\']);
        if parts.is_empty() {
            return PathBuf::from(format!("\\\\{server}\\{share}"));
        }
        let joined = parts.join("\\");
        return PathBuf::from(format!("\\\\{server}\\{share}\\{joined}"));
    }
    if is_drive_abs(&s) {
        let drive = s.as_bytes()[0] as char;
        let parts = fold_rooted(&s[3..], &['/', '\\']);
        if parts.is_empty() {
            return PathBuf::from(format!("{drive}:\\"));
        }
        return PathBuf::from(format!("{drive}:\\{}", parts.join("\\")));
    }
    if let Some(rest) = s.strip_prefix('/') {
        let parts = fold_rooted(rest, &['/']);
        if parts.is_empty() {
            return PathBuf::from("/");
        }
        return PathBuf::from(format!("/{}", parts.join("/")));
    }
    if is_windows_style(&s) {
        return PathBuf::from(fold_relative(&s, &['/', '\\']).join("\\"));
    }
    PathBuf::from(fold_relative(&s, &['/']).join("/"))
}

fn expanded(token: &str, probe: &dyn Probe) -> Option<PathBuf> {
    if token == "~" {
        return probe.home().map(Path::to_path_buf);
    }
    if let Some(rest) = token.strip_prefix("~/") {
        return probe.home().map(|h| join_str(&h.to_string_lossy(), rest));
    }
    if let Some(rest) = token.strip_prefix("~\\") {
        return probe.home().map(|h| {
            let hs = h.to_string_lossy();
            if is_windows_style(&hs) {
                join_str(&hs, rest)
            } else {
                join_str(&hs, &rest.replace('\\', "/"))
            }
        });
    }
    if token.starts_with('~') {
        return None;
    }
    Some(PathBuf::from(token))
}

pub fn bases(text: &str, cwd: &str, probe: &dyn Probe) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for (_, token) in tokens(text) {
        if !(is_abs_token(token) || token.starts_with("~/") || token.starts_with("~\\")) {
            continue;
        }
        if let Some(path) = expanded(token, probe).map(|p| normalized(&p)) {
            if probe.is_dir(&path) && !out.contains(&path) {
                out.push(path);
            }
        }
    }
    if !cwd.is_empty() {
        let cwd = PathBuf::from(cwd);
        if !out.contains(&cwd) {
            out.push(cwd);
        }
    }
    out
}

pub(crate) fn without_line(token: &str) -> Option<&str> {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if let Some(i) = token.rfind("#L") {
        let tail = &token[i + 2..];
        if tail
            .split('-')
            .all(|p| digits(p.strip_prefix('L').unwrap_or(p)))
        {
            return Some(&token[..i]);
        }
    }
    let mut cut = token.len();
    let mut seen = 0;
    while seen < 2 {
        let Some(i) = token[..cut].rfind(':') else {
            break;
        };
        if i == 1 && token.as_bytes()[0].is_ascii_alphabetic() {
            break;
        }
        if !digits(&token[i + 1..cut]) {
            break;
        }
        cut = i;
        seen += 1;
    }
    (seen > 0).then(|| &token[..cut])
}

pub fn resolve(token: &str, bases: &[PathBuf], bare: bool, probe: &dyn Probe) -> Option<PathBuf> {
    let direct = |token: &str| -> Option<PathBuf> {
        if token.is_empty() || token.contains("://") || is_bad_or_bare_root(token) {
            return None;
        }
        let slashed = token.contains(['/', '\\']);
        let named = bare
            && !slashed
            && !token.starts_with('~')
            && token
                .rsplit_once('.')
                .is_some_and(|(stem, ext)| !stem.is_empty() && !ext.is_empty());
        if !slashed && !named {
            return None;
        }
        if is_abs_token(token) || token.starts_with('~') {
            let path = normalized(&expanded(token, probe)?);
            return probe.exists(&path).then_some(path);
        }
        bases.iter().find_map(|base| {
            let path = normalized(&join_str(&base.to_string_lossy(), token));
            probe.exists(&path).then_some(path)
        })
    };
    direct(token).or_else(|| without_line(token).and_then(direct))
}

fn destination(url: &str) -> Option<String> {
    let url = url.trim();
    let raw = match url.strip_prefix("file://") {
        Some(rest) => rest,
        None if url.contains("://") || url.starts_with("mailto:") => return None,
        None => url,
    };
    let decoded = percent_decoded(raw);
    if let Some(after) = decoded.strip_prefix('/') {
        if is_drive_abs(after) {
            return Some(after.to_owned());
        }
    }
    Some(decoded)
}

pub fn link_spans(spans: Vec<Span>, bases: &[PathBuf], probe: &dyn Probe) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        if let Some(url) = &span.link {
            let found = destination(url).and_then(|d| resolve(&d, bases, true, probe));
            out.push(Span {
                link: found
                    .map(|p| p.to_string_lossy().into_owned())
                    .or(span.link.clone()),
                ..span
            });
            continue;
        }
        let mut found: Vec<(std::ops::Range<usize>, PathBuf)> = tokens(&span.text)
            .into_iter()
            .filter_map(|(r, t)| resolve(t, bases, false, probe).map(|p| (r, p)))
            .collect();
        let word = span.text.trim();
        if found.is_empty() && span.code && !word.contains(char::is_whitespace) {
            if let (Some(p), Some(from)) = (resolve(word, bases, true, probe), span.text.find(word))
            {
                found.push((from..from + word.len(), p));
            }
        }
        if found.is_empty() {
            out.push(span);
            continue;
        }
        let mut at = 0;
        let mut split: Vec<Span> = Vec::new();
        let piece = |text: &str, link: Option<String>| Span {
            text: text.to_owned(),
            link,
            ..span.clone()
        };
        for (range, path) in found {
            if range.start > at {
                split.push(piece(&span.text[at..range.start], None));
            }
            split.push(piece(
                &span.text[range.clone()],
                Some(path.to_string_lossy().into_owned()),
            ));
            at = range.end;
        }
        if at < span.text.len() {
            split.push(piece(&span.text[at..], None));
        }
        out.extend(split);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::markdown::spans;
    use crate::testutil::Scratch;

    struct Temp {
        home: PathBuf,
    }
    impl Probe for Temp {
        fn exists(&self, path: &Path) -> bool {
            path.exists()
        }
        fn is_dir(&self, path: &Path) -> bool {
            path.is_dir()
        }
        fn home(&self) -> Option<&Path> {
            Some(&self.home)
        }
    }

    fn words(spans: &[Span]) -> Vec<(String, Option<String>)> {
        spans
            .iter()
            .map(|s| (s.text.clone(), s.link.clone()))
            .collect()
    }

    #[test]
    fn tokens_are_the_words_with_a_slash_and_no_trailing_punctuation() {
        let text = "See `film/plan.md`, then (~/a/b/). Also src/x.rs: and a.b and http://x.y/z?";
        let got: Vec<&str> = tokens(text).into_iter().map(|(_, t)| t).collect();
        assert_eq!(
            got,
            vec!["film/plan.md", "~/a/b/", "src/x.rs", "http://x.y/z"]
        );
        let (range, token) = tokens("go to /tmp/x.")[0].clone();
        assert_eq!((range, token), (6..12, "/tmp/x"));
        assert!(tokens("no paths here").is_empty());
    }

    #[test]
    fn dots_are_folded_without_the_disk() {
        assert_eq!(
            normalized(Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
        assert_eq!(normalized(Path::new("/../a")), PathBuf::from("/a"));
    }

    #[test]
    fn bases_are_the_folders_the_message_names_then_the_session() {
        let s = Scratch::new("paths-bases");
        let reel = s.write("reel/edit/a.txt", "x");
        let reel = reel.parent().unwrap().to_path_buf();
        let probe = Temp {
            home: s.path().to_path_buf(),
        };
        let text = format!(
            "The project is in {}/ and ~/reel/edit/ and {}/nope/ and a file {}/a.txt",
            reel.display(),
            s.path().display(),
            reel.display()
        );
        let got = bases(&text, "/work/session", &probe);
        assert_eq!(got, vec![reel, PathBuf::from("/work/session")]);
        assert_eq!(
            bases("nothing", "", &probe),
            Vec::<PathBuf>::new(),
            "no session folder, no bases"
        );
    }

    #[test]
    fn a_path_resolves_only_when_it_exists() {
        let s = Scratch::new("paths-resolve");
        let plan = s.write("film/plan/colab/plan.md", "# Plan");
        s.write("other/plan/colab/plan.md", "# Other");
        let probe = Temp {
            home: s.path().to_path_buf(),
        };
        let base = vec![s.path().join("film"), s.path().join("other")];
        assert_eq!(
            resolve("plan/colab/plan.md", &base, false, &probe),
            Some(plan.clone())
        );
        assert_eq!(
            resolve("./plan/../plan/colab/plan.md", &base, false, &probe),
            Some(plan.clone())
        );
        assert_eq!(
            resolve(&plan.to_string_lossy(), &[], false, &probe),
            Some(plan.clone())
        );
        assert_eq!(
            resolve("~/film/plan/colab/plan.md", &[], false, &probe),
            Some(plan.clone())
        );
        for token in [
            "plan/colab/plan.md:12",
            "plan/colab/plan.md:12:3",
            "plan/colab/plan.md#L7-L9",
        ] {
            assert_eq!(
                resolve(token, &base, false, &probe),
                Some(plan.clone()),
                "{token}"
            );
        }
        for token in [
            "plan/colab/nope.md",
            "https://example.com/plan/colab/plan.md",
            "/",
            "~/",
            "plan.md",
            "~film/plan/colab/plan.md",
        ] {
            assert_eq!(resolve(token, &base, false, &probe), None, "{token}");
        }
        assert_eq!(
            resolve("plan/colab", &base, false, &probe),
            Some(s.path().join("film/plan/colab"))
        );
    }

    #[test]
    fn a_bare_file_name_in_code_is_tried_under_the_bases() {
        let s = Scratch::new("paths-bare");
        let readme = s.write("proj/README.md", "hi");
        let probe = Temp {
            home: s.path().to_path_buf(),
        };
        let base = vec![s.path().join("proj")];
        assert_eq!(resolve("README.md", &base, true, &probe), Some(readme));
        assert_eq!(resolve("README.md", &base, false, &probe), None);
        assert_eq!(resolve("README", &base, true, &probe), None, "no extension");
        assert_eq!(resolve("MISSING.md", &base, true, &probe), None);
    }

    #[test]
    fn existing_paths_in_prose_and_code_become_links_and_the_rest_stays_text() {
        let s = Scratch::new("paths-link");
        let plan = s.write("film/plan/colab/plan.md", "# Plan");
        let probe = Temp {
            home: s.path().to_path_buf(),
        };
        let base = vec![s.path().to_path_buf()];
        let plan_s = plan.to_string_lossy().into_owned();

        let got = link_spans(spans("Open `film/plan/colab/plan.md` now"), &base, &probe);
        assert_eq!(
            words(&got),
            vec![
                ("Open ".into(), None),
                ("film/plan/colab/plan.md".into(), Some(plan_s.clone())),
                (" now".into(), None),
            ]
        );
        assert!(got[1].code);

        let got = link_spans(spans("It is in film/plan/colab/plan.md."), &base, &probe);
        assert_eq!(
            words(&got),
            vec![
                ("It is in ".into(), None),
                ("film/plan/colab/plan.md".into(), Some(plan_s.clone())),
                (".".into(), None),
            ]
        );

        let got = link_spans(spans("`open film/plan/colab/plan.md`"), &base, &probe);
        assert_eq!(
            words(&got),
            vec![
                ("open ".into(), None),
                ("film/plan/colab/plan.md".into(), Some(plan_s.clone())),
            ]
        );

        let text = "`film/nope.md` and [site](https://example.com/film/plan.md)";
        let got = link_spans(spans(text), &base, &probe);
        assert_eq!(
            words(&got),
            vec![
                ("film/nope.md".into(), None),
                (" and ".into(), None),
                (
                    "site".into(),
                    Some("https://example.com/film/plan.md".into())
                ),
            ]
        );
    }

    #[test]
    fn a_markdown_destination_is_made_absolute_when_it_exists() {
        let s = Scratch::new("paths-dest");
        let doc = s.write("docs/my file.md", "x");
        let probe = Temp {
            home: s.path().to_path_buf(),
        };
        let base = vec![s.path().join("docs")];
        let got = link_spans(
            spans("[a](my%20file.md) [b](gone.md) [c](mailto:a@b.c)"),
            &base,
            &probe,
        );
        assert_eq!(got[0].link.as_deref(), Some(&*doc.to_string_lossy()));
        assert_eq!(
            got[2].link.as_deref(),
            Some("gone.md"),
            "a missing one is left as written"
        );
        assert_eq!(got[4].link.as_deref(), Some("mailto:a@b.c"));
    }

    #[test]
    fn a_file_url_with_escaped_spaces_links_the_file() {
        let s = Scratch::new("paths-file-url");
        let prompt = s.write("the batch/prompts/S09-end.txt", "a prompt");
        let probe = Temp {
            home: s.path().to_path_buf(),
        };
        let url = format!("file://{}", prompt.to_string_lossy().replace(' ', "%20"));
        let got = link_spans(
            spans(&format!("Saved to [S09-end.txt]({url}).")),
            &[],
            &probe,
        );
        let linked: Vec<_> = got.iter().filter_map(|s| s.link.as_deref()).collect();
        assert_eq!(linked, vec![&*prompt.to_string_lossy()]);
    }

    struct Fake {
        home: PathBuf,
        files: &'static [&'static str],
        dirs: &'static [&'static str],
    }
    impl Probe for Fake {
        fn exists(&self, path: &Path) -> bool {
            let Some(s) = path.to_str() else {
                return false;
            };
            self.files.contains(&s) || self.dirs.contains(&s)
        }
        fn is_dir(&self, path: &Path) -> bool {
            path.to_str().is_some_and(|s| self.dirs.contains(&s))
        }
        fn home(&self) -> Option<&Path> {
            Some(&self.home)
        }
    }

    #[test]
    fn windows_paths_tokens_bases_resolve_and_file_urls_work_on_any_os() {
        let sample = r"See `film\plan.md`, (~\a\b\), and C:\Users\me\x.rs:12:";
        let got: Vec<&str> = tokens(sample).into_iter().map(|(_, t)| t).collect();
        assert_eq!(
            got,
            vec![r"film\plan.md", r"~\a\b\", r"C:\Users\me\x.rs:12"]
        );
        assert_eq!(
            normalized(Path::new(r"C:\..\a\b\..\c\.\d")),
            PathBuf::from(r"C:\a\c\d")
        );
        assert_eq!(
            normalized(Path::new(r"\\srv\share\..\a\.\b")),
            PathBuf::from(r"\\srv\share\a\b")
        );

        let probe = Fake {
            home: PathBuf::from(r"C:\Users\me"),
            files: &[
                r"C:\Users\me\film\plan\colab\plan.md",
                r"C:\Users\me\the batch\end.txt",
                r"\\srv\share\docs\spec.md",
            ],
            dirs: &[
                r"C:\Users\me\film",
                r"C:\Users\me\film\plan\colab",
                r"\\srv\share\docs",
            ],
        };
        let text = r"In C:\Users\me\film\ and ~\film\ and \\srv\share\docs\ and C:\Users\me\nope\";
        assert_eq!(
            bases(text, r"C:\work\session", &probe),
            vec![
                PathBuf::from(r"C:\Users\me\film"),
                PathBuf::from(r"\\srv\share\docs"),
                PathBuf::from(r"C:\work\session"),
            ]
        );

        let base = vec![PathBuf::from(r"C:\Users\me\film")];
        let plan = PathBuf::from(r"C:\Users\me\film\plan\colab\plan.md");
        for token in [
            r"plan\colab\plan.md",
            r".\plan\..\plan\colab\plan.md",
            r"plan\colab\plan.md:12:3",
            r"plan\colab\plan.md#L7-L9",
            r"C:\Users\me\film\plan\colab\plan.md:9",
            "C:/Users/me/film/plan/colab/plan.md",
            r"~\film\plan\colab\plan.md",
        ] {
            assert_eq!(
                resolve(token, &base, false, &probe),
                Some(plan.clone()),
                "{token}"
            );
        }
        assert_eq!(
            resolve(r"\\srv\share\docs\spec.md:4", &[], false, &probe),
            Some(PathBuf::from(r"\\srv\share\docs\spec.md"))
        );
        for bad in [
            r"C:\",
            "C:/",
            "C:",
            r"C:plan\colab\plan.md",
            r"~\",
            r"~other\film\plan\colab\plan.md",
            r"\\srv\share",
            r"\Users\me\film\plan\colab\plan.md",
        ] {
            assert_eq!(resolve(bad, &base, false, &probe), None, "{bad}");
        }

        let a = "file:///C:/Users/me/the%20batch/end.txt";
        let b = r"file://C:\Users\me\the%20batch\end.txt";
        let md = format!(r"Saved [a]({a}) and [b]({b}) and `plan\colab\plan.md`.");
        let got = link_spans(spans(&md), &base, &probe);
        let linked: Vec<_> = got.iter().filter_map(|s| s.link.as_deref()).collect();
        assert_eq!(
            linked,
            vec![
                r"C:\Users\me\the batch\end.txt",
                r"C:\Users\me\the batch\end.txt",
                r"C:\Users\me\film\plan\colab\plan.md",
            ]
        );
    }
}
