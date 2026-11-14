use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::App;
use serde_json::Value;

use super::shell::chat_window;
use crate::switches::feed::SwitchFeed;
use crate::switches::settings_changed::current;

const TICK: Duration = Duration::from_millis(200);

pub fn watch(cx: &mut App, feed: Arc<SwitchFeed>, data_dir: PathBuf) {
    cx.spawn(async move |cx| loop {
        cx.background_executor().timer(TICK).await;
        let keys = feed.take_settings();
        if keys.is_empty() {
            continue;
        }
        let dir = data_dir.clone();
        let changes = cx
            .background_executor()
            .spawn(async move { current(&dir, &keys) })
            .await;
        if cx.update(|cx| apply(cx, &changes)).is_err() {
            return;
        }
    })
    .detach();
}

pub fn app_wide(key: &str, value: &Value, cx: &mut App) {
    super::appearance::on_change(key, value, cx);
    super::shell::on_search_setting(cx, key, value);
    super::shell::on_awake_setting(cx, key, value);
}

pub fn apply(cx: &mut App, changes: &[(String, Value)]) {
    for (key, value) in changes {
        match chat_window(cx) {
            Some(handle) => {
                handle
                    .update(cx, |view, _, cx| {
                        view.applied_setting(key, value, cx);
                        view.show_setting(key, value, cx);
                    })
                    .ok();
            }
            None => {
                crate::chat::history::on_change(key, value);
                app_wide(key, value, cx);
            }
        }
    }
}
