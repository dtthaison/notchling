//! Chat acting on sessions (Milestone 5's app side): the reply box sends `session.reply`,
//! `codex.send` or `agy.send` by the row's tool, Stop sends `session.interrupt`, `codex.stop` or
//! `agy.cancel`, the pickers send nothing (Codex) or `agy.set_option`, and an Antigravity
//! permission is answered with `agy.permission`. An Antigravity row's conversation comes from the
//! daemon (`agy.conversation`), fetched again whenever the `tools` topic says it changed. Every
//! control follows `Acting::control`; nothing is sent while acting isn't on.

use std::collections::{HashMap, HashSet};

use gpui::{Context, FocusHandle};
use notchling_engine::tools::agy::ConfigOption;
use notchling_engine::tools::codex::models;
use serde_json::Value;

use super::ChatView;
use crate::acting::{Acting, Control, Request};
use crate::chat::data::Tool;
use crate::phone::bridge::now_ms;
use crate::tools::agy_options::parse_config;
use crate::tools::agy_permission::AgyPermission;
use crate::tools::codex_picker::CodexPicker;
use crate::tools::live::ToolLive;
use crate::tools::open_gate::OpenGate;
use crate::tools::requests::{
    agy_cancel, agy_conversation, agy_open, agy_permission, agy_send, agy_send_draft,
    agy_set_option, agy_state, codex_send, codex_send_draft, codex_stop, is_draft,
    parse_conversation, sent_row, sent_state, AgyConversation, Pick, Sent,
};

/// Why a Claude Code row that isn't a background job takes no reply.
pub const READ_ONLY: &str = "Read only here. Reply where the session runs.";
/// Antigravity takes one turn at a time (Codex queues; the daemon says so).
pub const AGY_BUSY: &str = "Antigravity is still on the last turn. Send again when it ends.";
pub const EXITED: &str = "This session has exited.";

/// A pop-up over the composer.
pub enum Picker {
    Codex(CodexPicker),
    Agy,
}

/// What Chat keeps about acting on the other tools.
pub struct ChatTools {
    /// What the `tools` topic last said, from the session source.
    pub live: ToolLive,
    /// The reply box's text per row, so switching sessions keeps what was typed.
    pub drafts: HashMap<String, String>,
    pub sending: bool,
    pub error: Option<String>,
    /// Each Codex row's model and effort for its next turns (kept for this run).
    pub picks: HashMap<String, Pick>,
    /// The selected Antigravity row's conversation and state from the daemon.
    pub agy: Option<AgyConversation>,
    /// Which row and change count the last fetch was for.
    pub agy_fetched: Option<(String, u64)>,
    pub agy_config: Vec<ConfigOption>,
    /// Which Antigravity rows this Chat asked the daemon to open, and when to ask again (the
    /// daemon closes a session after ten idle minutes).
    pub opens: OpenGate,
    /// Messages the daemon queued behind a Codex turn, shown until that turn ends.
    pub queued: HashMap<String, Vec<String>>,
    /// Antigravity prompts answered here, hidden until the daemon resolves them.
    pub answered: HashSet<String>,
    pub picker: Option<Picker>,
    pub focus: FocusHandle,
}

impl ChatTools {
    pub fn new(cx: &mut Context<ChatView>) -> ChatTools {
        ChatTools {
            live: ToolLive::default(),
            drafts: HashMap::new(),
            sending: false,
            error: None,
            picks: HashMap::new(),
            agy: None,
            agy_fetched: None,
            agy_config: Vec::new(),
            opens: OpenGate::default(),
            queued: HashMap::new(),
            answered: HashSet::new(),
            picker: None,
            focus: cx.focus_handle(),
        }
    }

    /// The box's text for a row.
    pub fn draft(&self, id: &str) -> &str {
        self.drafts.get(id).map(String::as_str).unwrap_or("")
    }
}

impl ChatView {
    pub(super) fn acting(&self, cx: &mut Context<Self>) -> Acting {
        self.acts(cx).acting()
    }

