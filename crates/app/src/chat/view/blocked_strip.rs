use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_kit::labels::Corner;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, icon, label, KitPart};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::vault_cards::card_frame;
use super::ChatView;
use crate::chat::data::{Session, Tool};
use crate::inbox::renders_b2::blocked_for;
use crate::inbox::triage::blocking_for;

pub const BLOCKED_ENV: &str = "NOTCHLING_RENDER_BLOCKED";

pub(super) struct SessionKeys {
    pub node: Option<String>,
    pub ids: Vec<String>,
}

pub(super) fn session_keys(session: &Session) -> SessionKeys {
    let id = session.agent.id.as_str();
    let (node, local) = match id.split_once('/') {
        Some((node, rest)) if notchling_engine::cluster::NodeId::parse(node).is_some() => {
            (Some(node.to_owned()), rest)
        }
        _ => (None, id),
    };
    let mut ids = vec![id.to_owned()];
    let own_prefix = match session.tool {
        Tool::Codex => Some("codex:"),
        Tool::Antigravity => Some("agy:"),
        Tool::Claude => None,
    };
    if let Some(prefix) = own_prefix {
        if let Some(bare) = local.strip_prefix(prefix) {
            ids.push(bare.to_owned());
        }
    } else {
        ids.push(local.to_owned());
    }
    ids.extend(session.agent.session_id());
    if let Some(stem) = session
        .transcript
        .as_deref()
        .and_then(|path| path.file_stem())
        .and_then(|stem| stem.to_str())
    {
        ids.push(stem.to_owned());
    }
    ids.retain(|key| !key.is_empty());
    ids.sort();
    ids.dedup();
    SessionKeys { node, ids }
}

pub(super) fn strip(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let selected = view.selected_session()?;
    let keys = session_keys(selected);
    let names: Vec<&str> = keys.ids.iter().map(String::as_str).collect();
    let node = keys.node.as_deref();
    let blocked = if view.blocked_preview {
        blocking_for(&blocked_for(&selected.agent.id), node, &names)?
    } else {
        view.inbox.blocking_for(node, &names)?
    };
    let id = blocked.id.clone();
    let open = button(
        SharedString::from("blocked-open-card"),
        t::chat::blocked_open(),
        ButtonKind::Secondary,
        PartState::Rest,
        a,
    )
    .flex_shrink_0()
    .on_click(cx.listener(move |this, _, _, cx| this.show_inbox_at(&id, cx)));
    Some(
        card_frame(a)
            .flex()
            .items_start()
            .gap(px(8.0))
            .p(px(8.0))
            .child(icon(
                Icon::Warning,
                TypeRole::Label.spec().size,
                Ink::Needs,
                a,
            ))
            .child(div().flex_1().min_w(px(0.0)).child(label(
                &t::chat::blocked_strip(&blocked.title),
                TypeRole::Label,
                Ink::Text,
                a,
            )))
            .child(open)
            .part_at("Blocked Strip", Corner::TopTrailing, a, cx),
    )
}

#[cfg(test)]
mod tests {
    use notchling_engine::sessions::{AgentGroup, AgentTarget};

    use super::*;
    use crate::chat::sidebar::tests::session;

    fn keys(id: &str, tool: Tool) -> Vec<String> {
        session_keys(&session(id, AgentGroup::NeedsInput, None, tool)).ids
    }

    #[test]
    fn a_codex_row_is_found_by_the_tools_own_session_id() {
        assert!(keys("codex:019a-77", Tool::Codex).contains(&"019a-77".to_owned()));
    }

    #[test]
    fn an_agy_row_is_found_by_the_tools_own_session_id() {
        assert!(keys("agy:c0ffee", Tool::Antigravity).contains(&"c0ffee".to_owned()));
    }

    #[test]
    fn a_node_prefixed_row_is_found_by_the_bare_session_id() {
        let node = "00000000000000a1";
        assert!(keys(&format!("{node}/aa11bb22"), Tool::Claude).contains(&"aa11bb22".to_owned()));
        assert!(keys(&format!("{node}/codex:019a-77"), Tool::Codex).contains(&"019a-77".to_owned()));
    }

    #[test]
    fn a_claude_job_is_found_by_its_session_id_once_known_and_by_its_job_id_before() {
        let mut row = session("ab12", AgentGroup::NeedsInput, None, Tool::Claude);
        assert!(session_keys(&row).ids.contains(&"ab12".to_owned()));
        row.agent.target = AgentTarget::Resume {
            session_id: "1f2e-uuid".into(),
            cwd: "/tmp".into(),
        };
        let found = session_keys(&row).ids;
        assert!(found.contains(&"1f2e-uuid".to_owned()) && found.contains(&"ab12".to_owned()));
    }

    #[test]
    fn a_new_codex_row_has_no_session_id_to_match() {
        assert!(!keys("codex-new:1234", Tool::Codex).contains(&"1234".to_owned()));
    }

    #[test]
    fn a_remote_row_carries_its_node_and_a_local_row_has_none() {
        let node = "00000000000000a1";
        let remote = session_keys(&session(
            &format!("{node}/aa11"),
            AgentGroup::NeedsInput,
            None,
            Tool::Claude,
        ));
        assert_eq!(remote.node.as_deref(), Some(node));
        let local = session_keys(&session("aa11", AgentGroup::NeedsInput, None, Tool::Claude));
        assert_eq!(local.node, None);
    }

    #[test]
    fn a_prefix_is_stripped_only_for_its_own_tool() {
        assert!(!keys("codex:abc", Tool::Claude).contains(&"abc".to_owned()));
        assert!(!keys("agy:abc", Tool::Codex).contains(&"abc".to_owned()));
        assert!(!keys("codex:abc", Tool::Antigravity).contains(&"abc".to_owned()));
        assert!(keys("codex:abc", Tool::Codex).contains(&"abc".to_owned()));
    }
}
