use notchling_engine::organizations::COLORS;
use notchling_kit::ORG_COLORS;
use notchling_locale as t;
use serde_json::{json, Value};

use super::model::{
    colour_name, Counts, Load, Notes, Open, OrganizationsModel, Preview, MOVED_SHOWN,
};
use super::renders::{model, reply, Shown, HOME};

fn empty_acme() -> OrganizationsModel {
    let mut m = model(Shown::List);
    let mut r = reply(Shown::List);
    r["organizations"][2]["counts"] = json!({"accounts": 0, "secrets": 0, "machines": 0});
    m.finish(Ok(r));
    m
}

#[test]
fn the_list_reads_personal_first_with_counts_colours_and_folders() {
    let m = model(Shown::List);
    let names: Vec<&str> = m.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Personal", "AptusFIT", "Acme Studio"]);
    assert!(m.rows[0].personal && !m.rows[1].personal);
    assert_eq!(m.rows[1].color, "orange");
    assert_eq!(m.rows[1].initial(), "A");
    assert_eq!(
        m.rows[1].counts.line(),
        "2 accounts · 3 secrets · 1 machine"
    );
    assert_eq!(
        m.rows[0].counts.line(),
        "2 accounts · 3 secrets · 2 machines"
    );
    assert_eq!(m.tilde_line(&m.rows[1]), "Folders: ~/work/aptusfit");
    assert_eq!(m.tilde_line(&m.rows[0]), t::organizations::folders_none());
    assert!(!m.only_personal());
    assert_eq!(m.colors, COLORS);
    assert_eq!(m.load, Load::Idle);
}

#[test]
fn with_only_personal_the_page_knows_it() {
    let m = model(Shown::One);
    assert_eq!(m.rows.len(), 1);
    assert!(m.only_personal());
    assert!(m.editable());
}

#[test]
fn an_unreadable_store_blocks_editing_and_says_why() {
    let mut m = model(Shown::One);
    let mut r = reply(Shown::One);
    r["unreadable"] = json!("version 9 is not supported");
    m.finish(Ok(r));
    assert!(!m.editable());
    assert_eq!(m.unreadable.as_deref(), Some("version 9 is not supported"));
}

#[test]
fn a_failed_read_keeps_the_rows_already_shown_and_reads_again_later() {
    let mut m = model(Shown::List);
    assert!(m.start());
    m.finish(Err("daemon busy".into()));
    assert_eq!(m.load, Load::Failed("daemon busy".into()));
    assert_eq!(m.rows.len(), 3);
    assert!(m.start());
}

#[test]
fn delete_is_refused_for_personal_with_a_plain_reason_and_nothing_opens() {
    let mut m = model(Shown::List);
    m.ask_delete("personal");
    assert_eq!(m.open, Open::Nothing);
    let notice = m.notice.clone().unwrap();
    assert_eq!(notice.row.as_deref(), Some("personal"));
    assert_eq!(notice.text, t::organizations::error_personal());
    assert_eq!(
        notice.text,
        "Personal can't be deleted. It is where everything starts."
    );
}

#[test]
fn delete_is_refused_while_the_organization_holds_things_and_names_them() {
    let mut m = model(Shown::List);
    m.ask_delete("aptusfit");
    assert_eq!(m.open, Open::Nothing);
    assert_eq!(
        m.notice.unwrap().text,
        "AptusFIT still has 2 accounts, 3 secrets, 1 machine. Move them to another organization first."
    );
}

#[test]
fn an_empty_organization_asks_before_it_goes() {
    let mut m = empty_acme();
    m.ask_delete("acme-studio");
    assert_eq!(m.open, Open::ConfirmDelete("acme-studio".into()));
    assert_eq!(m.notice, None);
}

#[test]
fn counts_list_only_what_is_there() {
    let c = Counts {
        accounts: 1,
        secrets: 0,
        machines: 2,
    };
    assert_eq!(c.things(), "1 account, 2 machines");
    assert!(Counts::default().is_empty());
}

