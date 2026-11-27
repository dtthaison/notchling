use crate::acting_ops_tests::app;
use crate::catalog_ops::CatalogService;
use crate::limits_ops_tests::TempDir;
use crate::op_group::OpGroup;
use notchling_engine::accounts::fake::PanicHttp;
use notchling_engine::catalog::live_cache::LiveCache;
use notchling_engine::catalog::Catalog;
use notchling_engine::tools::agy::ConfigOption;
use notchling_local_api::{ops, OpSpec};
use serde_json::{json, Value};
use std::sync::Arc;

const CODEX: &str = r#"{"models":[{"slug":"gpt-6-astra","visibility":"list","priority":1}]}"#;

fn service(tmp: &TempDir) -> CatalogService {
    CatalogService::new(
        Catalog::built_in(),
        LiveCache::new(tmp.path("state/live-models.json")),
        tmp.path("codex"),
        None,
        Arc::new(PanicHttp),
    )
    .with_override_file(tmp.path("state/catalog.json"))
}

fn roles(service: &CatalogService) -> Value {
    service
        .call(&app(), ops::CATALOG_ROLES, &json!({}))
        .unwrap()
}

fn role<'a>(reply: &'a Value, name: &str) -> &'a Value {
    reply["roles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["role"] == name)
        .unwrap()
}

#[test]
fn catalog_roles_is_an_agent_read() {
    let tmp = TempDir::new("roles-spec");
    assert_eq!(
        service(&tmp).spec(ops::CATALOG_ROLES),
        Some(OpSpec::AGENT_READ)
    );
}

#[test]
fn it_answers_from_the_data_file_then_the_live_lists() {
    let tmp = TempDir::new("roles-live");
    tmp.write("codex/models_cache.json", CODEX);
    let service = service(&tmp);
    let reply = roles(&service);
    let review = &role(&reply, "review")["choices"][0];
    assert_eq!(review["model"], "gemini-4-hydrogen");
    assert_eq!(
        review["checked"], false,
        "Antigravity's list is not known yet"
    );
    let last = &role(&reply, "review-final")["choices"][0];
    assert_eq!(last["tool"], "codex");
    assert_eq!(last["checked"], true);
    assert_eq!(last["source"], "built_in");

    let options = ConfigOption::parse(Some(&json!([
        {"id": "model", "name": "Model", "type": "select", "currentValue": "gemini-4.1-hydrogen",
         "options": [{"value": "gemini-4.1-hydrogen", "name": "H"}]},
    ])));
    service.record_agy_options(&options);
    let reply = roles(&service);
    let review = &role(&reply, "review")["choices"][0];
    assert_eq!(review["model"], "gemini-4.1-hydrogen");
    assert_eq!(review["source"], "live");
    assert!(review["note"].as_str().unwrap().contains("is gone"));
}

#[test]
fn the_override_file_is_read_on_every_call_and_reported() {
    let tmp = TempDir::new("roles-override");
    let service = service(&tmp);
    let reply = roles(&service);
    assert_eq!(reply["override"]["state"], "missing");
    assert_eq!(
        reply["override"]["path"],
        tmp.path("state/catalog.json").to_string_lossy().as_ref()
    );
    tmp.write(
        "state/catalog.json",
        r#"{"antigravity": {"picks": [
            {"model": "gemini-4-argon", "effort": "xhigh", "roles": ["review"]}]}}"#,
    );
    let reply = roles(&service);
    assert_eq!(reply["override"]["state"], "found");
    let review = role(&reply, "review")["choices"].as_array().unwrap();
    assert_eq!(review.len(), 2);
    let mine = review
        .iter()
        .find(|c| c["marked"] == "gemini-4-argon")
        .unwrap();
    assert_eq!(mine["source"], "override");
    assert_eq!(mine["effort"], "xhigh");
    tmp.write("state/catalog.json", "{ nope");
    assert_eq!(roles(&service)["override"]["state"], "unreadable");
}
