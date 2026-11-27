use std::collections::HashMap;

use notchling_platform::notify::{Authorization, Notifier, Response};

use super::banners::{
    banner_id, closures_of, notification_for, route, targets_of, BannerInfo, Route,
};
use super::model::InboxModel;
use crate::attention::{Attention, BlockingTarget, Effect, Input, Level, Notifications, Target};

#[derive(Clone, Copy, Debug)]
pub struct Surroundings {
    pub level: Level,
    pub notch_shown: bool,
    pub reduce_motion: bool,
    pub app_active: bool,
}

#[derive(Default)]
pub struct Driver {
    machine: Attention,
    posted: HashMap<String, i64>,
    known: HashMap<BlockingTarget, (Target, BannerInfo)>,
}

impl Driver {
    pub fn new() -> Driver {
        Driver::default()
    }

    pub fn machine(&mut self) -> &mut Attention {
        &mut self.machine
    }

    pub fn run(
        &mut self,
        model: &InboxModel,
        around: &Surroundings,
        notifier: &dyn Notifier,
        now_ms: i64,
    ) -> Vec<Effect> {
        let found = targets_of(model);
        let targets: Vec<_> = found.iter().map(|(target, _)| target.clone()).collect();
        let open: Vec<BlockingTarget> = targets.iter().map(|target| target.key.clone()).collect();
        for (target, info) in found {
            self.known.insert(target.key.clone(), (target, info));
        }
        let delivered = notifier
            .delivered()
            .iter()
            .filter_map(|id| super::banners::key_of(id))
            .collect();
        let (closed, expired) = closures_of(model);
        let input = Input {
            targets,
            level: around.level,
            notch_shown: around.notch_shown,
            notifications: match notifier.authorization() {
                Authorization::Allowed => Notifications::Allowed,
                Authorization::Denied => Notifications::Denied,
            },
            reduce_motion: around.reduce_motion,
            app_active: around.app_active,
            delivered,
            closed,
            expired,
        };
        let mut ui = Vec::new();
        for effect in self.machine.step(&input, now_ms) {
            match effect {
                Effect::Banner {
                    key, step, sound, ..
                } => {
                    let Some((target, info)) = self.known.get(&key) else {
                        continue;
                    };
                    let id = banner_id(&key);
                    let notification = notification_for(target, info, step, sound, now_ms);
                    if notifier.upsert(&id, &notification).is_ok() {
                        self.posted.insert(id, now_ms);
                    }
                }
                Effect::RemoveBanner { key } => {
                    let id = banner_id(&key);
                    self.posted.remove(&id);
                    let _ = notifier.remove(&id);
                }
                other => ui.push(other),
            }
        }
        self.known.retain(|key, _| open.contains(key));
        ui
    }

    pub fn route(&self, response: &Response, now_ms: i64) -> Route {
        route(response, self.posted.get(&response.id).copied(), now_ms)
    }

    pub fn next_due_ms(
        &self,
        model: &InboxModel,
        around: &Surroundings,
        now_ms: i64,
    ) -> Option<i64> {
        let targets = targets_of(model).into_iter().map(|(t, _)| t).collect();
        let input = Input {
            targets,
            level: around.level,
            notch_shown: around.notch_shown,
            ..Input::default()
        };
        self.machine.next_due_ms(&input, now_ms)
    }
}
