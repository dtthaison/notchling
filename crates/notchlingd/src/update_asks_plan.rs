use super::*;
use notchling_engine::versions::update::update_argv;
use notchling_engine::versions::{Tool, Version};

impl UpdateAsks {
    pub fn plan(&self, tool: &str, to: Option<&str>, look: Look) -> Result<Plan, ApiError> {
        let tool = Tool::from_name(tool).ok_or_else(|| ApiError::bad_request(USAGE))?;
        if tool == Tool::Notchling {
            return Err(bad(NOTCHLING_SELF, "cant_update"));
        }
        let (installed, back) = self
            .versions
            .update_state(tool)
            .ok_or_else(|| bad(NOT_INSTALLED, "not_installed"))?;
        let from = installed
            .version
            .clone()
            .ok_or_else(|| bad(NOT_INSTALLED, "not_installed"))?;
        let to = match to {
            None => None,
            Some(text) => {
                let asked = Version::parse(text).ok_or_else(|| ApiError::bad_request(USAGE))?;
                if back.as_ref() != Some(&asked) {
                    return Err(bad(NOT_GO_BACK, "not_go_back"));
                }
                Some(asked)
            }
        };
        update_argv(tool, installed.method, to.as_ref()).map_err(|why| bad(&why, "cant_update"))?;
        let newest = match to {
            Some(_) => None,
            None => self.newest(tool, look),
        };
        if to.is_none() && newest.as_ref().is_some_and(|n| *n <= from) {
            return Err(bad(UP_TO_DATE, "up_to_date"));
        }
        if look == Look::Probe && self.versions.updates().is_running(tool) {
            return Err(bad(RUNNING, "running"));
        }
        let target = to.clone().or(newest);
        Ok(Self::plan_of(tool, &installed, &from, to, target))
    }

    fn newest(&self, tool: Tool, look: Look) -> Option<Version> {
        let updates = self.versions.updates();
        if look == Look::Probe {
            let _ = updates.latest(tool);
        }
        updates.known_latest(tool)
    }

    fn plan_of(
        tool: Tool,
        installed: &notchling_engine::versions::mac::Installed,
        from: &Version,
        back: Option<Version>,
        target: Option<Version>,
    ) -> Plan {
        let action = if back.is_some() { "back" } else { "update" };
        let mut args = json!({"tool": tool.name()});
        if let Some(back) = &back {
            args["to"] = json!(back.as_str());
        }
        let shown = json!({
            "tool": tool.title(),
            "method": installed.method.title(),
            "from": from.as_str(),
            "to": target.as_ref().map(|v| v.as_str()),
        });
        let line = match (action, target.is_some()) {
            ("back", _) => "consent-update-back",
            (_, true) => "consent-update-tool",
            (_, false) => "consent-update-tool-latest",
        };
        Plan {
            action,
            target: format!("{}:{WHERE}", tool.name()),
            change: json!({
                "action": action,
                "tool": tool.name(),
                "method": installed.method,
                "from": from.as_str(),
                "to": target.as_ref().map(|v| v.as_str()),
                "args": args,
                "apply": {"op": ops::VERSIONS_UPDATE_LOCAL, "args": args},
            }),
            facts: json!([
                {"line": line, "args": shown},
                {"line": "consent-update-running", "args": {}},
            ]),
        }
    }
}
