use std::sync::Arc;

use gpui::{AnyView, AppContext};

use super::copy_name;
use super::form::Form;
use super::renders::{self, Shown};
use super::reveal::Part;
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
        Shown::Fields | Shown::Revealed => {
            let vault = Arc::new(
                Vault::fixed(renders::entries()).with_reader(Arc::new(renders::FakeReader)),
            );
            let revealed = which == Shown::Revealed;
            cx.new(|cx| {
                let mut sheet =
                    VaultSheetView::fixed(vault, feed, cluster, None, renders::TODAY, cx);
                let revision = |name: &str| {
                    renders::entries()
                        .iter()
                        .find(|e| e.name == name)
                        .map(|e| e.revision())
                        .unwrap_or_default()
                };
                sheet.usernames.insert(
                    "registry".into(),
                    (revision("registry"), Some(renders::FAKE_USER.into())),
                );
                if revealed {
                    sheet.revealed.show(
                        "github",
                        Part::Secret,
                        &revision("github"),
                        renders::FAKE_SECRET.into(),
                        copy_name::now_ms(),
                    );
                }
                sheet
            })
            .into()
        }
        Shown::Form => {
            let form = Form::edit(&renders::entries()[0]);
            cx.new(|cx| VaultSheetView::fixed(vault, feed, cluster, Some(form), renders::TODAY, cx))
                .into()
        }
        Shown::FormLines => {
            let mut form = Form::add();
            form.set_text(super::form::Field::Name, "deploy-key");
            form.set_text(super::form::Field::Secret, renders::FAKE_KEY);
            form.focus = super::form::Field::Secret;
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
