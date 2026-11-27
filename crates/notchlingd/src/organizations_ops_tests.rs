use crate::layout::Layout;
use crate::limits_ops_tests::TempDir;
use crate::op_group::OpGroup;
use crate::organizations_ops::OrganizationsOps;
use notchling_engine::organizations::{Kind, Organizations};
use notchling_engine::store;
use notchling_local_api::{ops, ApiError, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const EDITS: [&str; 5] = [
    ops::ORGANIZATIONS_CREATE,
    ops::ORGANIZATIONS_RENAME,
    ops::ORGANIZATIONS_COLOR_SET,
    ops::ORGANIZATIONS_FOLDERS_SET,
    ops::ORGANIZATIONS_DELETE,
];

const READS: [&str; 2] = [ops::ORGANIZATIONS_LIST, ops::ORGANIZATIONS_FOLDERS_PREVIEW];

const CLAUDE: &str = r#"[{"id":"c1","email":"a@example.com","plan":"Max 5x","needsLogin":false}]"#;
const VAULT: &str = r#"[{"kind":"token","name":"github-read","note":"","secretVar":"GH","updated":"2026-11-01","url":"https://github.com","userVar":""}]"#;

fn app() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: None,
    }
}

fn err(r: Result<Value, ApiError>) -> ApiError {
    r.unwrap_err()
}

struct Rig {
    dir: TempDir,
    ops: OrganizationsOps,
    sessions: Arc<Mutex<Vec<PathBuf>>>,
}

fn rig(tag: &str) -> Rig {
    let dir = TempDir::new(tag);
    let layout = Layout::resolve(None, Some(dir.root().as_os_str()), || None).unwrap();
    let sessions = Arc::new(Mutex::new(Vec::new()));
    let shown = sessions.clone();
    let ops =
        OrganizationsOps::for_daemon(&layout, Arc::new(move || shown.lock().unwrap().clone()));
    Rig { dir, ops, sessions }
}

impl Rig {
    fn data(&self) -> PathBuf {
        self.dir.path("notchling")
    }

    fn call(&self, op: &str, args: Value) -> Result<Value, ApiError> {
        self.ops.call(&app(), op, &args)
    }

    fn with_things(&self) {
        store::put_doc(&self.data(), Kind::Claude.doc(), CLAUDE).unwrap();
        store::put_doc(&self.data(), Kind::Vault.doc(), VAULT).unwrap();
    }

    fn rows(&self) -> Vec<Value> {
        self.call(ops::ORGANIZATIONS_LIST, json!({})).unwrap()["organizations"]
            .as_array()
            .unwrap()
            .clone()
    }
}

#[test]
fn every_edit_is_app_tier_and_no_agent_can_make_change_or_remove_an_organization() {
    let rig = rig("orgs-tiers");
    for op in EDITS {
        let spec = rig.ops.spec(op).unwrap();
        assert_eq!(spec, OpSpec::APP, "{op}");
        assert_eq!(spec.tier, Tier::App, "{op}");
        assert!(!spec.quiet_ok && !spec.approval, "{op}");
    }
    for op in READS {
        let spec = rig.ops.spec(op).unwrap();
        assert_eq!(spec, OpSpec::APP_QUIET, "{op}");
        assert_eq!(spec.tier, Tier::App, "{op}");
    }
    assert_eq!(rig.ops.spec(ops::VAULT_LIST), None);
    assert_eq!(rig.ops.spec("organizations.move"), None);
}

#[test]
fn a_fresh_data_folder_lists_personal_alone_and_changes_nothing() {
    let rig = rig("orgs-fresh");
    let reply = rig.call(ops::ORGANIZATIONS_LIST, json!({})).unwrap();
    assert_eq!(reply["onlyPersonal"], true);
    assert_eq!(reply["unreadable"], Value::Null);
    let rows = reply["organizations"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "personal");
    assert_eq!(rows[0]["name"], "Personal");
    assert_eq!(rows[0]["personal"], true);
    assert_eq!(
        rows[0]["counts"],
        json!({"accounts": 0, "secrets": 0, "machines": 0})
    );
    assert_eq!(reply["colors"][0], "grey");
    assert_eq!(store::doc(&rig.data(), "organizations").unwrap(), None);
}

