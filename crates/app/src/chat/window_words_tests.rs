use std::path::Path;

use notchling_locale as t;

use super::data::Tool;
use super::file_panel::{subtitle as file_subtitle, Body};
use super::markers;
use super::page_panel::{subtitle as page_subtitle, ArtifactLink, Opens};
use super::terminal::Tabs;
use crate::sheets::new_session::{draft_session, trust_text, Section};

#[test]
fn a_tool_has_a_name_to_match_and_a_title_to_show() {
    assert_eq!(Tool::Claude.name(), markers::TOOL_CLAUDE_CODE);
    assert_eq!(Tool::Codex.name(), markers::TOOL_CODEX);
    assert_eq!(Tool::Antigravity.name(), markers::TOOL_ANTIGRAVITY);
    assert_eq!(Tool::Claude.title(), "Claude Code");
    assert_eq!(Tool::Codex.title(), "Codex");
    assert_eq!(Tool::Antigravity.title(), "Antigravity");
    assert_eq!(Tool::Claude.title(), t::chat::tool_claude_code());
    assert_eq!(Section::Claude.title(), "Claude Code");
    assert_eq!(Section::Antigravity.title(), "Antigravity");
}

#[test]
fn the_panels_subtitles_read_as_they_did() {
    let link = ArtifactLink {
        id: "abc".into(),
        code: false,
    };
    assert_eq!(page_subtitle(&Opens::Live(link)), "Live on claude.ai");
    let cut = Body::Text {
        text: "x".into(),
        cut: true,
    };
    assert_eq!(
        file_subtitle(Path::new("/a/b/c.txt"), None, &cut),
        "/a/b · first 1 MB shown"
    );
    let whole = Body::Text {
        text: "x".into(),
        cut: false,
    };
    assert_eq!(file_subtitle(Path::new("/a/b/c.txt"), None, &whole), "/a/b");
}

#[test]
fn a_terminal_tab_is_named_by_its_number() {
    let mut tabs: Tabs<u8> = Tabs::default();
    tabs.add(1);
    tabs.add(2);
    let names: Vec<String> = tabs.iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["Terminal 1", "Terminal 2"]);
    assert_eq!(t::chat::terminal_tab("3"), "Terminal 3");
}

#[test]
fn the_new_session_sheets_words_read_as_they_did() {
    let session = draft_session("codex", "/work/proj", 5).expect("a draft for Codex");
    assert_eq!(session.agent.name, "New Codex Session");
    assert_eq!(session.agent.label, "New");
    assert_eq!(
        trust_text("proj"),
        "Claude Code hasn't trusted proj yet. Trust Folder and Start accepts Claude Code's “Do you trust this folder?” prompt for it, as running claude there would, then starts the session."
    );
    assert_eq!(t::chat::sheet_switches("Codex"), "Switches Codex");
    assert_eq!(
        t::chat::sheet_chosen("a@b.c", "In use", "proj"),
        "a@b.c (In use) in proj"
    );
    assert_eq!(t::chat::sheet_trust_title(), "Trust This Folder?");
    assert_eq!(t::chat::sheet_pick(), "Pick an account and a folder.");
    assert_eq!(
        t::chat::sheet_untrusted(),
        "Claude Code hasn't trusted this folder yet."
    );
}

#[test]
fn the_window_and_simulator_words_read_as_they_did() {
    assert_eq!(t::chat::window_no_session(), "No session selected");
    assert_eq!(t::chat::window_hide_details(), "Hide the session details");
    assert_eq!(
        t::chat::window_model_note("Opus 5.5"),
        "Window as Opus 5.5 until Notchling knows this model"
    );
    assert_eq!(
        t::chat::header_host("Claude Code", "m1"),
        "Claude Code · m1"
    );
    assert_eq!(t::chat::sim_shut_down_device("iPhone"), "Shut Down iPhone");
    assert_eq!(t::chat::sim_booting("iPhone"), "Booting iPhone…");
    assert_eq!(
        t::chat::sim_adding_photos("iPhone"),
        "Adding to iPhone's Photos…"
    );
    assert_eq!(t::chat::sim_added_many("3"), "Added 3 to Photos");
    assert_eq!(
        t::chat::sim_screenshot_failed("no"),
        "Couldn't take the screenshot: no"
    );
    assert_eq!(
        t::chat::terminal_start_failed("zsh", "gone"),
        "Couldn't start zsh: gone"
    );
    assert_eq!(t::chat::panel_artifact(), "Artifact");
    assert_eq!(t::chat::panel_back_to_details(), "Back to Details");
}

#[test]
fn the_tool_names_matched_against_the_daemon_stay_literal() {
    assert_eq!(markers::TOOL_CLAUDE_CODE, "Claude Code");
    assert_eq!(markers::TOOL_CODEX, "Codex");
    assert_eq!(markers::TOOL_ANTIGRAVITY, "Antigravity");
}
