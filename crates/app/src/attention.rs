use notchling_engine::inbox::AlertAction;

pub const FIRST_SIGHT_QUIET_MS: i64 = 10_000;
pub const TONE_GAP_MS: i64 = 4_000;
pub const PULSE_MS: i64 = 60_000;
pub const STRONGER_MS: i64 = 180_000;
pub const LAST_CALL_LEAD_MS: i64 = 60_000;
pub const NEAR_LAST_CALL_MS: i64 = 90_000;
pub const SHORT_ITEM_MS: i64 = 240_000;
pub const OPEN_MS: i64 = 45_000;
pub const CALM_OPEN_MS: i64 = 8_000;
pub const EXPIRED_OPEN_MS: i64 = 4_000;
pub const URGENT_REPEAT_MS: i64 = 120_000;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BlockingTarget {
    Item(String),
    HookPrompt(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub key: BlockingTarget,
    pub title: String,
    pub waiting_since_ms: i64,
    pub expires_ms: Option<i64>,
    pub alert_actions: Vec<AlertAction>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    Calm,
    #[default]
    Normal,
    Urgent,
}

impl Level {
    pub fn parse(value: &str) -> Option<Level> {
        match value {
            "calm" => Some(Level::Calm),
            "normal" => Some(Level::Normal),
            "urgent" => Some(Level::Urgent),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::Calm => "calm",
            Level::Normal => "normal",
            Level::Urgent => "urgent",
        }
    }
}

pub fn tray_still(level: Level, reduce_motion: bool) -> bool {
    reduce_motion || level == Level::Calm
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Notifications {
    #[default]
    Allowed,
    Denied,
}

#[derive(Clone, Debug, Default)]
pub struct Input {
    pub targets: Vec<Target>,
    pub level: Level,
    pub notch_shown: bool,
    pub notifications: Notifications,
    pub reduce_motion: bool,
    pub app_active: bool,
    pub delivered: Vec<BlockingTarget>,
    pub closed: Vec<BlockingTarget>,
    pub expired: Vec<BlockingTarget>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BannerStep {
    Start,
    Pulse,
    Stronger,
    LastCall,
    Expired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warning {
    NoChannel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    OpenNotch {
        key: BlockingTarget,
        ms: i64,
        stay: bool,
    },
    GrowNotch {
        key: BlockingTarget,
    },
    CloseNotch,
    Tone {
        times: u8,
    },
    Banner {
        key: BlockingTarget,
        step: BannerStep,
        actions: Vec<AlertAction>,
        sound: bool,
    },
    RemoveBanner {
        key: BlockingTarget,
    },
    PulseTray,
    DockBadge(Option<u32>),
    DockBounce {
        critical: bool,
    },
    CancelBounce,
    Warn(Warning),
}

struct TargetState {
    target: Target,
    anchor_ms: i64,
    watermark: i64,
    level: Level,
    banner: bool,
    bounce: Option<bool>,
    stay: bool,
}

#[derive(Default)]
pub struct Attention {
    states: Vec<TargetState>,
    last_tone_ms: Option<i64>,
    warned: bool,
    badge: usize,
}

fn end_offset(target: &Target, anchor_ms: i64) -> Option<i64> {
    target.expires_ms.map(|end| (end - anchor_ms).max(0))
}

fn steps_until(expires_offset: Option<i64>, level: Level, until: i64) -> Vec<(i64, BannerStep)> {
    let last_call = expires_offset.map(|end| (end - LAST_CALL_LEAD_MS).max(0));
    let crowded = |offset: i64| last_call.is_some_and(|call| offset >= call - NEAR_LAST_CALL_MS);
    let mut steps = vec![(0, BannerStep::Start)];
    if level != Level::Calm {
        if !crowded(PULSE_MS) {
            steps.push((PULSE_MS, BannerStep::Pulse));
        }
        let short = expires_offset.is_some_and(|end| end < SHORT_ITEM_MS);
        if !short && !crowded(STRONGER_MS) {
            steps.push((STRONGER_MS, BannerStep::Stronger));
        }
        if level == Level::Urgent {
            let mut repeats = vec![2 * PULSE_MS];
            let mut offset = STRONGER_MS + URGENT_REPEAT_MS;
            while offset <= until {
                repeats.push(offset);
                offset += URGENT_REPEAT_MS;
            }
            for offset in repeats {
                if offset <= until && !crowded(offset) {
                    steps.push((offset, BannerStep::Pulse));
                }
            }
        }
    }
    if let Some(call) = last_call {
        steps.push((call, BannerStep::LastCall));
    }
    steps.sort();
    steps
}

fn sounds(step: BannerStep) -> bool {
    step != BannerStep::Expired
}

impl Attention {
    pub fn new() -> Attention {
        Attention::default()
    }

    pub fn later(&mut self, key: &BlockingTarget) {
        if let Some(state) = self.states.iter_mut().find(|s| &s.target.key == key) {
            state.stay = false;
        }
    }

    pub fn next_due_ms(&self, input: &Input, now_ms: i64) -> Option<i64> {
        input
            .targets
            .iter()
            .filter_map(|target| {
                let Some(state) = self.states.iter().find(|s| s.target.key == target.key) else {
                    return Some(now_ms);
                };
                let end = end_offset(target, state.anchor_ms);
                let age = (now_ms - state.anchor_ms).max(0);
                steps_until(end, input.level, age.max(state.watermark) + SHORT_ITEM_MS)
                    .into_iter()
                    .find(|(offset, _)| *offset > state.watermark)
                    .map(|(offset, _)| state.anchor_ms + offset)
            })
            .min()
    }

    pub fn step(&mut self, input: &Input, now_ms: i64) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.leave(input, now_ms, &mut fx);
        if input.notifications == Notifications::Allowed {
            self.warned = false;
        } else if !input.targets.is_empty() && !self.warned {
            self.warned = true;
            fx.push(Effect::Warn(Warning::NoChannel));
        }
        for target in &input.targets {
            self.visit(input, target, now_ms, &mut fx);
        }
        if self.badge != input.targets.len() {
            self.badge = input.targets.len();
            let count = u32::try_from(self.badge).ok().filter(|n| *n > 0);
            fx.push(Effect::DockBadge(count));
        }
        if input.app_active && self.states.iter().any(|s| s.bounce.is_some()) {
            for state in &mut self.states {
                state.bounce = None;
            }
            fx.push(Effect::CancelBounce);
        }
        fx
    }

    fn leave(&mut self, input: &Input, now_ms: i64, fx: &mut Vec<Effect>) {
        let (kept, gone): (Vec<TargetState>, Vec<TargetState>) = std::mem::take(&mut self.states)
            .into_iter()
            .partition(|state| input.targets.iter().any(|t| t.key == state.target.key));
        self.states = kept;
        let mut closed_stay = false;
        let mut lost_bounce = false;
        for state in gone {
            let key = state.target.key.clone();
            let ended = input.expired.contains(&key)
                || (!input.closed.contains(&key)
                    && state.target.expires_ms.is_some_and(|end| now_ms >= end));
            if ended {
                if input.notifications == Notifications::Allowed {
                    fx.push(Effect::Banner {
                        key: key.clone(),
                        step: BannerStep::Expired,
                        actions: state.target.alert_actions.clone(),
                        sound: false,
                    });
                }
                if input.notch_shown {
                    fx.push(Effect::OpenNotch {
                        key,
                        ms: EXPIRED_OPEN_MS,
                        stay: false,
                    });
                }
            } else if state.banner || input.delivered.contains(&key) {
                fx.push(Effect::RemoveBanner { key });
            }
            closed_stay |= state.stay;
            lost_bounce |= state.bounce.is_some();
        }
        if closed_stay && input.notch_shown && !self.states.iter().any(|s| s.stay) {
            fx.push(Effect::CloseNotch);
        }
        if lost_bounce {
            fx.push(Effect::CancelBounce);
            if !input.app_active && self.states.iter().any(|s| s.bounce == Some(true)) {
                fx.push(Effect::DockBounce { critical: true });
            }
        }
    }

    fn visit(&mut self, input: &Input, target: &Target, now_ms: i64, fx: &mut Vec<Effect>) {
        let fresh = !self.states.iter().any(|s| s.target.key == target.key);
        if fresh {
            self.states.push(TargetState {
                target: target.clone(),
                anchor_ms: target.waiting_since_ms.min(now_ms),
                watermark: -1,
                level: input.level,
                banner: false,
                bounce: None,
                stay: false,
            });
        }
        let Some(index) = self.states.iter().position(|s| s.target.key == target.key) else {
            return;
        };
        let (anchor, watermark) = {
            let state = &mut self.states[index];
            state.target = target.clone();
            (state.anchor_ms, state.watermark)
        };
        let age = (now_ms - anchor).max(0);
        if self.states[index].level != input.level {
            self.states[index].level = input.level;
            self.states[index].watermark = age.max(watermark);
            return;
        }
        let end = end_offset(target, anchor);
        let due: Vec<(i64, BannerStep)> = steps_until(end, input.level, age)
            .into_iter()
            .filter(|(offset, _)| *offset > watermark && *offset <= age)
            .collect();
        let Some(&(_, step)) = due.last() else {
            return;
        };
        self.states[index].watermark = age;
        let allowed = input.notifications == Notifications::Allowed;

        if fresh && age > FIRST_SIGHT_QUIET_MS {
            if allowed && !input.delivered.contains(&target.key) {
                self.states[index].banner = true;
                fx.push(banner(target, step, false));
            }
            return;
        }

        if input.notch_shown {
            self.notch(index, input, step, now_ms, fx);
        }

        let past_end = end.is_some_and(|offset| age >= offset);
        let loud = sounds(step)
            && !past_end
            && input.level != Level::Calm
            && allowed
            && self
                .last_tone_ms
                .is_none_or(|at| now_ms - at >= TONE_GAP_MS);
        if allowed {
            self.states[index].banner = true;
            fx.push(banner(target, step, loud));
        }
        if loud {
            self.last_tone_ms = Some(now_ms);
            let times = if input.level == Level::Urgent { 2 } else { 1 };
            fx.push(Effect::Tone { times });
        }
        if input.level != Level::Calm && sounds(step) && !input.reduce_motion {
            fx.push(Effect::PulseTray);
        }
        if let Some(critical) = bounce(input, step) {
            if !input.app_active {
                let held = self.states[index].bounce == Some(true);
                self.states[index].bounce = Some(held || critical);
                fx.push(Effect::DockBounce { critical });
            }
        }
    }

    fn notch(
        &mut self,
        index: usize,
        input: &Input,
        step: BannerStep,
        now_ms: i64,
        fx: &mut Vec<Effect>,
    ) {
        let key = self.states[index].target.key.clone();
        let end = self.states[index].target.expires_ms;
        match step {
            BannerStep::Start => {
                let (ms, stay) = match input.level {
                    Level::Calm => (CALM_OPEN_MS, false),
                    Level::Normal => (OPEN_MS, false),
                    Level::Urgent => (OPEN_MS, true),
                };
                self.states[index].stay = stay;
                fx.push(Effect::OpenNotch { key, ms, stay });
            }
            BannerStep::Pulse => {
                if input.level != Level::Calm {
                    fx.push(Effect::GrowNotch { key });
                }
            }
            BannerStep::Stronger => fx.push(Effect::OpenNotch {
                key,
                ms: OPEN_MS,
                stay: false,
            }),
            BannerStep::LastCall => {
                let left = end.map_or(LAST_CALL_LEAD_MS, |e| e - now_ms);
                fx.push(Effect::OpenNotch {
                    key,
                    ms: left.clamp(1_000, LAST_CALL_LEAD_MS),
                    stay: false,
                });
            }
            BannerStep::Expired => fx.push(Effect::OpenNotch {
                key,
                ms: EXPIRED_OPEN_MS,
                stay: false,
            }),
        }
    }
}

fn banner(target: &Target, step: BannerStep, sound: bool) -> Effect {
    Effect::Banner {
        key: target.key.clone(),
        step,
        actions: target.alert_actions.clone(),
        sound,
    }
}

fn bounce(input: &Input, step: BannerStep) -> Option<bool> {
    match (input.level, step) {
        (Level::Calm, _) => None,
        (Level::Urgent, BannerStep::Start) => Some(true),
        (Level::Normal, BannerStep::Start) if !input.notch_shown => Some(false),
        (_, BannerStep::Stronger | BannerStep::LastCall) => Some(false),
        _ => None,
    }
}

#[cfg(test)]
#[path = "attention_tests.rs"]
mod tests;