#[test]
fn creating_a_second_organization_lists_personal_first_with_what_each_holds() {
    let rig = rig("orgs-create");
    rig.with_things();
    let made = rig
        .call(
            ops::ORGANIZATIONS_CREATE,
            json!({"name": " AptusFIT ", "color": "orange"}),
        )
        .unwrap();
    assert_eq!(made["id"], "aptusfit");
    let orgs = Organizations::new(&rig.data());
    orgs.set_organization(Kind::Claude, "c1", "aptusfit")
        .unwrap();
    let reply = rig.call(ops::ORGANIZATIONS_LIST, json!({})).unwrap();
    assert_eq!(reply["onlyPersonal"], false);
    let rows = reply["organizations"].as_array().unwrap();
    assert_eq!(rows[0]["id"], "personal");
    assert_eq!(
        rows[0]["counts"],
        json!({"accounts": 0, "secrets": 1, "machines": 0})
    );
    assert_eq!(rows[1]["id"], "aptusfit");
    assert_eq!(rows[1]["color"], "orange");
    assert_eq!(
        rows[1]["counts"],
        json!({"accounts": 1, "secrets": 0, "machines": 0})
    );
}

#[test]
fn rename_and_colour_check_their_input_and_say_why_in_data() {
    let rig = rig("orgs-edit");
    rig.call(ops::ORGANIZATIONS_CREATE, json!({"name": "Acme"}))
        .unwrap();
    rig.call(
        ops::ORGANIZATIONS_RENAME,
        json!({"id": "acme", "name": "Acme Inc"}),
    )
    .unwrap();
    rig.call(
        ops::ORGANIZATIONS_COLOR_SET,
        json!({"id": "acme", "color": "teal"}),
    )
    .unwrap();
    let rows = rig.rows();
    assert_eq!(rows[1]["name"], "Acme Inc");
    assert_eq!(rows[1]["color"], "teal");

    let taken = err(rig.call(
        ops::ORGANIZATIONS_RENAME,
        json!({"id": "acme", "name": "personal"}),
    ));
    assert_eq!(taken.code, ErrorCode::BadRequest);
    assert_eq!(
        taken.data.unwrap()["problems"][0],
        json!({"kind": "name_taken", "name": "personal"})
    );
    let bad = err(rig.call(
        ops::ORGANIZATIONS_COLOR_SET,
        json!({"id": "acme", "color": "chartreuse"}),
    ));
    assert_eq!(bad.code, ErrorCode::BadRequest);
    assert_eq!(bad.data.unwrap()["problems"][0]["kind"], "color_invalid");
    let empty = err(rig.call(ops::ORGANIZATIONS_CREATE, json!({"name": "  "})));
    assert_eq!(empty.data.unwrap()["problems"][0]["kind"], "name_empty");
    let missing = err(rig.call(ops::ORGANIZATIONS_RENAME, json!({"id": "acme"})));
    assert_eq!(missing.code, ErrorCode::BadRequest);
    assert_eq!(rig.rows()[1]["name"], "Acme Inc");
}