    /// The reply box's gate: acting, then what the row takes.
    pub(super) fn composer_control(&self, cx: &mut Context<Self>) -> Control {
        let gate = self.acting(cx).control();
        let Some(session) = self.selected_session() else {
            return Control {
                enabled: false,
                tooltip: None,
            };
        };
        let off = |why: &'static str| Control {
            enabled: false,
            tooltip: Some(why),
        };
        match session.tool {
            Tool::Claude if session.agent.label == "Exited" => off(EXITED),
            Tool::Claude if !session.agent.can_reply() => off(READ_ONLY),
            Tool::Antigravity if gate.enabled && self.tools.live.is_working(&session.agent.id) => {
                off(AGY_BUSY)
            }
            _ => gate,
        }
    }

    /// Whether the selected row's turn runs, so Stop shows.
    pub(super) fn selected_working(&self) -> bool {
        self.selected_session().is_some_and(|s| {
            self.tools.live.is_working(&s.agent.id)
                || (s.tool == Tool::Claude
                    && s.agent.group == notchling_engine::sessions::AgentGroup::Working)
        })
    }

    /// Sends `request` off the UI thread; `done` takes the answer.
    fn spawn_send(
        &mut self,
        request: Request,
        done: impl FnOnce(&mut ChatView, Result<Value, String>, &mut Context<ChatView>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let actions = self.acts(cx).actions;
        self.tools.sending = true;
        self.tools.error = None;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { actions.send(&request) })
                .await;
            this.update(cx, |view, cx| {
                view.tools.sending = false;
                done(view, result, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Return in the reply box: the text goes to the row's tool.
    pub(super) fn send_text(&mut self, cx: &mut Context<Self>) {
        if self.tools.sending || !self.composer_control(cx).enabled {
            return;
        }
        let Some(session) = self.selected_session().cloned() else {
            return;
        };
        let id = session.agent.id.clone();
        let text = self.tools.draft(&id).trim().to_owned();
        if text.is_empty() {
            return;
        }
        let draft = is_draft(&id);
        let request = match session.tool {
            Tool::Claude => Request::reply(&id, &text),
            Tool::Codex => {
                let pick = self.tools.picks.get(&id).cloned().unwrap_or_default();
                if draft {
                    codex_send_draft(&session.agent.cwd, &text, &pick)
                } else {
                    codex_send(&id, &text, &pick)
                }
            }
            Tool::Antigravity => {
                if draft {
                    agy_send_draft(&session.agent.cwd, &text)
                } else {
                    agy_send(&id, &text)
                }
            }
        };
        self.tools.drafts.remove(&id);
        let sent_text = text;
        self.spawn_send(
            request,
            move |view, result, cx| match result {
                Ok(reply) => {
                    if session.tool == Tool::Codex {
                        if let Sent::Queued(_) = sent_state(&reply) {
                            view.tools
                                .queued
                                .entry(id.clone())
                                .or_default()
                                .push(sent_text.clone());
                        }
                    }
                    // A draft's first message names the real row: follow it.
                    if let Some(row) = sent_row(&reply).filter(|row| *row != id) {
                        if let Some(pick) = view.tools.picks.remove(&id) {
                            view.tools.picks.insert(row.clone(), pick);
                        }
                        view.selected = Some(row);
                        view.loading = true;
                    }
                    view.reload_now(cx);
                }
                Err(message) => {
                    // Back into the box, in front of anything typed since.
                    let typed = view.tools.drafts.remove(&id).unwrap_or_default();
                    let back = if typed.is_empty() {
                        sent_text.clone()
                    } else {
                        format!("{sent_text}\n\n{typed}")
                    };
                    view.tools.drafts.insert(id.clone(), back);
                    view.tools.error = Some(message);
                }
            },
            cx,
        );
    }

    /// Stop: the row's tool is asked to end its turn.
    pub(super) fn stop(&mut self, cx: &mut Context<Self>) {
        if self.tools.sending || !self.acting(cx).control().enabled {
            return;
        }
        let Some(session) = self.selected_session() else {
            return;
        };
        let id = session.agent.id.clone();
        let request = match session.tool {
            Tool::Claude => Request::interrupt(&id),
            Tool::Codex => codex_stop(&id),
            Tool::Antigravity => agy_cancel(&id),
        };
        self.spawn_send(
            request,
            |view, result, _| {
                if let Err(message) = result {
                    view.tools.error = Some(message);
                }
            },
            cx,
        );
    }

    /// The Antigravity options picker's pick.
    pub(super) fn set_agy_option(&mut self, option: &str, value: &str, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        self.tools.picker = None;
        self.spawn_send(
            agy_set_option(&id, option, value),
            |view, result, _| {
                if let Err(message) = result {
                    view.tools.error = Some(format!("Couldn't change it: {message}"));
                }
            },
            cx,
        );
    }

    /// An option of the Antigravity permission card.
    pub(super) fn answer_agy(
        &mut self,
        prompt_id: String,
        rpc_id: String,
        option: String,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        self.spawn_send(
            agy_permission(&id, &rpc_id, Some(&option)),
            move |view, result, _| match result {
                Ok(_) => {
                    view.tools.answered.insert(prompt_id);
                }
                Err(message) => view.tools.error = Some(message),
            },
            cx,
        );
    }

    /// The Antigravity permission waiting on the selected row, from the daemon's held prompts.
    pub(super) fn agy_prompt(&self) -> Option<(String, AgyPermission)> {
        let id = self.selected.as_deref()?;
        let prompts = self.source.view().prompts?;
        prompts.iter().find_map(|p| {
            if p.get("sessionId").and_then(Value::as_str) != Some(id) {
                return None;
            }
            let prompt_id = p.get("id")?.as_str()?.to_owned();
            if self.tools.answered.contains(&prompt_id) {
                return None;
            }
            AgyPermission::from_prompt(p).map(|agy| (prompt_id, agy))
        })
    }

    /// Opens or closes the picker the selected row has: Codex's models, Antigravity's options.
    pub(super) fn toggle_picker(&mut self, cx: &mut Context<Self>) {
        if self.tools.picker.is_some() {
            self.tools.picker = None;
            cx.notify();
            return;
        }
        let Some(session) = self.selected_session() else {
            return;
        };
        let id = session.agent.id.clone();
        self.tools.picker = match session.tool {
            Tool::Codex => Some(Picker::Codex(CodexPicker::new(
                models::load(&self.root.codex_dir),
                self.tools.picks.get(&id).cloned().unwrap_or_default(),
                &self.conversation.facts,
            ))),
            Tool::Antigravity => Some(Picker::Agy),
            Tool::Claude => None,
        };
        cx.notify();
    }

    /// Keeps a Codex pick for the row.
    pub(super) fn keep_pick(&mut self, picker: &CodexPicker) {
        if let Some(id) = &self.selected {
            if picker.is_default() {
                self.tools.picks.remove(id);
            } else {
                self.tools.picks.insert(id.clone(), picker.pick.clone());
            }
        }
    }

    /// After a load or a tools event: the live state, and the selected Antigravity row's
    /// conversation from the daemon (asking it to open the session first while acting is on).
    pub(super) fn sync_tools(&mut self, cx: &mut Context<Self>) {
        let view = self.source.view();
        self.tools.live = view.tools;
        let Some(session) = self.selected_session().cloned() else {
            return;
        };
        let id = session.agent.id;
        for (row, queued) in self.tools.queued.iter_mut() {
            queued.truncate(self.tools.live.waiting(row));
        }
        if session.tool != Tool::Antigravity || is_draft(&id) {
            return;
        }
        let changes = self.tools.live.agy_changes.get(&id).copied().unwrap_or(0);
        let acting = self.acting(cx) == Acting::On;
        let asked = u64::try_from(now_ms()).unwrap_or(0);
        if self
            .tools
            .opens
            .should_open(&id, self.tools.live.is_open(&id), acting, changes, asked)
        {
            let opened = id.clone();
            self.spawn_send(
                agy_open(&id),
                move |view, result, _| {
                    if let Err(message) = result {
                        view.tools.opens.failed(&opened);
                        view.tools.error = Some(message);
                    }
                },
                cx,
            );
        }
        if self.tools.agy_fetched.as_ref() != Some(&(id.clone(), changes)) {
            self.tools.agy_fetched = Some((id.clone(), changes));
            self.fetch_agy(id, cx);
        }
    }

    /// Reads the row's conversation and options from the daemon, off the UI thread.
    fn fetch_agy(&mut self, id: String, cx: &mut Context<Self>) {
        let actions = self.acts(cx).actions;
        let asked = id.clone();
        cx.spawn(async move |this, cx| {
            let (conversation, state) = cx
                .background_executor()
                .spawn(async move {
                    let conversation = actions.read(&agy_conversation(&id, false)).ok();
                    let state = actions.read(&agy_state(&id)).ok();
                    (conversation, state)
                })
                .await;
            this.update(cx, |view, cx| {
                if view.selected.as_deref() != Some(asked.as_str()) {
                    return;
                }
                if let Some(c) = conversation.as_ref().and_then(parse_conversation) {
                    view.conversation = c.conversation.clone();
                    view.loading = false;
                    view.tools.agy = Some(c);
                }
                if let Some(state) = state {
                    view.tools.agy_config = parse_config(&state);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
