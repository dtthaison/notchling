use crate::op_group::OpGroup;
use notchling_engine::machines::process::ProcessRunner;
use notchling_engine::machines::runner::Runner;
use notchling_engine::machines::tailscale::Tailscale;
use notchling_local_api::{ops, ApiError, Caller, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::Arc;

pub struct TailscaleOps {
    tailscale: Tailscale,
}

impl TailscaleOps {
    pub fn system() -> TailscaleOps {
        TailscaleOps::new(
            Arc::new(ProcessRunner),
            &notchling_engine::machines::tailscale::default_cli(),
        )
    }

    pub fn new(runner: Arc<dyn Runner>, cli: &str) -> TailscaleOps {
        TailscaleOps {
            tailscale: Tailscale::new(runner, cli),
        }
    }

    fn switch(&self, args: &Value) -> Result<Value, ApiError> {
        let wanted = args
            .get("tailnet")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| ApiError::bad_request("tailnet is the network to switch to"))?;
        let networks = self.tailscale.networks().map_err(ApiError::internal)?;
        let found: Vec<_> = networks
            .iter()
            .filter(|n| n.is(wanted) || n.id == wanted)
            .collect();
        let network = match found.as_slice() {
            [one] => *one,
            [] => {
                return Err(ApiError::bad_request(
                    "That isn't one of this Mac's Tailscale networks",
                ))
            }
            _ => return Err(ApiError::bad_request(
                "Two Tailscale networks have that name; use the id from `tailscale switch --list`",
            )),
        };
        if network.active {
            return Ok(json!({"switched": false, "tailnet": network.tailnet}));
        }
        self.tailscale
            .switch(&network.id)
            .map_err(|why| ApiError::new(ErrorCode::Failed, why))?;
        Ok(json!({"switched": true, "tailnet": network.tailnet}))
    }
}

impl OpGroup for TailscaleOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == ops::TAILSCALE_SWITCH).then_some(OpSpec::APP)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::TAILSCALE_SWITCH => {
                if caller.tier != Tier::App {
                    return Err(ApiError::new(
                        ErrorCode::Forbidden,
                        "Only the app switches Tailscale networks",
                    ));
                }
                self.switch(args)
            }
            other => Err(ApiError::internal(format!("{other} has no handler"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::machines::runner::{CommandResult, ScriptedRunner};
    use notchling_local_api::ClientKind;

    const LIST: &str = "ID    Tailnet              Account\na1b2  person@example.com   person@example.com*\nc3d4  work.example         work@example.com\n";

    fn caller(tier: Tier) -> Caller {
        Caller {
            tier,
            client: if tier == Tier::App {
                ClientKind::App
            } else {
                ClientKind::Cli
            },
            peer: None,
            pid: None,
        }
    }

    fn group() -> (Arc<ScriptedRunner>, TailscaleOps) {
        let runner = Arc::new(ScriptedRunner::new());
        runner.on(
            |a| a.get(2).map(String::as_str) == Some("--list"),
            CommandResult::ok(LIST),
        );
        let ops = TailscaleOps::new(runner.clone(), "/fake/tailscale");
        (runner, ops)
    }

    fn commands(runner: &ScriptedRunner) -> Vec<Vec<String>> {
        runner.calls().into_iter().map(|c| c.argv).collect()
    }

    #[test]
    fn the_switch_is_app_tier_and_nothing_else_is_offered() {
        let (_, ops) = group();
        assert_eq!(ops.spec(ops::TAILSCALE_SWITCH), Some(OpSpec::APP));
        assert_eq!(OpSpec::APP.tier, Tier::App);
        assert_eq!(ops.spec("tailscale.status"), None);
        assert_eq!(ops.spec("tailscale.list"), None);
    }

    #[test]
    fn an_agent_or_the_command_line_cannot_switch_even_if_called_directly() {
        let (runner, ops) = group();
        let err = ops
            .call(
                &caller(Tier::Agent),
                ops::TAILSCALE_SWITCH,
                &json!({"tailnet": "work.example"}),
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn the_app_switches_by_running_one_exact_command() {
        let (runner, ops) = group();
        let reply = ops
            .call(
                &caller(Tier::App),
                ops::TAILSCALE_SWITCH,
                &json!({"tailnet": "work.example"}),
            )
            .unwrap();
        assert_eq!(reply, json!({"switched": true, "tailnet": "work.example"}));
        assert_eq!(
            commands(&runner),
            vec![
                vec!["/fake/tailscale", "switch", "--list"],
                vec!["/fake/tailscale", "switch", "c3d4"],
            ]
        );
    }

    #[test]
    fn the_active_network_and_an_unknown_one_run_no_switch() {
        let (runner, ops) = group();
        let app = caller(Tier::App);
        let already = ops
            .call(
                &app,
                ops::TAILSCALE_SWITCH,
                &json!({"tailnet": "person@example.com"}),
            )
            .unwrap();
        assert_eq!(already["switched"], json!(false));
        let unknown = ops
            .call(
                &app,
                ops::TAILSCALE_SWITCH,
                &json!({"tailnet": "--accept-risk=all"}),
            )
            .unwrap_err();
        assert_eq!(unknown.code, ErrorCode::BadRequest);
        let missing = ops
            .call(&app, ops::TAILSCALE_SWITCH, &json!({}))
            .unwrap_err();
        assert_eq!(missing.code, ErrorCode::BadRequest);
        assert!(commands(&runner).iter().all(|c| c[2] == "--list"));
    }

    #[test]
    fn a_refused_switch_is_a_failure_with_the_clis_words() {
        let runner = Arc::new(ScriptedRunner::new());
        runner.on(
            |a| a.get(2).map(String::as_str) == Some("--list"),
            CommandResult::ok(LIST),
        );
        runner.on(
            |a| a.get(2).map(String::as_str) == Some("c3d4"),
            CommandResult::new(1, "", "switch failed: not logged in\n"),
        );
        let ops = TailscaleOps::new(runner, "tailscale");
        let err = ops
            .call(
                &caller(Tier::App),
                ops::TAILSCALE_SWITCH,
                &json!({"tailnet": "work.example"}),
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Failed);
        assert_eq!(
            err.message,
            "tailscale switch: switch failed: not logged in"
        );
    }

    #[test]
    fn a_login_shared_by_two_networks_does_not_pick_one_and_an_account_is_not_a_network() {
        let runner = Arc::new(ScriptedRunner::new());
        runner.on(
            |a| a.get(2).map(String::as_str) == Some("--list"),
            CommandResult::ok(
                "ID    Tailnet              Account\na1b2  person@example.com   person@example.com*\nc3d4  work.example         person@example.com\ne5f6  work.example         other@example.com\n",
            ),
        );
        let ops = TailscaleOps::new(runner.clone(), "tailscale");
        let app = caller(Tier::App);
        let account = ops
            .call(
                &app,
                ops::TAILSCALE_SWITCH,
                &json!({"tailnet": "other@example.com"}),
            )
            .unwrap_err();
        assert_eq!(account.code, ErrorCode::BadRequest);
        let twice = ops
            .call(
                &app,
                ops::TAILSCALE_SWITCH,
                &json!({"tailnet": "work.example"}),
            )
            .unwrap_err();
        assert_eq!(twice.code, ErrorCode::BadRequest);
        assert!(twice.message.contains("Two Tailscale networks"));
        let by_id = ops
            .call(&app, ops::TAILSCALE_SWITCH, &json!({"tailnet": "e5f6"}))
            .unwrap();
        assert_eq!(by_id["switched"], json!(true));
        assert!(commands(&runner).iter().any(|c| c[2] == "e5f6"));
    }
}