#[test]
fn folders_are_set_normalized_and_a_folder_another_organization_has_is_refused() {
    let rig = rig("orgs-folders");
    rig.call(ops::ORGANIZATIONS_CREATE, json!({"name": "AptusFIT"}))
        .unwrap();
    rig.call(ops::ORGANIZATIONS_CREATE, json!({"name": "Acme"}))
        .unwrap();
    let kept = rig
        .call(
            ops::ORGANIZATIONS_FOLDERS_SET,
            json!({"id": "aptusfit", "folders": ["~/work/aptusfit/", "~/work/./aptusfit"]}),
        )
        .unwrap();
    assert_eq!(kept["folders"], json!(["~/work/aptusfit"]));
    let taken = err(rig.call(
        ops::ORGANIZATIONS_FOLDERS_SET,
        json!({"id": "acme", "folders": ["~/work/aptusfit"]}),
    ));
    assert_eq!(
        taken.data.unwrap()["problems"][0],
        json!({"kind": "folder_taken", "folder": "~/work/aptusfit", "by": "aptusfit"})
    );
    let relative = err(rig.call(
        ops::ORGANIZATIONS_FOLDERS_SET,
        json!({"id": "acme", "folders": ["work/acme"]}),
    ));
    assert_eq!(
        relative.data.unwrap()["problems"][0]["kind"],
        "folder_invalid"
    );
    let shape = err(rig.call(
        ops::ORGANIZATIONS_FOLDERS_SET,
        json!({"id": "acme", "folders": "x"}),
    ));
    assert_eq!(shape.code, ErrorCode::BadRequest);
    assert_eq!(rig.rows()[2]["folders"], json!([]));
}

#[test]
fn the_preview_names_overlaps_and_the_open_sessions_that_would_move_and_writes_nothing() {
    let rig = rig("orgs-preview");
    let home = rig.dir.root().to_path_buf();
    rig.call(ops::ORGANIZATIONS_CREATE, json!({"name": "AptusFIT"}))
        .unwrap();
    rig.call(ops::ORGANIZATIONS_CREATE, json!({"name": "Acme"}))
        .unwrap();
    rig.call(
        ops::ORGANIZATIONS_FOLDERS_SET,
        json!({"id": "aptusfit", "folders": ["~/work/aptusfit"]}),
    )
    .unwrap();
    *rig.sessions.lock().unwrap() = vec![
        home.join("work/aptusfit/vendor/acme/x"),
        home.join("work/aptusfit/fe"),
        home.join("elsewhere"),
    ];
    let before = store::doc(&rig.data(), "organizations").unwrap();
    let reply = rig
        .call(
            ops::ORGANIZATIONS_FOLDERS_PREVIEW,
            json!({"id": "acme", "folders": ["~/work/aptusfit/vendor/acme"]}),
        )
        .unwrap();
    assert_eq!(reply["problem"], Value::Null);
    assert_eq!(
        reply["overlaps"],
        json!([{"folder": "~/work/aptusfit/vendor/acme", "organization": "aptusfit",
                "other": "~/work/aptusfit", "relation": "inside"}])
    );
    assert_eq!(
        reply["moved"],
        json!([{"folder": home.join("work/aptusfit/vendor/acme/x").to_string_lossy(),
                "from": "aptusfit", "to": "acme"}])
    );
    assert_eq!(store::doc(&rig.data(), "organizations").unwrap(), before);

    let refused = rig
        .call(
            ops::ORGANIZATIONS_FOLDERS_PREVIEW,
            json!({"id": "acme", "folders": ["~/work/aptusfit"]}),
        )
        .unwrap();
    assert_eq!(refused["problem"]["kind"], "folder_taken");
    assert_eq!(refused["moved"], json!([]));
}

