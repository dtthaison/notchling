use super::ClaudeAccounts;
use notchling_events::field::Percent;
use notchling_events::kinds::accounts::{
    AccountCause, AccountLogin, AccountSwitched, LaneAction, LaneChanged, LoginAction,
};
use notchling_events::kinds::choices::{DoneOrFailed, LimitWindow, Provider};
use notchling_events::spec::Outcome;
use notchling_events::{Account, Event, Lane, Ref};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwitchWhy {
    pub cause: AccountCause,
    pub window: Option<LimitWindow>,
    pub used_pct: Option<f64>,
}

impl SwitchWhy {
    pub const USER: SwitchWhy = SwitchWhy::of(AccountCause::User);
    pub const MOVE_ALL: SwitchWhy = SwitchWhy::of(AccountCause::MoveAll);
    pub const OTHER: SwitchWhy = SwitchWhy::of(AccountCause::Other);

    pub const fn of(cause: AccountCause) -> SwitchWhy {
        SwitchWhy {
            cause,
            window: None,
            used_pct: None,
        }
    }

    pub fn limit(window: Option<LimitWindow>, used_pct: Option<f64>) -> SwitchWhy {
        SwitchWhy {
            cause: AccountCause::LimitAuto,
            window,
            used_pct,
        }
    }
}

fn outcome(result: DoneOrFailed) -> Outcome {
    match result {
        DoneOrFailed::Done => Outcome::Ok,
        _ => Outcome::Failed,
    }
}

impl ClaudeAccounts {
    pub(super) fn push_switch(&self, from: Option<&str>, to: &str, why: &SwitchWhy) {
        self.env.events.push(Event::new(
            &AccountSwitched {
                provider: Provider::Claude,
                from: from.map(Ref::<Account>::new),
                to: Ref::<Account>::new(to),
                cause: why.cause,
                window: why.window,
                used_pct: why
                    .used_pct
                    .map(|p| Percent::new(p.max(0.0).round() as u64)),
                result: DoneOrFailed::Done,
            },
            Outcome::Ok,
            self.now(),
        ));
    }

    pub(super) fn push_login(&self, account: &str, action: LoginAction, result: DoneOrFailed) {
        self.env.events.push(Event::new(
            &AccountLogin {
                provider: Provider::Claude,
                account: Ref::<Account>::new(account),
                action,
                result,
            },
            outcome(result),
            self.now(),
        ));
    }

    pub(super) fn push_lane(&self, lane: &str, action: LaneAction, account: Option<&str>) {
        self.env.events.push(Event::new(
            &LaneChanged {
                lane: Ref::<Lane>::new(lane),
                action,
                account: account.map(Ref::<Account>::new),
            },
            Outcome::Ok,
            self.now(),
        ));
    }
}
