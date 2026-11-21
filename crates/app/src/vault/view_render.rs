use std::sync::Arc;

use gpui::{AnyView, AppContext};

use super::form::Form;
use super::renders::{self, Shown};
use super::store::Vault;
use super::view::VaultSheetView;
use crate::cluster::feed::ClusterFeed;
use crate::switches::feed::SwitchFeed;
use crate::switches::Switches;

pub fn build_render(name: &str, cx: &mut gpui::App) -> Option<AnyView> {
    use super::approval_view::VaultApprovalView;
    use super::prompt_view::VaultRequestView;
    let which = renders::shown(name)?;
    let vault = Arc::new(Vault::fixed(renders::entries()));
    let feed = Arc::new(SwitchFeed::fixed(Switches::all_on()));
    let cluster = Arc::new(ClusterFeed::none());
    Some(match which {
        Shown::List => cx
            .new(|cx| VaultSheetView::fixed(vault, feed, cluster, None, renders::TODAY, cx))
            .into(),
        Shown::Form => {
            let form = Form::edit(&renders::entries()[0]);
            cx.new(|cx| VaultSheetView::fixed(vault, feed, cluster, Some(form), renders::TODAY, cx))
                .into()
        }
        Shown::Remote => {
            let cluster = Arc::new(crate::cluster::renders::feed_with_remote_vault(
                renders::entries(),
            ));
            cx.new(|cx| {
                let mut sheet =
                    VaultSheetView::fixed(vault, feed, cluster, None, renders::TODAY, cx);
                sheet.remote_tab = true;
                sheet
            })
            .into()
        }
        Shown::Approval => {
            let approval = super::approval::pending(&renders::approvals())
                .into_iter()
                .next()?;
            cx.new(|cx| VaultApprovalView::fixed(&approval, cx)).into()
        }
        Shown::RequestRefused => cx
            .new(|cx| {
                VaultRequestView::fixed(renders::pending_request(), renders::TODAY, cx)
                    .refused(super::consent_save::Refusal::Changed)
            })
            .into(),
        Shown::Request => cx
            .new(|cx| VaultRequestView::fixed(renders::pending_request(), renders::TODAY, cx))
            .into(),
    })
}
