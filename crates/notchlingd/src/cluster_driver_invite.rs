use super::failed;
use super::{Driver, Reply};
use crate::cluster_node::Answer;
use notchling_engine::bridge::net::crypto::random_token;
use notchling_engine::cluster::core::LocalCmd;
use notchling_engine::cluster::invite::Invite;
use notchling_engine::cluster::join::{encode_token, JoinToken};
use notchling_engine::cluster::wire::{Origin, OriginTier, WireError};
use notchling_engine::cluster::{Input, NodeId};
use serde_json::{json, Value};

impl Driver {
    pub(super) fn invite(&mut self, minter: Option<NodeId>, invite: Invite, answer: Answer) {
        let Some(state) = self.core.state() else {
            let _ = answer.send(Err(WireError::new(
                "no_cluster",
                "this node is in no cluster",
            )));
            return;
        };
        if !self.core.is_home() && minter.is_some() {
            let _ = answer.send(Err(WireError::new(
                "forbidden",
                "only the primary makes invites",
            )));
            return;
        }
        if !self.core.is_home() {
            let (home, origin) = (
                state.home.clone(),
                Origin {
                    node: self.me.clone(),
                    tier: OriginTier::App,
                },
            );
            let args = json!({
                "node": invite.node,
                "name": invite.name,
                "canBeMaster": invite.can_be_master,
                "acting": invite.acting,
            });
            let cmd = LocalCmd::Forward {
                node: home,
                op: "cluster.invite".to_string(),
                args: args.into(),
                origin,
            };
            self.local_cmd(cmd, Some(answer));
            return;
        }
        let secret = match random_token(32) {
            Ok(secret) => secret,
            Err(e) => {
                let _ = answer.send(Err(failed(e.to_string())));
                return;
            }
        };
        let token = encode_token(&JoinToken {
            cluster: state.id.clone(),
            home: self.me.clone(),
            fingerprint: self.net.fingerprint.clone(),
            addrs: self.net.transport.addrs(),
            port: self.port.unwrap_or(self.setup.port),
            secret: secret.clone(),
        });
        let node = invite.node.to_string();
        let id = self.reply_id(Reply::Invite(answer, token, node));
        self.step(Input::Local {
            id,
            cmd: LocalCmd::Invite {
                secret,
                invite,
                minter: minter.unwrap_or_else(|| self.me.clone()),
            },
        });
    }
}

pub(super) fn invited(token: String, result: Result<Value, WireError>) -> Result<Value, WireError> {
    result.map(|core| json!({"token": token, "expiresMs": core.get("expiresMs")}))
}
