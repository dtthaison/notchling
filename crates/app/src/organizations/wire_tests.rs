use std::sync::{Arc, Mutex};

use notchling_local_api::{ApiError, ClientError, ErrorCode};
use serde_json::{json, Value};

use super::wire;
use crate::actions::{Actions, Transport};

type Calls = Arc<Mutex<Vec<(String, Value)>>>;
type Answer = Box<dyn Fn(&str) -> Result<Value, ClientError> + Send + Sync>;

struct Fake {
    calls: Calls,
    answer: Answer,
}

impl Transport for Fake {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        self.calls.lock().unwrap().push((op.to_owned(), args));
        (self.answer)(op)
    }
}

fn fake(
    answer: impl Fn(&str) -> Result<Value, ClientError> + Send + Sync + 'static,
) -> (Actions, Calls) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let actions = Actions::over(Box::new(Fake {
        calls: calls.clone(),
        answer: Box::new(answer),
    }));
    (actions, calls)
}

#[test]
fn each_request_names_its_op_and_carries_only_what_the_daemon_reads() {
    let all = [
        (wire::list(), "organizations.list", json!({})),
        (
            wire::create("Acme", Some("blue")),
            "organizations.create",
            json!({"name": "Acme", "color": "blue"}),
        ),
        (
            wire::create("Acme", None),
            "organizations.create",
            json!({"name": "Acme"}),
        ),
        (
            wire::rename("acme", "Acme Inc"),
            "organizations.rename",
            json!({"id": "acme", "name": "Acme Inc"}),
        ),
        (
            wire::set_color("acme", "red"),
            "organizations.color.set",
            json!({"id": "acme", "color": "red"}),
        ),
        (
            wire::set_folders("acme", &["~/work/acme".into()]),
            "organizations.folders.set",
            json!({"id": "acme", "folders": ["~/work/acme"]}),
        ),
        (
            wire::preview_folders("acme", &[]),
            "organizations.folders.preview",
            json!({"id": "acme", "folders": []}),
        ),
        (
            wire::delete("acme"),
            "organizations.delete",
            json!({"id": "acme"}),
        ),
    ];
    for (request, op, args) in all {
        assert_eq!(request.op, op);
        assert_eq!(request.args, args, "{op}");
    }
}

#[test]
fn an_edit_goes_through_while_acting_is_off_because_it_is_not_a_session_action() {
    let (actions, calls) = fake(|_| Ok(json!({})));
    assert!(actions.read_full(&wire::rename("acme", "Acme Inc")).is_ok());
    assert_eq!(calls.lock().unwrap()[0].0, "organizations.rename");
}

#[test]
fn a_refusal_keeps_its_problems_so_the_words_come_from_the_app() {
    let (actions, _) = fake(|_| {
        Err(ClientError::Api(
            ApiError::new(ErrorCode::BadRequest, "Personal can't be deleted")
                .with_data(json!({"problems": [{"kind": "personal_not_deletable"}]})),
        ))
    });
    let failure = actions.read_full(&wire::delete("personal")).unwrap_err();
    assert_eq!(failure.message, "Personal can't be deleted");
    assert_eq!(
        failure.data.unwrap()["problems"][0]["kind"],
        "personal_not_deletable"
    );
}

#[test]
fn with_no_daemon_nothing_is_sent_and_the_page_says_so() {
    let actions = Actions::fixed(crate::acting::Acting::Detached);
    let failure = actions.read_full(&wire::list()).unwrap_err();
    assert_eq!(failure.message, crate::acting::DETACHED_MESSAGE);
}
