use std::path::Path;
use std::sync::Arc;

use gpui::{prelude::*, AnyView, App, Window};
use notchling_kit::ui::KitGallery;
use notchling_kit::Appearance;

use super::shell::AppAccounts;
use super::window::ShellView;
use crate::accounts::renders::{self, Shown};
use crate::accounts::store::{Accounts, Snapshot};
use crate::accounts::wire::Lane;
use crate::chat::data::DataRoot;
use crate::chat::view::ChatView;

pub fn build(
    name: &str,
    data_root: Option<&Path>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyView> {
    let (kind, appearance) = match name.rsplit_once('-') {
        Some((kind, suffix)) if Appearance::parse(suffix).is_some() => {
            (kind, Appearance::parse(suffix))
        }
        _ => (name, None),
    };
    match kind {
        "kit-gallery" => Some(cx.new(|cx| KitGallery::new(appearance, window, cx)).into()),
        "shell" => Some(cx.new(|cx| ShellView::new(appearance, window, cx)).into()),
        "chat" => {
            let root = DataRoot::for_render(data_root);
            cx.set_global(AppAccounts(Arc::new(Accounts::fixed(chat_accounts(&root)))));
            Some(cx.new(|cx| ChatView::new(root, window, cx)).into())
        }
        notch if notch.starts_with("notch-") => crate::notch::backdrop::build_render(notch, cx),
        other => crate::chat::view::list_probe::build_render(other, cx)
            .or_else(|| crate::phone::view::build_render(other, cx))
            .or_else(|| crate::phone::prompt_view::build_render(other, cx))
            .or_else(|| crate::sheets::view::build_render(other, cx))
            .or_else(|| crate::search::view::build_render(other, cx))
            .or_else(|| crate::pages::view::build_render(other, cx))
            .or_else(|| crate::tools::view::build_render(other, cx))
            .or_else(|| crate::accounts::window::build_render(other, cx))
            .or_else(|| crate::vault::view::build_render(other, cx))
            .or_else(|| crate::inbox::view::build_render(other, cx))
            .or_else(|| crate::welcome::view::build_render(other, cx))
            .or_else(|| crate::switches::restart_view::build_render(other, cx))
            .or_else(|| crate::machines::view::build_render(other, cx)),
    }
}

fn chat_accounts(root: &DataRoot) -> Snapshot {
    let mut snapshot = renders::snapshot(Shown::Claude);
    let lanes = notchling_engine::sessions::lanes::read_lanes(&root.data_dir.join("lanes.json"));
    if !lanes.is_empty() {
        snapshot.lanes = lanes
            .into_iter()
            .map(|l| Lane {
                name: l.name,
                path: l.path,
                account_id: Some(l.account_id),
                email: Some(l.email),
            })
            .collect();
    }
    snapshot
}
