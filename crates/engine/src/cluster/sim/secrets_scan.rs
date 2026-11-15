use serde_json::Value;

use super::secrets::Path;
use super::world::World;
use crate::cluster::core::Effect;
use crate::cluster::id::NodeId;
use crate::cluster::wire::Msg;

fn job_of(args: &Value) -> Option<&str> {
    args.get("job").and_then(Value::as_str)
}

impl World {
    pub(super) fn scan_effect(&mut self, n: &NodeId, effect: &Effect) {
        match effect {
            Effect::Send { to, msg } => {
                let text = serde_json::to_string(msg).unwrap_or_default();
                let (op, job) = match msg {
                    Msg::Call { op, args, .. } => (op.as_str(), job_of(args)),
                    _ => ("msg", None),
                };
                self.secrets.record(n, Path::Wire { op, to, job }, &text);
            }
            Effect::Persist { entry, changes } => {
                let text = format!(
                    "{}{}",
                    serde_json::to_string(entry).unwrap_or_default(),
                    serde_json::to_string(changes).unwrap_or_default()
                );
                self.secrets.record(n, Path::Journal, &text);
            }
            Effect::Publish { data, .. } => {
                self.secrets.record(n, Path::Publish, &data.to_string());
            }
            Effect::RunLocal { op, args, .. } => {
                let path = Path::Run {
                    op,
                    job: job_of(args),
                };
                self.secrets.record(n, path, &args.to_string());
            }
            Effect::Answer { result, .. } => {
                let text = match result {
                    Ok(v) => v.to_string(),
                    Err(e) => e.message.clone(),
                };
                self.secrets.record(n, Path::Answer, &text);
            }
            _ => {}
        }
    }
}