#[test]
fn personal_is_never_deleted_and_an_organization_with_things_in_it_says_what_is_left() {
    let rig = rig("orgs-delete");
    rig.with_things();
    rig.call(ops::ORGANIZATIONS_CREATE, json!({"name": "AptusFIT"}))
        .unwrap();
    let personal = err(rig.call(ops::ORGANIZATIONS_DELETE, json!({"id": "personal"})));
    assert_eq!(personal.code, ErrorCode::BadRequest);
    assert_eq!(personal.message, "Personal can't be deleted");
    assert_eq!(
        personal.data.unwrap()["problems"][0]["kind"],
        "personal_not_deletable"
    );

    let orgs = Organizations::new(&rig.data());
    orgs.set_organization(Kind::Claude, "c1", "aptusfit")
        .unwrap();
    orgs.set_organization(Kind::Vault, "github-read", "aptusfit")
        .unwrap();
    let held = err(rig.call(ops::ORGANIZATIONS_DELETE, json!({"id": "aptusfit"})));
    assert_eq!(held.code, ErrorCode::BadRequest);
    assert_eq!(
        held.data.unwrap()["problems"][0],
        json!({"kind": "not_empty", "counts": {"accounts": 1, "secrets": 1, "machines": 0}})
    );
    assert_eq!(rig.rows().len(), 2);

    orgs.set_organization(Kind::Claude, "c1", "personal")
        .unwrap();
    orgs.set_organization(Kind::Vault, "github-read", "personal")
        .unwrap();
    rig.call(ops::ORGANIZATIONS_DELETE, json!({"id": "aptusfit"}))
        .unwrap();
    assert_eq!(rig.rows().len(), 1);
    let gone = err(rig.call(ops::ORGANIZATIONS_DELETE, json!({"id": "aptusfit"})));
    assert_eq!(gone.data.unwrap()["problems"][0]["kind"], "id_unknown");
}

#[test]
fn a_machine_in_the_registry_keeps_its_organization_from_being_deleted() {
    let rig = rig("orgs-delete-machine");
    store::put_doc(
        &rig.data(),
        "machines/registry",
        r#"{"machines":[{"claudeSecret":"x","host":"h.tail","name":"m1","user":"u","organization":"acme"}],"version":3}"#,
    )
    .unwrap();
    rig.call(ops::ORGANIZATIONS_CREATE, json!({"name": "Acme"}))
        .unwrap();
    let rows = rig.rows();
    assert_eq!(rows[1]["counts"]["machines"], 1);
    let held = err(rig.call(ops::ORGANIZATIONS_DELETE, json!({"id": "acme"})));
    assert_eq!(held.data.unwrap()["problems"][0]["counts"]["machines"], 1);
}

#[test]
fn an_unreadable_document_is_reported_and_every_edit_is_refused_without_a_write() {
    let rig = rig("orgs-unreadable");
    store::put_doc(&rig.data(), "organizations", "{not json").unwrap();
    let reply = rig.call(ops::ORGANIZATIONS_LIST, json!({})).unwrap();
    assert!(reply["unreadable"].as_str().is_some());
    assert_eq!(reply["organizations"].as_array().unwrap().len(), 1);
    for (op, args) in [
        (ops::ORGANIZATIONS_CREATE, json!({"name": "Acme"})),
        (
            ops::ORGANIZATIONS_RENAME,
            json!({"id": "personal", "name": "Me"}),
        ),
        (
            ops::ORGANIZATIONS_COLOR_SET,
            json!({"id": "personal", "color": "red"}),
        ),
        (
            ops::ORGANIZATIONS_FOLDERS_SET,
            json!({"id": "personal", "folders": []}),
        ),
        (
            ops::ORGANIZATIONS_FOLDERS_PREVIEW,
            json!({"id": "personal", "folders": []}),
        ),
        (ops::ORGANIZATIONS_DELETE, json!({"id": "acme"})),
    ] {
        let e = err(rig.call(op, args));
        assert_eq!(e.code, ErrorCode::Failed, "{op}");
        assert_eq!(e.data.unwrap()["kind"], "unreadable", "{op}");
    }
    assert_eq!(
        store::doc(&rig.data(), "organizations").unwrap().as_deref(),
        Some("{not json")
    );
}

#[test]
fn an_unknown_op_has_no_handler() {
    let rig = rig("orgs-unknown");
    assert_eq!(
        err(rig.call("organizations.move", json!({}))).code,
        ErrorCode::Internal
    );
}