#[test]
fn the_folder_editor_adds_once_removes_and_saves_only_a_change_that_checked_out() {
    let mut m = model(Shown::List);
    m.begin_folders("acme-studio");
    assert!(!m.can_save_folders());
    assert!(!m.add_folder("   "));
    assert!(!m.add_folder("~/work/acme"));
    assert!(m.add_folder(" ~/work/other "));
    assert!(!m.add_folder("~/work/other"));
    assert_eq!(
        m.drafts().unwrap(),
        ["~/work/acme", "~/clients/acme-site", "~/work/other"]
    );
    assert!(!m.can_save_folders(), "still checking");
    let drafts = m.drafts().unwrap().to_vec();
    m.finish_preview(
        "acme-studio",
        &drafts,
        Ok(json!({"folders": drafts, "problem": null, "overlaps": [], "moved": []})),
    );
    assert!(m.can_save_folders());
    assert!(m.remove_folder(2));
    assert!(!m.remove_folder(9));
    assert!(!m.can_save_folders());
    let drafts = m.drafts().unwrap().to_vec();
    m.finish_preview(
        "acme-studio",
        &drafts,
        Ok(json!({"folders": drafts, "problem": null, "overlaps": [], "moved": []})),
    );
    assert!(!m.folders_changed());
    assert!(!m.can_save_folders(), "nothing changed from what is saved");
}

#[test]
fn an_answer_for_an_older_list_is_dropped() {
    let mut m = model(Shown::List);
    m.begin_folders("acme-studio");
    assert!(m.add_folder("~/a"));
    let old = m.drafts().unwrap().to_vec();
    assert!(m.add_folder("~/b"));
    m.finish_preview(
        "acme-studio",
        &old,
        Ok(json!({"folders": old, "problem": null, "overlaps": [], "moved": []})),
    );
    let Open::Folders(edit) = &m.open else {
        panic!("editor closed");
    };
    assert_eq!(edit.preview, Preview::Checking);
    assert_eq!(edit.drafts.len(), 4);
}

#[test]
fn the_preview_tells_overlaps_and_open_sessions_in_words() {
    let m = model(Shown::Folders);
    let Open::Folders(edit) = &m.open else {
        panic!("no editor");
    };
    let Preview::Ready(Notes {
        overlaps,
        moved,
        head,
        more,
    }) = &edit.preview
    else {
        panic!("not ready");
    };
    assert_eq!(
        overlaps,
        &["~/work/aptusfit/vendor/acme-sdk is inside ~/work/aptusfit, which belongs to AptusFIT. Under ~/work/aptusfit/vendor/acme-sdk the longer folder wins."]
    );
    assert_eq!(head, "2 open sessions would change organization:");
    assert_eq!(
        moved[0],
        "~/work/aptusfit/vendor/acme-sdk: AptusFIT to Acme Studio"
    );
    assert_eq!(more, &None);
    assert!(m.can_save_folders());
}

#[test]
fn a_long_list_of_moved_sessions_is_cut_and_counted() {
    let m = model(Shown::List);
    let rows: Vec<Value> = (0..MOVED_SHOWN + 3)
        .map(|i| json!({"folder": format!("{HOME}/x/{i}"), "from": "personal", "to": "aptusfit"}))
        .collect();
    let Preview::Ready(notes) = m.parse_preview(&json!({"overlaps": [], "moved": rows})) else {
        panic!("not ready");
    };
    assert_eq!(notes.moved.len(), MOVED_SHOWN);
    assert_eq!(notes.more.as_deref(), Some("and 3 more"));
    assert_eq!(notes.moved[0], "~/x/0: Personal to AptusFIT");
}

#[test]
fn no_moved_session_says_so() {
    let m = model(Shown::List);
    let Preview::Ready(notes) = m.parse_preview(&json!({"overlaps": [], "moved": []})) else {
        panic!("not ready");
    };
    assert_eq!(notes.head, t::organizations::folders_moved_none());
    assert!(notes.moved.is_empty());
}

