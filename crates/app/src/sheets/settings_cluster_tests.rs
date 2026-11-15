use notchling_engine::cluster::failover::FENCE_LEAD_MS;
use notchling_locale::lookup;
use serde_json::json;

use super::settings::{choice_key, title_key};
use super::settings_cluster::{row, KEY};
use crate::cluster::model::ClusterModel;
use crate::cluster::renders::{empty, status, ME, STANDBY};
use notchling_local_api::ops;

fn with(value: &str, armed: bool, target: Option<&str>) -> ClusterModel {
    let mut status = status();
    status["failover"] = json!({"value": value, "armed": armed, "target": target});
    ClusterModel::from_status(&status, ME)
}

#[test]
fn settings_row_only_in_a_cluster_and_names_the_target() {
    assert_eq!(row(&ClusterModel::from_status(&empty(), ME)), None);
    assert_eq!(row(&ClusterModel::default()), None);
    assert_eq!(
        row(&ClusterModel::from_status(&status(), ME)),
        None,
        "an older daemon says nothing about failover"
    );
    let shown = row(&with("30m", true, Some(STANDBY))).expect("a row");
    assert_eq!(shown.title, "Automatic Failover");
    let labels: Vec<&str> = shown.options.iter().map(|(_, l)| *l).collect();
    assert_eq!(labels, ["Off", "After 30 Minutes", "After 1 Hour"]);
    let values: Vec<&str> = shown.options.iter().map(|(v, _)| *v).collect();
    assert_eq!(values, ["off", "30m", "1h"]);
    assert_eq!(shown.selected, "30m");
    assert_eq!(
        shown.lines,
        [
            "mac-mini takes over when MacBook can't be reached for this long. MacBook pauses those sessions 2 minutes before."
        ]
    );
    assert_eq!(shown.not_ready, None);
}

#[test]
fn settings_row_says_not_ready_until_first_grant() {
    let waiting = row(&with("1h", false, Some(STANDBY))).expect("a row");
    assert_eq!(waiting.not_ready, Some("Automatic failover: not ready"));
    let granted = row(&with("1h", true, Some(STANDBY))).expect("a row");
    assert_eq!(granted.not_ready, None);
    let off = row(&with("off", false, Some(STANDBY))).expect("a row");
    assert_eq!(off.not_ready, None);
    assert_eq!(off.lines, ["Off: no machine takes over by itself."]);
}

#[test]
fn with_no_standby_the_row_says_so_and_is_never_not_ready() {
    let none = row(&with("30m", false, None)).expect("a row");
    assert_eq!(none.not_ready, None);
    assert_eq!(none.lines.len(), 1);
    assert!(none.lines[0].starts_with("No Standby can take over yet."));
}

#[test]
fn choosing_sends_the_cluster_op_and_nothing_else() {
    let shown = row(&with("off", false, None)).expect("a row");
    let sent = shown.change("1h").expect("a request");
    assert_eq!(sent.op, ops::CLUSTER_FAILOVER_SET);
    assert_eq!(sent.args, json!({"value": "1h"}));
    assert_eq!(shown.change("2h"), None);
}

#[test]
fn the_words_come_from_messages_and_the_lead_is_two_minutes() {
    assert_eq!(FENCE_LEAD_MS, 120_000);
    let text = lookup(&title_key(KEY)).expect("the row title message");
    assert_eq!(text.english(), "Automatic Failover");
    for value in ["off", "30m", "1h"] {
        let found = lookup(&choice_key(KEY, value));
        assert!(found.is_some(), "no message for {value}");
    }
}
