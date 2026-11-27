use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{App, Global};
use notchling_engine::control::RealLauncher;
use notchling_engine::settings::Settings;

use super::shell;
use crate::chat::data::DataRoot;
use crate::welcome::about::{build_id, current};
use crate::welcome::updates::{
    self, Attempt, CurlFetcher, FeedFetcher, Inputs, Outcome, KILL_SWITCH, SETTING_KEY, TICK_SECS,
};

#[derive(Default)]
struct Latest(Option<Outcome>);

impl Global for Latest {}

pub fn latest(cx: &App) -> Option<Outcome> {
    cx.try_global::<Latest>().and_then(|l| l.0.clone())
}

pub fn publish(cx: &mut App, outcome: &Outcome) {
    if latest(cx).as_ref() == Some(outcome) {
        return;
    }
    cx.set_global(Latest(Some(outcome.clone())));
    let version = match outcome {
        Outcome::Available(release) => Some(release.version.to_string()),
        _ => None,
    };
    shell::on_update(cx, version);
}

pub fn open_release_page(cx: &mut App) {
    if let Some(Outcome::Available(release)) = latest(cx) {
        cx.open_url(&release.page());
    }
}

fn setting_on(file: &Path) -> bool {
    Settings::open(file)
        .ok()
        .and_then(|s| s.get_bool(SETTING_KEY))
        .unwrap_or(true)
}

pub fn fetcher_for(root: &DataRoot) -> Arc<dyn FeedFetcher> {
    Arc::new(CurlFetcher::new(RealLauncher::new(&root.home)))
}

pub fn watch(cx: &mut App, root: &DataRoot) {
    let file = root.data_dir.join("settings.json");
    let fetcher = fetcher_for(root);
    let switch = std::env::var(KILL_SWITCH).ok();
    let started = Instant::now();
    cx.spawn(async move |cx| {
        let mut last: Option<Attempt> = None;
        loop {
            let (file, fetcher, switch) = (file.clone(), fetcher.clone(), switch.clone());
            let now = started.elapsed().as_secs();
            let (outcome, next) = cx
                .background_executor()
                .spawn(async move {
                    let version = current().version;
                    let inputs = Inputs {
                        version: &version,
                        build: build_id(),
                        switch: switch.as_deref(),
                        setting: setting_on(&file),
                    };
                    let mut next = last;
                    let outcome = updates::tick(&mut next, now, &inputs, fetcher.as_ref());
                    (outcome, next)
                })
                .await;
            last = next;
            if let Some(outcome) = outcome {
                if cx.update(|cx| publish(cx, &outcome)).is_err() {
                    return;
                }
            }
            cx.background_executor()
                .timer(Duration::from_secs(TICK_SECS))
                .await;
        }
    })
    .detach();
}