#[test]
fn a_contained_folder_reads_as_holding_the_other() {
    let m = model(Shown::List);
    let Preview::Ready(notes) = m.parse_preview(&json!({"overlaps": [
        {"folder": "~/work", "organization": "aptusfit", "other": "~/work/aptusfit", "relation": "contains"}
    ], "moved": []})) else {
        panic!("not ready");
    };
    assert_eq!(
        notes.overlaps[0],
        "~/work holds ~/work/aptusfit, which belongs to AptusFIT. Sessions under ~/work/aptusfit stay with AptusFIT."
    );
}

#[test]
fn a_refused_preview_blocks_saving_and_says_why() {
    let mut m = model(Shown::List);
    m.begin_folders("acme-studio");
    assert!(m.add_folder("~/work/aptusfit"));
    let drafts = m.drafts().unwrap().to_vec();
    m.finish_preview(
        "acme-studio",
        &drafts,
        Ok(json!({"folders": [], "problem": {"kind": "folder_taken", "folder": "~/work/aptusfit", "by": "aptusfit"}})),
    );
    let Open::Folders(edit) = &m.open else {
        panic!("no editor");
    };
    assert_eq!(
        edit.preview,
        Preview::Refused("~/work/aptusfit already belongs to AptusFIT.".into())
    );
    assert!(!m.can_save_folders());
    assert_eq!(edit.drafts, drafts, "what was typed stays");
}

#[test]
fn the_normalized_folders_replace_what_was_typed() {
    let mut m = model(Shown::List);
    m.begin_folders("aptusfit");
    assert!(m.add_folder("~/work/./x/"));
    let drafts = m.drafts().unwrap().to_vec();
    m.finish_preview(
        "aptusfit",
        &drafts,
        Ok(json!({"folders": ["~/work/aptusfit", "~/work/x"], "problem": null, "overlaps": [], "moved": []})),
    );
    assert_eq!(m.drafts().unwrap(), ["~/work/aptusfit", "~/work/x"]);
}

#[test]
fn every_problem_the_daemon_names_has_a_plain_sentence() {
    let m = model(Shown::List);
    let say = |p: Value| m.problem_text(&p, "AptusFIT");
    assert_eq!(
        say(json!({"kind": "name_empty"})),
        "Give the organization a name."
    );
    assert_eq!(
        say(json!({"kind": "name_too_long"})),
        "Names are 40 characters at most."
    );
    assert_eq!(
        say(json!({"kind": "name_taken", "name": "Acme"})),
        "Another organization is already called Acme."
    );
    assert_eq!(
        say(json!({"kind": "color_invalid", "color": "mauve"})),
        "mauve isn't one of the colours."
    );
    assert_eq!(
        say(json!({"kind": "folder_invalid", "folder": "work"})),
        "work isn't a full folder. Start it with / or ~/."
    );
    assert_eq!(
        say(json!({"kind": "not_empty", "counts": {"accounts": 1, "secrets": 1, "machines": 0}})),
        "AptusFIT still has 1 account, 1 secret. Move them to another organization first."
    );
    assert_eq!(
        say(json!({"kind": "personal_not_deletable"})),
        t::organizations::error_personal()
    );
    assert_eq!(
        say(json!({"kind": "id_unknown", "id": "x"})),
        t::organizations::error_gone()
    );
    assert_eq!(
        m.failure_text("disk full", None, ""),
        "The change wasn't made: disk full"
    );
    assert_eq!(
        m.failure_text(
            "Organizations can't be read",
            Some(&json!({"kind": "unreadable"})),
            ""
        ),
        "Organizations can't be read, so nothing here can be changed. Organizations can't be read"
    );
    assert_eq!(
        m.failure_text(
            "Organizations can't be read, so nothing was changed (version 9)",
            Some(&json!({"kind": "unreadable", "why": "version 9"})),
            ""
        ),
        "Organizations can't be read, so nothing here can be changed. version 9"
    );
}

