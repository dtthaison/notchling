use std::path::Path;

use super::file_panel::{kind, load, opens, Body, Kind};
use super::html_preview::{
    git_file, git_look, git_offers, is_html, page_subtitle, repo_root, source, HtmlView, Look, GONE,
};
use super::page_panel::Opens;
use crate::pages::local::PageSource;
use crate::testutil::Scratch;

#[test]
fn an_html_file_opens_as_a_page_and_other_text_stays_text() {
    let s = Scratch::new("html-opens");
    let html = s.write("site/index.html", "<h1>Hi</h1>");
    let htm = s.write("site/OLD.HTM", "<p>old</p>");
    let xhtml = s.write("site/doc.xhtml", "<html/>");
    let empty = s.write("site/blank.html", "");
    let txt = s.write("site/notes.txt", "<h1>not a page</h1>");
    let md = s.write("site/plan.md", "# Plan");
    let svg = s.write("site/logo.svg", "<svg/>");
    let rs = s.write("site/main.rs", "fn main() {}");
    for page in [&html, &htm, &xhtml, &empty] {
        assert_eq!(opens(page), Opens::Html(page.clone()), "{page:?}");
    }
    for plain in [&txt, &md, &svg, &rs] {
        assert_eq!(opens(plain), Opens::File(plain.clone()), "{plain:?}");
    }
    assert_eq!(
        opens(html.parent().unwrap()),
        Opens::Folder(html.parent().unwrap().to_path_buf())
    );
}

#[test]
fn a_file_named_html_that_is_not_text_goes_to_the_system() {
    let s = Scratch::new("html-binary");
    let fake = s.write("site/blob.html", "");
    std::fs::write(&fake, [0u8, 1, 2, 0, 255]).unwrap();
    assert_eq!(opens(&fake), Opens::System(fake.clone()));
    assert_eq!(kind(&fake, &[0, 1]), Kind::Other);
    assert_eq!(kind(Path::new("a/x.html"), b"<p>"), Kind::Html);
    assert_eq!(kind(Path::new("a/x.htm"), b"<p>"), Kind::Html);
    assert_eq!(kind(Path::new("a/x.html.txt"), b"<p>"), Kind::Text);
}

#[test]
fn an_html_files_source_loads_as_text() {
    let s = Scratch::new("html-load");
    let html = s.write("index.html", "<h1>Hi</h1>\n");
    assert_eq!(
        load(&html).body,
        Body::Text {
            text: "<h1>Hi</h1>\n".into(),
            cut: false
        }
    );
    let empty = s.write("empty.html", "");
    assert_eq!(
        load(&empty).body,
        Body::Text {
            text: String::new(),
            cut: false
        }
    );
}

#[test]
fn only_html_names_are_pages() {
    for name in ["a.html", "a.HTML", "a.htm", "a.xhtml", "dir.v2/a.html"] {
        assert!(is_html(Path::new(name)), "{name}");
    }
    for name in ["a.txt", "a.html.bak", "html", "a.svg", "a.md", "a", ""] {
        assert!(!is_html(Path::new(name)), "{name}");
    }
}

#[test]
fn a_page_is_served_from_its_own_folder_and_a_gone_file_says_so() {
    let s = Scratch::new("html-source");
    let html = s.write("site/index.html", "<p>hi</p>");
    assert_eq!(
        source(&html),
        Ok(PageSource::File {
            file: html.clone(),
            folder: s.path().join("site")
        })
    );
    assert_eq!(
        source(&s.path().join("site/none.html")),
        Err(GONE.to_owned())
    );
    assert_eq!(source(&s.path().join("site")), Err(GONE.to_owned()));
}

#[test]
fn the_view_notices_when_the_file_changes() {
    let s = Scratch::new("html-stamp");
    let html = s.write("index.html", "one");
    let mut view = HtmlView::file(html.clone(), None);
    assert!(view.is_page());
    assert!(!view.changes);
    assert!(!view.moved());
    std::fs::write(&html, "one and more").unwrap();
    assert!(view.moved());
    assert!(!view.moved());
    std::fs::remove_file(&html).unwrap();
    assert!(view.moved());
    assert_eq!(view.page_tip(), "Show as Page");
    assert_eq!(view.other_tip(), "Show Source");
    let git = HtmlView::git(html, Look::Source, None);
    assert!(!git.is_page());
    assert_eq!(git.other_tip(), "Show Changes");
}

#[test]
fn the_subtitle_says_the_folder_and_that_scripts_are_off() {
    let home = Path::new("/Users/me");
    assert_eq!(
        page_subtitle(Path::new("/Users/me/site/index.html"), Some(home)),
        "~/site \u{b7} Scripts off"
    );
    assert_eq!(
        page_subtitle(Path::new("/tmp/index.html"), None),
        "/tmp \u{b7} Scripts off"
    );
    let view = HtmlView::file(
        Path::new("/Users/me/site/index.html").to_path_buf(),
        Some(home),
    );
    assert_eq!(view.page_subtitle, "~/site \u{b7} Scripts off");
}

#[test]
fn a_git_row_shows_a_page_first_only_when_its_diff_is_the_whole_file() {
    assert_eq!(git_look("?"), Look::Page);
    assert_eq!(git_look("A"), Look::Page);
    for status in ["M", "R", "U", "D"] {
        assert_eq!(git_look(status), Look::Source, "{status}");
    }
    assert!(!git_offers("D"));
    for status in ["?", "A", "M", "R", "U"] {
        assert!(git_offers(status), "{status}");
    }
}

#[test]
fn a_git_path_is_found_from_the_repository_root_not_from_the_session_folder() {
    let s = Scratch::new("html-git");
    s.write("repo/.git/HEAD", "ref: refs/heads/main");
    let top = s.write("repo/docs/index.html", "<p>top</p>");
    let nested = s.write("repo/sub/docs/index.html", "<p>nested</p>");
    s.write("repo/sub/readme.txt", "x");
    let sub = s.path().join("repo/sub");
    assert_eq!(repo_root(&sub), Some(s.path().join("repo")));
    assert_eq!(git_file(&sub, "docs/index.html"), Some(top.clone()));
    assert_ne!(git_file(&sub, "docs/index.html"), Some(nested.clone()));
    assert_eq!(
        git_file(&s.path().join("repo"), "docs/index.html"),
        Some(top)
    );
    assert_eq!(git_file(&sub, "sub/docs/index.html"), Some(nested));
    assert_eq!(git_file(&sub, "docs/missing.html"), None);
    assert_eq!(git_file(&sub, "sub/readme.txt"), None, "not an html name");
}

#[test]
fn a_worktree_is_a_root_too_and_a_bad_git_path_finds_nothing() {
    let s = Scratch::new("html-git-worktree");
    s.write("wt/.git", "gitdir: /elsewhere");
    let page = s.write("wt/index.html", "<p>x</p>");
    s.write("outside.html", "<p>y</p>");
    let wt = s.path().join("wt");
    assert_eq!(git_file(&wt, "index.html"), Some(page));
    for bad in [
        "../outside.html",
        "/etc/hosts.html",
        "",
        "a/../index.html",
        ".",
    ] {
        assert_eq!(git_file(&wt, bad), None, "{bad:?}");
    }
}
