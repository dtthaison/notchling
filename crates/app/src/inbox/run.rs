use gpui::Context;

use super::consent_card::{AccountApply, ClusterApply, ConsentCard};
use super::consent_family::{Admin, FamilyApply, Part};
use super::feed::SignIn;
use super::view::InboxPanelView;

#[derive(Clone)]
pub(super) enum Run {
    Plain,
    Switch(String),
    Cluster(ClusterApply),
    Account(AccountApply),
    Family(FamilyApply, Vec<Part>, Option<Admin>),
}

pub(super) fn run_of(card: &ConsentCard) -> Run {
    if let Some(switch) = &card.switch {
        Run::Switch(switch.clone())
    } else if let Some(apply) = &card.cluster_apply {
        Run::Cluster(apply.clone())
    } else if let Some(apply) = &card.account_apply {
        Run::Account(apply.clone())
    } else if let Some(apply) = &card.family_apply {
        Run::Family(apply.clone(), card.parts.clone(), None)
    } else {
        Run::Plain
    }
}

pub(super) enum Done {
    Plain,
    Form,
    Sign(SignIn),
    Note(Option<String>),
}

pub(super) fn open_sign_in(sign: SignIn, cx: &mut Context<InboxPanelView>) {
    match sign {
        SignIn::OpenUrl(url) => cx.open_url(&url),
        SignIn::OpenAccounts(provider) => {
            crate::accounts::window::open(cx, provider.as_deref());
        }
        SignIn::Nothing => {}
    }
}
