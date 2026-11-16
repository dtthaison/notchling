use crate::cluster_returns::Returns;
use notchling_engine::cluster::claims::Claim;
use notchling_engine::cluster::state::Command;
use notchling_engine::cluster::wire::OriginTier;
use std::time::Duration;

pub const TOUCH_WAIT: Duration = Duration::from_secs(5);

impl Returns {
    fn mine_untouched(&self) -> Vec<Claim> {
        let (Some(state), Some(me)) = (self.load(), self.p.mover.me()) else {
            return Vec::new();
        };
        state
            .claims
            .into_values()
            .filter(|c| c.node == me && !c.touched)
            .collect()
    }

    fn record_touch(&self, claim: &Claim, retry: bool) {
        let key = (claim.session.clone(), claim.n);
        let go = self.remember(|m| {
            if m.touched.contains(&key) || (!retry && m.tried.contains(&key)) {
                return false;
            }
            m.tried.insert(key.clone());
            true
        });
        if !go {
            return;
        }
        let command = Command::Touched {
            session: claim.session.clone(),
        };
        let link = &self.p.mover.p.link;
        match link.record(command, OriginTier::Node, TOUCH_WAIT) {
            Ok(()) => {
                self.remember(|m| m.touched.insert(key));
            }
            Err(why) => self.failed(
                &format!("touch-failed:{}", claim.session),
                &format!(
                    "Couldn't tell the cluster {} is running here",
                    claim.session
                ),
                &why.message,
            ),
        }
    }

    pub fn before_send(&self, session: &str) {
        let claim = self
            .mine_untouched()
            .into_iter()
            .find(|c| c.session == session);
        if let Some(claim) = claim {
            self.record_touch(&claim, false);
        }
    }

    pub(crate) fn touch_working(&self) {
        let claims = self.mine_untouched();
        if claims.is_empty() {
            return;
        }
        let ids: Vec<String> = claims.iter().map(|c| c.session.clone()).collect();
        let working = self.p.mover.working(&ids);
        for claim in &claims {
            let tried = self.remember(|m| m.tried.contains(&(claim.session.clone(), claim.n)));
            if working.contains(&claim.session) || tried {
                self.record_touch(claim, true);
            }
        }
    }
}
