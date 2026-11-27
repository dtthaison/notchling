use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::App;
use notchling_engine::settings::Settings;
use notchling_platform::notify::{system_notifier, Notifier, Response};

use super::shell;
use super::tray::reduce_motion;
use crate::attention::{tray_still, Effect, Level, Warning};
use crate::inbox::banners::{action_title, Route};
use crate::inbox::driver::{Driver, Surroundings};
use crate::inbox::feed::InboxFeed;
use crate::inbox::model::InboxModel;
use crate::inbox::tray_rows::{attention, Attention};
use crate::phone::bridge::now_ms;
use crate::session_source::SessionSource;

const TICK: Duration = Duration::from_millis(500);
const RECOUNT_EVERY: u32 = 20;
const SETTINGS_EVERY: u32 = 2;
const REFRESH_EVERY: u32 = 20;
const SECOND_TONE: Duration = Duration::from_millis(650);

thread_local! {
    static BOUNCE: RefCell<Vec<isize>> = const { RefCell::new(Vec::new()) };
    static GUARD: std::cell::RefCell<Option<notchling_platform::notify::DelegateGuard>> =
        const { std::cell::RefCell::new(None) };
}

fn level_from(data_dir: Option<&PathBuf>) -> (Level, bool) {
    let settings = data_dir.and_then(|dir| Settings::open(&dir.join("settings.json")).ok());
    let level = settings
        .as_ref()
        .and_then(|s| s.get_str("attentionLevel"))
        .and_then(|v| Level::parse(&v))
        .unwrap_or_default();
    let shown = settings
        .as_ref()
        .and_then(|s| s.get_bool("hideNotch"))
        .map(|hidden| !hidden)
        .unwrap_or(true);
    (level, shown)
}

fn prepare(system: Arc<dyn Notifier>) {
    std::thread::Builder::new()
        .name("notchling-banner-setup".into())
        .spawn(move || {
            let _ = system.request_authorization();
            let _ = system.register_categories(&action_title);
        })
        .ok();
}

fn listen() -> Receiver<Response> {
    let (sender, receiver) = mpsc::channel::<Response>();
    let sender = Mutex::new(sender);
    let handler: notchling_platform::notify::ResponseHandler = Arc::new(move |response| {
        let _ = sender.lock().map(|s| s.send(response));
    });
    GUARD.with(|guard| {
        *guard.borrow_mut() = notchling_platform::notify::install_delegate(handler);
    });
    receiver
}

fn apply_ui(cx: &mut App, effects: Vec<Effect>, warned: &mut bool) {
    for effect in effects {
        match effect {
            Effect::PulseTray => shell::pulse_tray(cx),
            Effect::DockBounce { critical } => {
                if let Some(request) = super::dock::bounce(critical) {
                    BOUNCE.with(|held| held.borrow_mut().push(request));
                }
            }
            Effect::CancelBounce => {
                let held = BOUNCE.with(|held| std::mem::take(&mut *held.borrow_mut()));
                for request in held {
                    super::dock::cancel_bounce(request);
                }
            }
            Effect::Warn(Warning::NoChannel) => *warned = true,
            Effect::Tone { times } if times > 1 => {
                for _ in 1..times {
                    cx.spawn(async move |cx| {
                        cx.background_executor().timer(SECOND_TONE).await;
                        cx.update(|_| {
                            super::sound::play();
                        })
                        .ok();
                    })
                    .detach();
                }
            }
            _ => shell::on_notch_effect(cx, &effect),
        }
    }
}

fn handle(cx: &mut App, driver: &Mutex<Driver>, response: &Response) {
    let route = driver
        .lock()
        .map(|d| d.route(response, now_ms()))
        .unwrap_or(Route::Ignore);
    match route {
        Route::Act(act, id) => shell::blocking_act(act, &id, cx).detach(),
        Route::OpenInbox(id) => {
            shell::open_chat_inbox_at(cx, &id);
            cx.activate(true);
        }
        Route::Ignore => {}
    }
}

pub fn watch(
    cx: &mut App,
    inbox: Arc<InboxFeed>,
    sessions: Arc<SessionSource>,
    data_dir: Option<PathBuf>,
) {
    let system: Arc<dyn Notifier> = Arc::from(system_notifier());
    prepare(system.clone());
    let banners = Arc::new(crate::inbox::banners::Detached::new(system));
    let responses = listen();
    let driver = Arc::new(Mutex::new(Driver::new()));
    cx.spawn(async move |cx| {
        let mut seen: Option<u64> = None;
        let mut ticks = 0u32;
        let mut last: Option<Attention> = None;
        let mut model = InboxModel::default();
        let mut setting = level_from(data_dir.as_ref());
        loop {
            let generation = inbox.generation();
            if seen != Some(generation) || ticks.is_multiple_of(RECOUNT_EVERY) {
                seen = Some(generation);
                model = inbox.snapshot(now_ms());
                last = Some(attention(&model, now_ms()));
            }
            if ticks.is_multiple_of(SETTINGS_EVERY) {
                setting = level_from(data_dir.as_ref());
            }
            if ticks.is_multiple_of(REFRESH_EVERY) {
                banners.refresh();
            }
            ticks = ticks.wrapping_add(1);
            let working = sessions.working();
            let around = Surroundings {
                level: setting.0,
                notch_shown: setting.1,
                reduce_motion: reduce_motion(),
                app_active: super::dock::app_active(),
            };
            let pending: Vec<Response> = responses.try_iter().collect();
            let driver = driver.clone();
            let banners = banners.clone();
            let found = last.clone();
            let model_now = model.clone();
            let update = cx.update(move |cx| {
                for response in &pending {
                    handle(cx, &driver, response);
                }
                if let Some(found) = found.as_ref() {
                    shell::on_attention(
                        cx,
                        found,
                        working,
                        tray_still(around.level, around.reduce_motion),
                    );
                }
                if !banners.ready() {
                    return;
                }
                let effects = driver
                    .lock()
                    .map(|mut d| d.run(&model_now, &around, &*banners, now_ms()))
                    .unwrap_or_default();
                let denied =
                    banners.authorization() == notchling_platform::notify::Authorization::Denied;
                crate::sheets::settings_attention::set_denied(denied);
                let mut warned = false;
                apply_ui(cx, effects, &mut warned);
                crate::inbox::warning::update(warned, denied, around.notch_shown);
            });
            if update.is_err() {
                return;
            }
            cx.background_executor().timer(TICK).await;
        }
    })
    .detach();
}
