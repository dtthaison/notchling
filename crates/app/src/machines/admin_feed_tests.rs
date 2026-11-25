use notchling_engine::machines::admin_secret::AdminSecrets;
use notchling_local_api::{ApiError, ClientError, ErrorCode};
use serde_json::json;

use super::*;

const VALUE: &str = "test-vault-password";

#[test]
fn the_secret_is_read_from_the_reply_and_an_empty_or_missing_one_is_none() {
    assert_eq!(
        secret_in(&json!({"secret": VALUE})).unwrap().expose(),
        VALUE
    );
    assert!(secret_in(&json!({"secret": ""})).is_none());
    assert!(secret_in(&json!({})).is_none());
    assert!(secret_in(&json!({"secret": 3})).is_none());
}

#[test]
fn an_error_says_why_and_never_carries_a_value() {
    let api = ClientError::Api(ApiError::new(
        ErrorCode::Failed,
        "m1-admin isn't in the Vault.",
    ));
    assert_eq!(failure_text(&api), "m1-admin isn't in the Vault.");
    assert_eq!(
        failure_text(&ClientError::Protocol(VALUE.into())),
        "the daemon sent something unexpected"
    );
    assert!(!failure_text(&ClientError::Timeout).contains(VALUE));
    assert_eq!(failure_text(&ClientError::Closed), NOT_RUNNING);
}

#[test]
fn with_no_daemon_the_read_fails_with_the_not_running_text() {
    let socket = std::env::temp_dir().join(format!("nl-no-admin-{}.sock", std::process::id()));
    let source = DaemonAdminSecrets::new(socket);
    assert_eq!(source.read("m1-admin", "m1").unwrap_err(), NOT_RUNNING);
}
