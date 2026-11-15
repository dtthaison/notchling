//! The calls behind Settings > Skills & Models: read the skills' plan and the model choices,
//! and send the reinstall. Only reads and the one acting op `install.apply`; the daemon's
//! `install` switch decides whether that one writes.

use notchling_local_api::ops;
use serde_json::{json, Value};

use super::models_rows::ModelsShown;
use super::skills_rows::SkillsShown;
use crate::acting::Request;
use crate::actions::Actions;

pub fn load(actions: &Actions, home: &str) -> (SkillsShown, ModelsShown) {
    let skills = actions.read(&Request {
        op: ops::INSTALL_PLAN,
        args: json!({}),
    });
    let models = actions.read(&Request {
        op: ops::CATALOG_ROLES,
        args: json!({}),
    });
    (
        SkillsShown::from_reply(skills),
        ModelsShown::from_reply(models, home),
    )
}

/// Writes the named skills and leaves the hook entries alone (`what` lists skills only).
pub fn reinstall(actions: &Actions, names: &[String]) -> Result<Value, String> {
    actions.read(&Request {
        op: ops::INSTALL_APPLY,
        args: json!({ "what": names }),
    })
}
