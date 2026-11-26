use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use notchling_engine::inbox::{AlertAction, Item, State};
use notchling_locale as t;
use notchling_platform::notify::{
    Authorization, BannerAction, Category, Notification, Notifier, Response,
};
use notchling_platform::{Error, Result};

use super::model::{words_of, InboxModel};
use crate::attention::{BannerStep, BlockingTarget, Target};
use crate::chat::format::age;
use crate::menu::BlockAct;

pub const SOUND: &str = "stopped.aiff";
pub const ALLOW_GRACE_MS: i64 = 1_000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BannerInfo {
    pub title: String,
    pub detail: String,
    pub who: String,
    pub thread: Option<String>,
}

pub fn banner_id(key: &BlockingTarget) -> String {
    match key {
        BlockingTarget::Item(id) => format!("inbox:{id}"),
        BlockingTarget::HookPrompt(id) => format!("hook:{id}"),
    }
}

pub fn key_of(id: &str) -> Option<BlockingTarget> {
    if let Some(rest) = id.strip_prefix("inbox:") {
        return Some(BlockingTarget::Item(rest.to_owned()));
    }
    id.strip_prefix("hook:")
        .map(|rest| BlockingTarget::HookPrompt(rest.to_owned()))
}

pub fn category_for(actions: &[AlertAction]) -> Option<Category> {
    let has = |a: AlertAction| actions.contains(&a);
    if has(AlertAction::Allow) {
        Some(Category::InboxAllow)
    } else if has(AlertAction::OpenForm) {
        Some(Category::SecretRequest)
    } else if has(AlertAction::Reply) {
        Some(Category::Ask)
    } else if has(AlertAction::Open) && has(AlertAction::Change) {
        Some(Category::InboxOpen)
    } else if has(AlertAction::Open) {
        Some(Category::InboxOpenOnly)
    } else {
        None
    }
}

pub fn action_title(action: BannerAction) -> String {
    match action {
        BannerAction::Allow => t::consent::banner_allow(),
        BannerAction::Deny => t::consent::banner_deny(),
        BannerAction::Change => t::consent::banner_change(),
        BannerAction::Open => t::consent::banner_open(),
        BannerAction::Reply => t::consent::banner_reply(),
        BannerAction::Dismiss => t::consent::banner_dismiss(),
        BannerAction::OpenForm => t::consent::banner_open_form(),
        BannerAction::Cancel => t::consent::banner_cancel(),
    }
    .to_owned()
}

pub fn notification_for(
    target: &Target,
    info: &BannerInfo,
    step: BannerStep,
    sound: bool,
    now_ms: i64,
) -> Notification {
    let waited = age(now_ms, target.waiting_since_ms.min(now_ms));
    let (title, body) = match step {
        BannerStep::Start => (info.title.clone(), info.detail.clone()),
        BannerStep::Pulse => (info.title.clone(), t::inbox::banner_pulse_body(&waited)),
        BannerStep::Stronger => (
            t::inbox::banner_stronger_title(&info.who, &waited),
            info.title.clone(),
        ),
        BannerStep::LastCall => (
            t::inbox::banner_last_call_title(&info.title),
            info.detail.clone(),
        ),
        BannerStep::Expired => (
            t::inbox::banner_expired_title(&info.title),
            info.detail.clone(),
        ),
    };
    let category = if step == BannerStep::Expired {
        None
    } else {
        category_for(&target.alert_actions)
    };
    Notification {
        title,
        body,
        category,
        thread: info.thread.clone(),
        sound: sound.then(|| SOUND.to_owned()),
    }
}

fn is_open_blocking(item: &Item) -> bool {
    item.state == State::Open && item.is_blocking()
}

pub fn targets_of(model: &InboxModel) -> Vec<(Target, BannerInfo)> {
    let mut found: Vec<&Item> = model
        .items()
        .iter()
        .filter(|item| is_open_blocking(item))
        .collect();
    found.sort_by(|a, b| a.created_ms.cmp(&b.created_ms).then(a.id.cmp(&b.id)));
    found
        .into_iter()
        .map(|item| {
            let (title, detail) = words_of(item);
            let place = item.place.as_ref();
            let who = place
                .and_then(|p| p.session_name.clone().or_else(|| p.machine_name.clone()))
                .unwrap_or_else(|| t::inbox::banner_who_alone().to_owned());
            let thread = place.and_then(|p| p.session.clone());
            let target = Target {
                key: BlockingTarget::Item(item.id.clone()),
                title: title.clone(),
                waiting_since_ms: item.waiting_since.unwrap_or(item.created_ms),
                expires_ms: item.expires_ms,
                alert_actions: item.alert_actions.clone(),
            };
            (
                target,
                BannerInfo {
                    title,
                    detail,
                    who,
                    thread,
                },
            )
        })
        .collect()
}

