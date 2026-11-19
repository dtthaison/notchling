use notchling_engine::machines::addresses::Route;
use notchling_engine::machines::host_trust::{HostKey, ScanFailure};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrustWhere {
    Tailscale(String),
    Edit(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrustFail {
    Jump,
    Nothing,
    Write(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrustFlow {
    Scanning,
    Review(Vec<HostKey>),
    Saving(Vec<HostKey>),
    Failed(TrustFail),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustAsk {
    pub at: TrustWhere,
    pub route: Route,
    pub round: u64,
    pub flow: TrustFlow,
}

impl TrustAsk {
    pub fn begin(at: TrustWhere, route: Route, round: u64) -> TrustAsk {
        let flow = if route.jump.is_some() {
            TrustFlow::Failed(TrustFail::Jump)
        } else {
            TrustFlow::Scanning
        };
        TrustAsk {
            at,
            route,
            round,
            flow,
        }
    }

    pub fn needs_scan(&self) -> bool {
        self.flow == TrustFlow::Scanning
    }

    pub fn scanned(&mut self, round: u64, result: Result<Vec<HostKey>, ScanFailure>) {
        if round != self.round || self.flow != TrustFlow::Scanning {
            return;
        }
        self.flow = match result {
            Ok(keys) => TrustFlow::Review(keys),
            Err(ScanFailure::Jump) => TrustFlow::Failed(TrustFail::Jump),
            Err(ScanFailure::Nothing) => TrustFlow::Failed(TrustFail::Nothing),
        };
    }

    pub fn confirm(&mut self) -> Option<Vec<HostKey>> {
        let TrustFlow::Review(keys) = &self.flow else {
            return None;
        };
        let keys = keys.clone();
        self.flow = TrustFlow::Saving(keys.clone());
        Some(keys)
    }

    pub fn saved(&mut self, round: u64, result: Result<usize, String>) -> bool {
        if round != self.round || !matches!(self.flow, TrustFlow::Saving(_)) {
            return false;
        }
        match result {
            Ok(_) => true,
            Err(why) => {
                self.flow = TrustFlow::Failed(TrustFail::Write(why));
                false
            }
        }
    }

    pub fn is_at(&self, at: &TrustWhere) -> bool {
        self.at == *at
    }
}
