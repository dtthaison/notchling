use std::sync::Arc;

use gpui::{
    div, point, prelude::*, px, size, AnyView, App, Bounds, Context, Render, TitlebarOptions,
    Window, WindowBounds, WindowHandle, WindowKind, WindowOptions,
};
use notchling_kit::metric::SIDE_PADDING;
use notchling_kit::ui::{label, window_appearance, KitStyled};
use notchling_kit::{Ink, TypeRole};

use super::limits::{self, Zone};
use super::model::{Tab, CLAUDE_CODE, CLAUDE_DESKTOP, CODEX};
use super::renders::{self, Shown};
use super::store::Accounts;
use super::view::AccountsView;
use super::view_rows::{limit_groups, meter};
use crate::mac::shell::{AppAccounts, AppSwitchFeed};
use crate::switches::feed::SwitchFeed;

pub const WINDOW_SIZE: (f32, f32) = (560.0, 640.0);

pub fn app_stores(cx: &App) -> (Arc<Accounts>, Arc<SwitchFeed>) {
    let store = cx
        .try_global::<AppAccounts>()
        .map(|s| s.0.clone())
        .unwrap_or_else(|| Arc::new(Accounts::fixed(Default::default())));
    let feed = cx
        .try_global::<AppSwitchFeed>()
        .map(|f| f.0.clone())
        .unwrap_or_else(|| Arc::new(SwitchFeed::none()));
    (store, feed)
}

pub fn open(cx: &mut App, provider: Option<&str>) -> Option<WindowHandle<AccountsView>> {
    let open_one = cx
        .windows()
        .into_iter()
        .find_map(|w| w.downcast::<AccountsView>());
    if let Some(handle) = open_one {
        let shown = handle.update(cx, |view, window, cx| {
            if let Some(p) = provider {
                view.show_provider(p, cx);
            }
            window.activate_window();
        });
        if shown.is_ok() {
            return Some(handle);
        }
    }
    let (store, feed) = app_stores(cx);
    let bounds = Bounds::centered(None, size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Accounts".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.0), px(12.0))),
        }),
        window_min_size: Some(size(px(460.0), px(420.0))),
        kind: WindowKind::Normal,
        ..Default::default()
    };
    let provider = provider.map(str::to_owned);
    let handle = cx
        .open_window(options, |_, cx| {
            cx.new(|cx| {
                let mut view = AccountsView::live(store, feed, false, cx);
                if let Some(p) = &provider {
                    view.show_provider(p, cx);
                }
                view
            })
        })
        .map_err(|e| eprintln!("open the Accounts window: {e}"))
        .ok()?;
    crate::mac::shell::note_titled(cx, handle.window_id());
    Some(handle)
}

pub struct LimitBarsView;

impl Render for LimitBarsView {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let now = renders::NOW;
        let codex = limits::groups(&renders::codex_report(), now, Zone::UTC);
        let agy = limits::groups(&renders::antigravity_report(), now, Zone::UTC);
        let snapshot = renders::snapshot(Shown::Claude);
        let windows = snapshot
            .limits
            .providers
            .first()
            .map(|p| p.windows.clone())
            .unwrap_or_default();
        let meters = limits::claude_meters(&windows, now, Zone::UTC);
        let codex_meters = limits::codex_meters(&renders::codex_report(), now, Zone::UTC);
        div()
            .size_full()
            .p(px(SIDE_PADDING))
            .flex()
            .flex_col()
            .gap(px(16.0))
            .fill(Ink::Window, a)
            .child(label("Codex", TypeRole::Section, Ink::TextFaint, a))
            .child(limit_groups(&codex, a))
            .child(label("Antigravity", TypeRole::Section, Ink::TextFaint, a))
            .child(limit_groups(&agy, a))
            .child(label("Header meters", TypeRole::Section, Ink::TextFaint, a))
            .child(
                div()
                    .flex()
                    .gap(px(14.0))
                    .children(
                        meters.iter().enumerate().map(|(i, m)| {
                            meter(gpui::SharedString::from(format!("claude-{i}")), m, a)
                        }),
                    )
                    .children(
                        codex_meters.iter().enumerate().map(|(i, m)| {
                            meter(gpui::SharedString::from(format!("codex-{i}")), m, a)
                        }),
                    ),
            )
    }
}

pub fn build_render(name: &str, cx: &mut App) -> Option<AnyView> {
    let which = renders::shown(name)?;
    let store = Arc::new(Accounts::fixed(renders::snapshot(which)));
    let feed = Arc::new(SwitchFeed::fixed(renders::switches(which)));
    Some(match which {
        Shown::LimitBars => cx.new(|_| LimitBarsView).into(),
        Shown::Cost => cx
            .new(|_| AccountsView::fixed(store, feed, CLAUDE_CODE, Tab::TokenCost, renders::NOW))
            .into(),
        Shown::Codex => cx
            .new(|_| AccountsView::fixed(store, feed, CODEX, Tab::Accounts, renders::NOW))
            .into(),
        Shown::Claude | Shown::Off => {
            let menu = name == renders::MENU_RENDER;
            let tab = if menu { CLAUDE_DESKTOP } else { CLAUDE_CODE };
            let mut view = AccountsView::fixed(store, feed, tab, Tab::Accounts, renders::NOW);
            if menu {
                view.open_menu(&format!("{CLAUDE_DESKTOP}-a1"));
            }
            cx.new(|_| view).into()
        }
    })
}
