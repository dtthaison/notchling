use std::sync::{Arc, Mutex};

use notchling_local_api::ClientError;
use serde_json::{json, Value};

use super::skills_models::{load, reinstall};
use super::skills_rows::SkillsShown;
use crate::actions::{Actions, Transport};

#[derive(Default)]
struct Fake {
    seen: Mutex<Vec<(String, Value)>>,
}

impl Transport for Arc<Fake> {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        self.seen.lock().unwrap().push((op.to_owned(), args));
        Ok(match op {
            "install.plan" => json!({"on": true, "applied": false, "plan": {"skills": [
                {"name": "notchling-pages", "tool": "claude", "action": "unchanged", "reason": null}]}}),
            "catalog.roles" => json!({"roles": [], "override": {"path": null, "state": "missing"}}),
            _ => json!({"on": true, "applied": true, "plan": {"skills": []}}),
        })
    }
}

fn rig() -> (Actions, Arc<Fake>) {
    let fake = Arc::new(Fake::default());
    (Actions::over(Box::new(fake.clone())), fake)
}

#[test]
fn the_page_asks_for_the_plan_of_everything_and_the_roles() {
    let (actions, fake) = rig();
    let (skills, models) = load(&actions, "/Users/me");
    assert!(matches!(
        skills,
        SkillsShown::Found {
            install_on: true,
            ..
        }
    ));
    assert!(models.status().is_none());
    let seen = fake.seen.lock().unwrap();
    assert_eq!(seen[0], ("install.plan".to_owned(), json!({})));
    assert_eq!(seen[1], ("catalog.roles".to_owned(), json!({})));
}

#[test]
fn reinstall_names_only_skills_so_the_hook_entries_are_left_alone() {
    let (actions, fake) = rig();
    let names = vec![
        "notchling-pages".to_owned(),
        "notchling-delegate".to_owned(),
    ];
    assert!(reinstall(&actions, &names).is_ok());
    let seen = fake.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, "install.apply");
    assert_eq!(
        seen[0].1,
        json!({"what": ["notchling-pages", "notchling-delegate"]})
    );
}

#[test]
fn with_no_daemon_both_parts_say_so() {
    let actions = Actions::fixed(crate::acting::Acting::Detached);
    let (skills, models) = load(&actions, "/Users/me");
    assert_eq!(skills, SkillsShown::Detached);
    assert_eq!(models, super::models_rows::ModelsShown::Detached);
}
