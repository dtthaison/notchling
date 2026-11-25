use serde_json::json;

use super::*;

const STANDARD: &str = "granite-97m-r2@835ad14/p1";
const HELPER: &str = "static-mrl@b68f412/512i8/p1";

fn status(models: &[&str], fetch: Value, state: &str) -> Value {
    json!({
        "state": state,
        "models": models.iter().map(|id| json!({"id": id, "present": true})).collect::<Vec<_>>(),
        "fetch": fetch,
        "corpora": [{"corpus": "sessions", "docs": 3, "chunks": 15000, "embedded": {"a": 1240}}],
    })
}

fn titles(model: &SemanticModel) -> Vec<&'static str> {
    lines(model).iter().map(|l| l.title).collect()
}

#[test]
fn off_shows_only_the_switch() {
    let shown = model(false, false, &status(&[STANDARD], json!([]), "off"));
    assert!(!shown.on);
    assert!(shown.rows.is_empty() && shown.indexing.is_none());
    assert!(lines(&shown).is_empty());
    assert_eq!(change(&shown, ""), Some((KEY, json!(true))));
}

#[test]
fn on_lists_models_by_purpose() {
    let shown = model(true, false, &status(&[HELPER], json!([]), "no_model"));
    assert_eq!(titles(&shown), ["Standard", "Accent Helper"]);
    let all = lines(&shown);
    assert!(all.iter().all(|l| l.recommended));
    assert!(all[0].about.starts_with("Good for English and Vietnamese."));
    assert!(all[1]
        .about
        .starts_with("Finds Vietnamese typed without accents."));
    assert_eq!(all[0].button, ("download", "Download"));
    assert_eq!(all[1].button, ("remove", "Remove Model"));
    assert_eq!(all[1].status.as_deref(), Some("On this Mac"));
    assert_eq!(change(&shown, ""), Some((KEY, json!(false))));
}

#[test]
fn large_is_hidden_unless_listed() {
    let shown = model(true, false, &Value::Null);
    assert!(shown.rows.iter().all(|(entry, _)| entry.listed));
    assert!(!titles(&shown).contains(&"Large"));
    assert!(shown.rows.iter().all(|(_, row)| *row == ModelRow::Absent));
}

#[test]
fn progress_and_cancel_while_downloading() {
    let fetch = json!([{"model": STANDARD, "state": "downloading", "bytes": 110_000_000u64, "total": 220_000_000u64}]);
    let shown = model(true, false, &status(&[], fetch, "no_model"));
    let first = &lines(&shown)[0];
    assert_eq!(first.status.as_deref(), Some("Downloading 110 of 220 MB"));
    assert_eq!(first.fraction, Some(0.5));
    assert_eq!(first.button, ("cancel", "Cancel"));
    assert_eq!(
        action(&shown, &format!("cancel:{STANDARD}")),
        Some(Action::Cancel(STANDARD.to_owned()))
    );
}

#[test]
fn a_failed_download_can_be_tried_again() {
    let fetch = json!([{"model": HELPER, "state": "failed", "bytes": 0, "total": 5, "error": "No network."}]);
    let shown = model(true, false, &status(&[], fetch, "no_model"));
    let helper = &lines(&shown)[1];
    assert_eq!(
        helper.status.as_deref(),
        Some("The download failed. No network.")
    );
    assert_eq!(helper.button, ("download", "Download"));
}

#[test]
fn indexing_text_counts() {
    let shown = model(true, false, &status(&[STANDARD], json!([]), "indexing"));
    assert_eq!(shown.indexing, Some((1240, 15000)));
    assert_eq!(
        indexing_text(&shown).as_deref(),
        Some("Indexing 1,240 of 15,000 passages")
    );
    assert_eq!(pause_button(&shown), ("pause", "Pause"));
    assert_eq!(change(&shown, "pause"), Some((PAUSED_KEY, json!(true))));
    let ready = model(true, true, &status(&[STANDARD], json!([]), "paused"));
    assert!(indexing_text(&ready).is_none());
    assert_eq!(pause_button(&ready), ("resume", "Resume"));
    assert_eq!(change(&ready, "resume"), Some((PAUSED_KEY, json!(false))));
}

#[test]
fn actions_only_name_listed_models() {
    let shown = model(true, false, &Value::Null);
    assert_eq!(action(&shown, "download:nope"), None);
    assert_eq!(action(&shown, "wipe:granite-97m-r2@835ad14/p1"), None);
    assert_eq!(
        action(&shown, &format!("download:{HELPER}")),
        Some(Action::Download(HELPER.to_owned()))
    );
    assert_eq!(
        action(&shown, &format!("remove:{STANDARD}")),
        Some(Action::Remove(STANDARD.to_owned()))
    );
}

#[test]
fn the_feed_merge_follows_progress_ready_and_cancel() {
    use crate::search::embed_feed::merge_changed;
    let mut now = status(&[], json!([]), "no_model");
    let step = |state: &str| json!({"state": "no_model", "fetch": {"model": HELPER, "state": state, "bytes": 1, "total": 2}});
    merge_changed(&mut now, &step("downloading"));
    assert_eq!(
        model(true, false, &now).rows[1].1,
        ModelRow::Downloading { bytes: 1, total: 2 }
    );
    merge_changed(&mut now, &step("ready"));
    assert_eq!(model(true, false, &now).rows[1].1, ModelRow::Ready);
    merge_changed(&mut now, &step("cancelled"));
    assert_eq!(model(true, false, &now).rows[1].1, ModelRow::Absent);
    merge_changed(&mut now, &json!({"state": "ready", "fetch": null}));
    assert_eq!(now["state"], "ready");
}

#[test]
fn the_about_line_names_only_what_is_searched() {
    let about = t::settings::semantic_search_about();
    assert!(about.starts_with("Finds sessions and pages by what they mean."));
    assert!(!about.contains("settings"));
}

#[test]
fn each_action_asks_the_daemon_and_writes_nothing() {
    let get = step(&Action::Download(HELPER.to_owned()));
    assert_eq!(
        get,
        Step {
            op: "embed.fetch",
            model: HELPER.to_owned(),
            remove: false
        }
    );
    let stop = step(&Action::Cancel(HELPER.to_owned()));
    assert_eq!((stop.op, stop.remove), ("embed.fetch.cancel", false));
    let drop = step(&Action::Remove(HELPER.to_owned()));
    assert_eq!((drop.op, drop.remove), ("embed.remove", true));
    assert_eq!(drop.model, HELPER);
}

#[test]
fn the_rows_show_what_the_daemon_reports() {
    let fetch = json!([{"model": STANDARD, "state": "downloading", "bytes": 1, "total": 2}]);
    let reported = status(&[HELPER], fetch, "no_model");
    let shown = model(true, false, &reported);
    assert_eq!(
        shown.rows[0].1,
        ModelRow::Downloading { bytes: 1, total: 2 }
    );
    assert_eq!(shown.rows[1].1, ModelRow::Ready);
    let gone = model(true, false, &status(&[], json!([]), "no_model"));
    assert!(gone.rows.iter().all(|(_, row)| *row == ModelRow::Absent));
}
