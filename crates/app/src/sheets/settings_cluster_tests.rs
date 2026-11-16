use serde_json::json;

use super::settings_cluster::row;
use crate::cluster::model::ClusterModel;
use crate::cluster::renders::{empty, status, ME, STANDBY};

fn with(value: &str) -> ClusterModel {
    let mut status = status();
    status["failover"] = json!({"value": value, "armed": true, "target": STANDBY});
    ClusterModel::from_status(&status, ME)
}

#[test]
fn settings_row_only_in_a_cluster() {
    assert_eq!(row(&ClusterModel::from_status(&empty(), ME)), None);
    assert_eq!(row(&ClusterModel::default()), None);
    assert!(row(&ClusterModel::from_status(&status(), ME)).is_some());
}

#[test]
fn failover_is_not_available_in_settings() {
    let plain = row(&ClusterModel::from_status(&status(), ME)).expect("a row");
    assert_eq!(plain.title, "Automatic Failover");
    assert!(
        plain.line.starts_with("Not available yet."),
        "{}",
        plain.line
    );
    assert!(plain.line.contains("Move to <name> Now"), "{}", plain.line);
    for value in ["off", "30m", "1h"] {
        assert_eq!(row(&with(value)), Some(plain.clone()), "{value}");
    }
}
