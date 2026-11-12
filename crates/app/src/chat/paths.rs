//! Paths in a message (the Swift `ChatPaths`): a word that names a file or folder that exists
//! becomes a link, in prose and in inline code alike. A relative path ("film/plan/plan.md") is read
//! against the folders the same message names ("the project is in ~/Downloads/reel/"), then the
//! session's folder. Pure over a [`Probe`], so it is tested on every system with a temp folder.

use std::path::{Component, Path, PathBuf};

use crate::chat::drops::percent_decoded;
use crate::chat::markdown::Span;

/// What the rules ask of the disk and the home folder.
pub trait Probe {
    fn exists(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    fn home(&self) -> Option<&Path>;
}

/// The real disk.
pub struct Disk {
    pub home: Option<PathBuf>,
}

impl Disk {
    pub fn new() -> Disk {
        Disk {
            home: std::env::var_os("HOME").map(PathBuf::from),
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

/// A word that is not a path: it has a space, a quote, a bracket, a comma, a semicolon or a pipe.
fn splits(c: char) -> bool {
    c.is_whitespace() || "()[]<>\"'`,;|".contains(c)
}

/// The words of `text` that could be paths (they hold a `/`), without trailing `.`, `:`, `!` or
/// `?`, with where each sits (byte range).
pub fn tokens(text: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut push = |from: usize, to: usize| {
        let token = text[from..to].trim_end_matches(['.', ':', '!', '?']);
        if token.contains('/') {
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

/// `.` and `..` folded away without touching the disk (a `..` past the root stays at the root).
pub fn normalized(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                let last = out.components().next_back();
                if matches!(last, Some(Component::Normal(_))) {
                    out.pop();
                } else if !matches!(last, Some(Component::RootDir)) {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `~` and `~/…` as the home folder's path; any other `~name` is not expanded.
fn expanded(token: &str, probe: &dyn Probe) -> Option<PathBuf> {
    if token == "~" {
        return probe.home().map(Path::to_path_buf);
    }
    match token.strip_prefix("~/") {
        Some(rest) => probe.home().map(|h| h.join(rest)),
        None if token.starts_with('~') => None,
        None => Some(PathBuf::from(token)),
    }
}

/// The folders to read relative paths against: absolute folders the text names (in the order it
/// names them), then the session's folder.
pub fn bases(text: &str, cwd: &str, probe: &dyn Probe) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for (_, token) in tokens(text) {
        if !(token.starts_with('/') || token.starts_with("~/")) {
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

/// `path` without a trailing line number (`:12`, `:12:3`, `#L12`, `#L12-L20`), if it has one.
fn without_line(token: &str) -> Option<&str> {
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
        if !digits(&token[i + 1..cut]) {
            break;
        }
        cut = i;
        seen += 1;
    }
    (seen > 0).then(|| &token[..cut])
}

/// The existing file or folder `token` names, or `None`. An absolute or `~` path is taken as it
/// is; a relative one is tried under each base in turn. A word with no `/`, a URL, and a bare root
/// are never paths. With `bare`, a lone file name with an extension (`plan.md`, inline code only)
/// is tried under the bases too.
pub fn resolve(token: &str, bases: &[PathBuf], bare: bool, probe: &dyn Probe) -> Option<PathBuf> {
    let direct = |token: &str| -> Option<PathBuf> {
        if token.is_empty() || token.contains("://") || token == "/" || token == "~/" {
            return None;
        }
        let slashed = token.contains('/');
        let named = bare
            && !slashed
            && !token.starts_with('~')
            && token
                .rsplit_once('.')
                .is_some_and(|(stem, ext)| !stem.is_empty() && !ext.is_empty());
        if !slashed && !named {
            return None;
        }
        if token.starts_with('/') || token.starts_with('~') {
            let path = normalized(&expanded(token, probe)?);
            return probe.exists(&path).then_some(path);
        }
        bases.iter().find_map(|base| {
            let path = normalized(&base.join(token));
            probe.exists(&path).then_some(path)
        })
    };
    direct(token).or_else(|| without_line(token).and_then(direct))
}

/// A Markdown destination as a path: percent escapes undone, a `file://` start dropped. `None`
/// for a web link, a `mailto:` and the like (anything with a scheme).
fn destination(url: &str) -> Option<String> {
    let url = url.trim();
    let raw = match url.strip_prefix("file://") {
        Some(rest) => rest,
        None if url.contains("://") || url.starts_with("mailto:") => return None,
        None => url,
    };
    Some(percent_decoded(raw))
}

/// The spans with every existing path made a link to its absolute path: inside prose and inline
/// code, a word that names one becomes a linked piece of its span (the span's own style kept); a
/// link's own destination that names one is replaced by the absolute path, so a click finds it
/// whatever folder it was written against. Spans that link to the web are left alone.
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
        // `plan.md`: no slash, so no token, but inline code that is one file name under a base.
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
        // The folder named twice (absolute, then with `~`) once; a missing one and a file are not
        // bases; the session's folder is last.
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
        // Relative: the first base that has it.
        assert_eq!(
            resolve("plan/colab/plan.md", &base, false, &probe),
            Some(plan.clone())
        );
        assert_eq!(
            resolve("./plan/../plan/colab/plan.md", &base, false, &probe),
            Some(plan.clone())
        );
        // Absolute and `~`.
        assert_eq!(
            resolve(&plan.to_string_lossy(), &[], false, &probe),
            Some(plan.clone())
        );
        assert_eq!(
            resolve("~/film/plan/colab/plan.md", &[], false, &probe),
            Some(plan.clone())
        );
        // A line number is not part of the name.
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
        // Missing, a URL, a bare root, a word with no slash, `~name`.
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
        // A folder counts.
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

        // Inline code that is the path: one linked span, still code.
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

        // Prose: the word links, the sentence's dot does not.
        let got = link_spans(spans("It is in film/plan/colab/plan.md."), &base, &probe);
        assert_eq!(
            words(&got),
            vec![
                ("It is in ".into(), None),
                ("film/plan/colab/plan.md".into(), Some(plan_s.clone())),
                (".".into(), None),
            ]
        );

        // Code with a command: only the path inside links.
        let got = link_spans(spans("`open film/plan/colab/plan.md`"), &base, &probe);
        assert_eq!(
            words(&got),
            vec![
                ("open ".into(), None),
                ("film/plan/colab/plan.md".into(), Some(plan_s.clone())),
            ]
        );

        // A path that isn't there stays as written; so does a web link.
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

    /// Antigravity links a file it wrote as an absolute `file://` URL with its spaces escaped.
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
}