pub fn answerable(item: &Item) -> bool {
    item.state == State::Open
}

pub fn allow_refused(result: &std::result::Result<bool, String>) -> bool {
    !matches!(result, Ok(true))
}

pub fn closures_of(model: &InboxModel) -> (Vec<BlockingTarget>, Vec<BlockingTarget>) {
    let mut closed = Vec::new();
    let mut expired = Vec::new();
    for item in model
        .items()
        .iter()
        .filter(|item| item.state != State::Open)
    {
        let key = BlockingTarget::Item(item.id.clone());
        if item.state == State::Expired {
            expired.push(key);
        } else {
            closed.push(key);
        }
    }
    (closed, expired)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Act(BlockAct, String),
    OpenInbox(String),
    Ignore,
}

pub fn route(response: &Response, posted_ms: Option<i64>, now_ms: i64) -> Route {
    let Some(BlockingTarget::Item(id)) = key_of(&response.id) else {
        return Route::Ignore;
    };
    match response.action {
        Some(BannerAction::Allow) => {
            let early = posted_ms.is_some_and(|at| now_ms - at < ALLOW_GRACE_MS);
            if early {
                Route::OpenInbox(id)
            } else {
                Route::Act(BlockAct::Allow, id)
            }
        }
        Some(BannerAction::Deny | BannerAction::Cancel | BannerAction::Dismiss) => {
            Route::Act(BlockAct::Deny, id)
        }
        Some(
            BannerAction::Open
            | BannerAction::Change
            | BannerAction::Reply
            | BannerAction::OpenForm,
        )
        | None => Route::OpenInbox(id),
    }
}

enum Job {
    Upsert(String, Notification),
    Remove(String),
    Refresh,
}

#[derive(Clone, Copy)]
struct Cache {
    authorization: Authorization,
}

pub struct Detached {
    sender: Option<Sender<Job>>,
    cache: Arc<Mutex<(Cache, Vec<String>)>>,
    ready: Arc<AtomicBool>,
}

impl Detached {
    pub fn new(inner: Arc<dyn Notifier>) -> Detached {
        let (sender, receiver) = mpsc::channel::<Job>();
        let cache = Arc::new(Mutex::new((
            Cache {
                authorization: Authorization::Allowed,
            },
            Vec::new(),
        )));
        let held = cache.clone();
        let ready = Arc::new(AtomicBool::new(false));
        let flag = ready.clone();
        let spawned = std::thread::Builder::new()
            .name("notchling-inbox-banner".into())
            .spawn(move || {
                for job in receiver {
                    match job {
                        Job::Upsert(id, notification) => {
                            let _ = inner.upsert(&id, &notification);
                        }
                        Job::Remove(id) => {
                            let _ = inner.remove(&id);
                        }
                        Job::Refresh => {
                            let authorization = inner.authorization();
                            let delivered = inner.delivered();
                            let mut state = held.lock().unwrap_or_else(|e| e.into_inner());
                            *state = (Cache { authorization }, delivered);
                            flag.store(true, Ordering::Release);
                        }
                    }
                }
            });
        Detached {
            sender: spawned.ok().map(|_| sender),
            cache,
            ready,
        }
    }

    pub fn ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    pub fn refresh(&self) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(Job::Refresh);
        }
    }

    fn send(&self, job: Job) -> Result<()> {
        let Some(sender) = &self.sender else {
            return Err(Error::System("banner_thread_missing".into()));
        };
        sender.send(job).map_err(|e| Error::System(e.to_string()))
    }
}

impl Notifier for Detached {
    fn upsert(&self, id: &str, notification: &Notification) -> Result<()> {
        self.send(Job::Upsert(id.to_owned(), notification.clone()))
    }

    fn remove(&self, id: &str) -> Result<()> {
        self.send(Job::Remove(id.to_owned()))
    }

    fn request_authorization(&self) -> Result<()> {
        Ok(())
    }

    fn register_categories(&self, _title_of: &dyn Fn(BannerAction) -> String) -> Result<()> {
        Ok(())
    }

    fn authorization(&self) -> Authorization {
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .0
            .authorization
    }

    fn delivered(&self) -> Vec<String> {
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .clone()
    }
}

#[cfg(test)]
#[path = "banners_tests.rs"]
mod tests;