#[test]
fn a_failed_edit_shows_under_its_row_and_keeps_the_form_open() {
    let mut m = model(Shown::List);
    m.begin_rename("acme-studio");
    m.fail(
        Some("acme-studio"),
        "x",
        Some(&json!({"problems": [{"kind": "name_taken", "name": "AptusFIT"}]})),
    );
    assert_eq!(m.open, Open::Rename("acme-studio".into()));
    let notice = m.notice.unwrap();
    assert_eq!(notice.row.as_deref(), Some("acme-studio"));
    assert_eq!(
        notice.text,
        "Another organization is already called AptusFIT."
    );
}

#[test]
fn a_row_that_is_gone_closes_what_was_open_on_it() {
    let mut m = model(Shown::Folders);
    m.finish(Ok(reply(Shown::One)));
    assert_eq!(m.open, Open::Nothing);
    let mut m = model(Shown::Refused);
    assert!(m.notice.is_some());
    m.finish(Ok(reply(Shown::One)));
    assert_eq!(m.notice, None);
}

#[test]
fn the_menu_opens_and_closes_on_the_same_row() {
    let mut m = model(Shown::List);
    m.toggle_menu("aptusfit");
    assert_eq!(m.open, Open::Menu("aptusfit".into()));
    m.toggle_menu("aptusfit");
    assert_eq!(m.open, Open::Nothing);
    m.toggle_menu("aptusfit");
    m.toggle_menu("acme-studio");
    assert_eq!(m.open, Open::Menu("acme-studio".into()));
}

#[test]
fn absolute_folders_under_home_are_shown_with_a_tilde() {
    let m = model(Shown::List);
    assert_eq!(m.tilde("/Users/me/work/x"), "~/work/x");
    assert_eq!(m.tilde("/Users/mel/work"), "/Users/mel/work");
    assert_eq!(m.tilde("/srv/x"), "/srv/x");
}

#[test]
fn the_kit_palette_and_the_engine_list_name_the_same_colours_in_the_same_order() {
    let kit: Vec<&str> = ORG_COLORS.iter().map(|c| c.name).collect();
    assert_eq!(kit, COLORS);
    for name in COLORS {
        assert_ne!(colour_name(name), name, "{name} has no word");
    }
    assert_eq!(colour_name("mauve"), "mauve");
}

#[test]
fn a_gone_organization_leaves_a_page_notice_that_a_refresh_keeps() {
    let mut m = model(Shown::List);
    m.fail(
        Some("acme-studio"),
        "x",
        Some(&json!({"problems": [{"kind": "id_unknown", "id": "acme-studio"}]})),
    );
    let notice = m.notice.clone().unwrap();
    assert!(notice.page);
    assert_eq!(notice.row, None);
    assert_eq!(notice.text, t::organizations::error_gone());
    assert!(m.start());
    m.finish(Ok(reply(Shown::One)));
    assert_eq!(m.notice, Some(notice));
}

#[test]
fn a_failed_first_read_offers_no_editing() {
    let mut m = OrganizationsModel::default();
    assert!(m.start());
    m.finish(Err("daemon busy".into()));
    assert!(!m.editable());
    assert!(m.start());
    m.finish(Ok(reply(Shown::One)));
    assert!(m.editable());
}

#[test]
fn a_read_asked_for_while_one_is_in_flight_drops_the_older_answer_and_reads_again() {
    let mut m = OrganizationsModel::default();
    assert!(m.start());
    assert!(!m.start());
    assert!(m.finish(Ok(reply(Shown::One))));
    assert!(m.rows.is_empty());
    assert!(m.start());
    assert!(!m.finish(Ok(reply(Shown::List))));
    assert_eq!(m.rows.len(), 3);
    assert!(!m.again);
}

#[test]
fn a_failed_delete_closes_the_confirm_and_keeps_the_error() {
    let mut m = model(Shown::List);
    m.open = Open::ConfirmDelete("acme-studio".into());
    m.fail(
        Some("acme-studio"),
        "x",
        Some(&json!({"problems": [{"kind": "not_empty", "counts": {"accounts": 1}}]})),
    );
    assert_eq!(m.open, Open::Nothing);
    assert_eq!(m.notice.unwrap().row.as_deref(), Some("acme-studio"));
}
