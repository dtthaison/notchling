use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::chat::drops::{has_scheme, percent_decoded};
use crate::chat::paths::{normalized, without_line};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Link(String),
    Path(PathBuf),
}

impl Target {
    pub fn text(&self) -> String {
        match self {
            Target::Link(url) => url.clone(),
            Target::Path(path) => path.to_string_lossy().into_owned(),
        }
    }
}

pub fn link_at(links: &[(Range<usize>, String)], index: usize) -> Option<&str> {
    links
        .iter()
        .find(|(range, _)| range.contains(&index))
        .map(|(_, url)| url.as_str())
}

pub fn resolve(
    url: &str,
    folder: &str,
    home: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> Option<Target> {
    let url = url.trim();
    if url.is_empty() || url.starts_with('#') {
        return None;
    }
    let raw = match url.strip_prefix("file://") {
        Some(rest) => rest,
        None if has_scheme(url) => return Some(Target::Link(url.to_owned())),
        None => url,
    };
    let text = percent_decoded(raw);
    let absolute = |token: &str| -> Option<PathBuf> {
        let path = if token == "~" {
            home?.to_path_buf()
        } else if let Some(rest) = token.strip_prefix("~/") {
            home?.join(rest)
        } else if token.starts_with('/') {
            PathBuf::from(token)
        } else if folder.is_empty() {
            return None;
        } else {
            Path::new(folder).join(token)
        };
        Some(normalized(&path))
    };
    if raw != text {
        if let Some(literal) = absolute(raw).filter(|path| exists(path)) {
            return Some(Target::Path(literal));
        }
    }
    let whole = absolute(&text)?;
    if exists(&whole) {
        return Some(Target::Path(whole));
    }
    let bare = without_line(&text)
        .and_then(absolute)
        .filter(|path| exists(path));
    Some(Target::Path(bare.unwrap_or(whole)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(known: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |p| known.iter().any(|k| Path::new(k) == p)
    }

    const FOLDER: &str = "/work/app";
    const HOME: &str = "/Users/me";

    fn path(url: &str, known: &'static [&'static str]) -> Option<Target> {
        resolve(url, FOLDER, Some(Path::new(HOME)), has(known))
    }

    fn at(p: &str) -> Option<Target> {
        Some(Target::Path(PathBuf::from(p)))
    }

    #[test]
    fn a_url_is_a_link_kept_whole() {
        let url = "https://example.com/a/very/long/path?q=1#top";
        assert_eq!(path(url, &[]), Some(Target::Link(url.into())));
        assert_eq!(
            path("mailto:a@b.co", &[]),
            Some(Target::Link("mailto:a@b.co".into()))
        );
    }

    #[test]
    fn a_relative_file_resolves_against_the_session_folder() {
        assert_eq!(
            path(
                "scripts/report-role-grants.ts",
                &["/work/app/scripts/report-role-grants.ts"]
            ),
            at("/work/app/scripts/report-role-grants.ts")
        );
    }

    #[test]
    fn a_relative_folder_resolves_like_a_file() {
        assert_eq!(
            path(".next/dev/types", &["/work/app/.next/dev/types"]),
            at("/work/app/.next/dev/types")
        );
        assert_eq!(
            path("./.next/dev", &["/work/app/.next/dev"]),
            at("/work/app/.next/dev")
        );
    }

    #[test]
    fn an_absolute_path_stays_and_home_expands() {
        assert_eq!(path("/etc/hosts", &["/etc/hosts"]), at("/etc/hosts"));
        assert_eq!(path("~/notes/a.md", &[]), at("/Users/me/notes/a.md"));
        assert_eq!(
            path("file:///tmp/my%20file.txt", &["/tmp/my file.txt"]),
            at("/tmp/my file.txt")
        );
    }

    #[test]
    fn a_line_suffix_goes_when_the_file_exists_without_it() {
        assert_eq!(
            path("src/a.rs:12:3", &["/work/app/src/a.rs"]),
            at("/work/app/src/a.rs")
        );
        assert_eq!(
            path("src/a.rs#L10-L20", &["/work/app/src/a.rs"]),
            at("/work/app/src/a.rs")
        );
        assert_eq!(path("src/gone.rs:4", &[]), at("/work/app/src/gone.rs:4"));
    }

    #[test]
    fn a_path_that_exists_with_a_percent_in_its_name_is_not_decoded() {
        assert_eq!(
            path("/tmp/100%20real.png", &["/tmp/100%20real.png"]),
            at("/tmp/100%20real.png")
        );
        assert_eq!(
            path("/tmp/100%20real.png", &["/tmp/100 real.png"]),
            at("/tmp/100 real.png")
        );
    }

    #[test]
    fn a_page_anchor_has_no_target() {
        assert_eq!(path("#section", &[]), None);
    }

    #[test]
    fn a_relative_path_without_a_folder_has_no_target() {
        assert_eq!(resolve("a/b.rs", "", None, |_| true), None);
        assert_eq!(resolve("  ", FOLDER, None, |_| true), None);
    }

    #[test]
    fn the_link_under_an_index_is_found_by_range() {
        let links = vec![(2..6, "a".to_owned()), (8..12, "b".to_owned())];
        assert_eq!(link_at(&links, 2), Some("a"));
        assert_eq!(link_at(&links, 5), Some("a"));
        assert_eq!(link_at(&links, 6), None);
        assert_eq!(link_at(&links, 9), Some("b"));
        assert_eq!(link_at(&links, 0), None);
    }
}
