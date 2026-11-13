use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::{App, AppContext, Context, Entity, FocusHandle, Focusable, Global, Subscription};

#[derive(Clone, Default)]
struct SavedDrafts(HashMap<String, String>);

impl Global for SavedDrafts {}
use notchling_engine::catalog::{ModelInfo, Tool as CatalogTool};
use notchling_engine::tools::agy::ConfigOption;
use notchling_engine::tools::codex::models;
use serde_json::Value;

use super::ChatView;
use crate::acting::{Acting, Control, Request};
use crate::chat::claude_picker::{self, Hint, Kind};
use crate::chat::data::{Session, Tool};
use crate::chat::outgoing::Outgoing;
use crate::chat::prompts::is_dialog;
use crate::chat::run_command::RunHow;
use crate::phone::bridge::now_ms;
use crate::tools::agy_options::parse_config;
use crate::tools::agy_permission::{held_prompt, HeldPrompt};
use crate::tools::codex_picker::CodexPicker;
use crate::tools::fetch_gate::{Ask, FetchGate};
use crate::tools::live::{Follow, ToolLive};
use crate::tools::open_gate::OpenGate;
use crate::tools::requests::{
    agy_conversation, agy_open, agy_permission, agy_set_option, agy_state, is_draft,
    parse_conversation, sent_row, sent_state, AgyConversation, Pick, Sent,
};
use notchling_kit::ui::{TextEditor, TextEditorEvent};

const RETRY_AFTER_STOP_MS: u64 = 1_500;

pub const READ_ONLY: &str = "Read only here. Reply where the session runs.";
pub const AGY_BUSY: &str = "Antigravity is still on the last turn. Send again when it ends.";
pub const EXITED: &str = "This session has exited.";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Picker {
    pub menu: String,
    pub more: bool,
}

pub struct ChatTools {
    pub live: ToolLive,
    pub drafts: HashMap<String, String>,
    pub sending: bool,
    pub error: Option<String>,
    pub picks: HashMap<String, Pick>,
    pub agy: Option<AgyConversation>,
    pub agy_fetched: Option<(String, u64)>,
    pub fetches: FetchGate,
    pub agy_config: Vec<ConfigOption>,
    pub opens: OpenGate,
    pub queued: HashMap<String, Vec<String>>,
    pub outgoing: HashMap<String, Vec<Outgoing>>,
    pub starting: HashMap<String, String>,
    pub answered: HashSet<String>,
    pub picker: Option<Picker>,
    pub codex_models: Option<Vec<models::Model>>,
    pub model_hints: HashMap<String, Hint>,
    pub effort_hints: HashMap<String, Hint>,
    pub stopping: Option<(String, i64)>,
    pub retrying: Option<(String, i64)>,
    pub editor: Entity<TextEditor>,
    pub focus: FocusHandle,
    _editor_events: Subscription,
}

impl ChatTools {
    pub fn new(cx: &mut Context<ChatView>) -> ChatTools {
        let editor = cx.new(|cx| TextEditor::new("Reply…", super::composer::MAX_LINES, cx));
        let focus = editor.read(cx).focus_handle(cx);
        let events = cx.subscribe(
            &editor,
            |this, _, event: &TextEditorEvent, cx| match event {
                TextEditorEvent::Changed => this.draft_changed(cx),
                TextEditorEvent::Submit => this.send_text(cx),
            },
        );
        ChatTools {
            live: ToolLive::default(),
            drafts: cx
                .try_global::<SavedDrafts>()
                .map(|d| d.0.clone())
                .unwrap_or_default(),
            sending: false,
            error: None,
            picks: HashMap::new(),
            agy: None,
            agy_fetched: None,
            fetches: FetchGate::default(),
            agy_config: Vec::new(),
            opens: OpenGate::default(),
            queued: HashMap::new(),
            outgoing: HashMap::new(),
            starting: HashMap::new(),
            answered: HashSet::new(),
            picker: None,
            codex_models: None,
            model_hints: HashMap::new(),
            effort_hints: HashMap::new(),
            stopping: None,
            retrying: None,
            editor,
            focus,
            _editor_events: events,
        }
    }

    pub fn draft(&self, id: &str) -> &str {
        self.drafts.get(id).map(String::as_str).unwrap_or("")
    }

    fn delivered(&mut self, row: &str, text: &str, now_ms: i64) {
        if let Some(out) = self.outgoing.get_mut(row).and_then(|list| {
            list.iter_mut()
                .find(|o| o.text == text && o.delivered_at.is_none())
        }) {
            out.delivered(now_ms);
        }
    }

    fn take_outgoing(&mut self, row: &str, text: &str) {
        if let Some(list) = self.outgoing.get_mut(row) {
            if let Some(i) = list.iter().position(|o| o.text == text) {
                list.remove(i);
            }
        }
    }
}

impl ChatView {
    pub(super) fn outgoing_texts(&self) -> Vec<String> {
        let now = now_ms();
        self.selected
            .as_deref()
            .and_then(|row| self.tools.outgoing.get(row))
            .map(|list| {
                list.iter()
                    .filter(|o| o.keep(&self.conversation, now))
                    .map(|o| o.text.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn acting(&self, cx: &mut Context<Self>) -> Acting {
        self.acts(cx).acting()
    }

    pub(super) fn acting_in(&self, cx: &App) -> Acting {
        self.acts_in(cx).acting()
    }

    fn draft_changed(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let text = self.tools.editor.read(cx).text().to_owned();
        self.slash_typed(&text);
        self.save_draft(&id, &text, cx);
        if text.is_empty() {
            self.tools.drafts.remove(&id);
        } else {
            self.tools.drafts.insert(id, text);
        }
        self.share_drafts(cx);
        self.redraw_chrome(cx);
    }

    pub(super) fn put_draft(&mut self, id: &str, text: String, cx: &mut Context<Self>) {
        self.save_draft(id, &text, cx);
        if text.is_empty() {
            self.tools.drafts.remove(id);
        } else {
            self.tools.drafts.insert(id.to_owned(), text);
        }
        self.share_drafts(cx);
        if self.selected.as_deref() == Some(id) {
            self.show_draft(cx);
        }
    }

    fn share_drafts(&self, cx: &mut Context<Self>) {
        cx.set_global(SavedDrafts(self.tools.drafts.clone()));
    }

    fn save_draft(&self, id: &str, text: &str, _cx: &mut Context<Self>) {
        if let Some(dir) = self.drafts_dir() {
            let _ = crate::chat::drafts::put(&dir, id, text);
        }
    }

    fn drafts_dir(&self) -> Option<std::path::PathBuf> {
        self.root
            .fixture
            .is_none()
            .then(|| self.root.data_dir.clone())
    }

    pub(super) fn restore_drafts(&mut self, cx: &mut Context<Self>) {
        if cx.has_global::<SavedDrafts>() {
            return;
        }
        let Some(dir) = self.drafts_dir() else {
            return;
        };
        self.tools.drafts = crate::chat::drafts::load(&dir);
        self.share_drafts(cx);
    }

    pub(super) fn show_draft(&mut self, cx: &mut Context<Self>) {
        let text = self
            .selected
            .as_deref()
            .map(|id| self.tools.draft(id).to_owned())
            .unwrap_or_default();
        self.tools
            .editor
            .update(cx, |editor, cx| editor.set_text(&text, cx));
    }

    pub(super) fn composer_control(&self, cx: &mut Context<Self>) -> Control {
        let gate = self.acting(cx).control();
        let Some(session) = self.selected_session() else {
            return Control {
                enabled: false,
                tooltip: None,
            };
        };
        match super::tool::of(session.tool).refusal(self, session) {
            Some(AGY_BUSY) if !gate.enabled => gate,
            Some(why) => Control {
                enabled: false,
                tooltip: Some(why),
            },
            None => gate,
        }
    }

    pub(super) fn selected_working(&self) -> bool {
        self.selected_session().is_some_and(|s| {
            self.tools.live.is_working(&s.agent.id) || super::tool::of(s.tool).working(s)
        })
    }

    pub(super) fn selected_held_working(&self) -> bool {
        self.selected_session()
            .is_some_and(|s| s.tool == Tool::Antigravity)
            && self.tools.agy.as_ref().is_some_and(|a| a.held_working())
    }

    pub(super) fn stopping(&self) -> bool {
        let Some((row, at)) = &self.tools.stopping else {
            return false;
        };
        self.selected.as_deref() == Some(row.as_str())
            && crate::chat::activity::is_stopping(Some(*at), self.selected_working(), now_ms())
    }

    pub(super) fn spawn_send(
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

    pub(super) fn send_text(&mut self, cx: &mut Context<Self>) {
        if self.tools.sending || !self.composer_control(cx).enabled {
            return;
        }
        let Some(session) = self.selected_session().cloned() else {
            return;
        };
        if self.tools.starting.contains_key(&session.agent.id) {
            return;
        }
        let files = self.attached(&session.agent.id);
        let text = crate::chat::drops::with_attachments(self.tools.editor.read(cx).text(), &files);
        self.dispatch(session, text, true, cx);
    }

    pub(super) fn run_available(&self, cx: &App) -> bool {
        let Some(session) = self.selected_session() else {
            return false;
        };
        let takes_replies = self.acting_in(cx) == Acting::On
            && super::tool::of(session.tool)
                .refusal(self, session)
                .is_none();
        crate::chat::run_command::available(
            session.tool,
            takes_replies,
            self.open_subagent.is_some(),
        )
    }

    pub(super) fn run_command(&mut self, command: &str, how: RunHow, cx: &mut Context<Self>) {
        if !self.run_available(cx) {
            return;
        }
        if how == RunHow::Terminal {
            self.open_command_terminal(command, cx);
            return;
        }
        if self.tools.sending {
            return;
        }
        let Some(session) = self.selected_session().cloned() else {
            return;
        };
        if self.tools.starting.contains_key(&session.agent.id) {
            return;
        }
        self.dispatch(
            session,
            crate::chat::run_command::message(command),
            false,
            cx,
        );
    }

    fn dispatch(&mut self, session: Session, text: String, from_box: bool, cx: &mut Context<Self>) {
        let id = session.agent.id.clone();
        if text.is_empty() {
            return;
        }
        let tool = super::tool::of(session.tool);
        if tool.intercept(self, &session, &text, cx) {
            return;
        }
        let request = tool.send(self, &session, &text);
        let queues = tool.queues();
        if from_box {
            self.put_draft(&id, String::new(), cx);
            self.set_attached(&id, &[], cx);
        }
        self.tools
            .outgoing
            .entry(id.clone())
            .or_default()
            .push(Outgoing::new(&text, &self.conversation, now_ms()));
        self.follow_sent_now(cx);
        let sent_text = text;
        self.spawn_send(
            request,
            move |view, result, cx| match result {
                Ok(reply) => {
                    view.tools.delivered(&id, &sent_text, now_ms());
                    if queues {
                        match sent_state(&reply) {
                            Sent::Queued(_) => {
                                view.tools.take_outgoing(&id, &sent_text);
                                view.tools
                                    .queued
                                    .entry(id.clone())
                                    .or_default()
                                    .push(sent_text.clone());
                            }
                            Sent::Starting => {
                                view.tools.starting.insert(id.clone(), sent_text.clone());
                            }
                            Sent::Started => {}
                        }
                    }
                    if let Some(row) = sent_row(&reply).filter(|row| *row != id) {
                        if let Some(pick) = view.tools.picks.remove(&id) {
                            view.tools.picks.insert(row.clone(), pick);
                        }
                        if let Some(out) = view.tools.outgoing.remove(&id) {
                            view.tools.outgoing.insert(row.clone(), out);
                        }
                        view.mark_used(&row);
                        view.selected = Some(row);
                        view.loading = true;
                        view.show_draft(cx);
                    }
                    view.reload_now(cx);
                }
                Err(message) => {
                    view.tools.take_outgoing(&id, &sent_text);
                    if from_box {
                        let typed = view.tools.drafts.remove(&id).unwrap_or_default();
                        let back = if typed.is_empty() {
                            sent_text.clone()
                        } else {
                            format!("{sent_text}\n\n{typed}")
                        };
                        view.put_draft(&id, back, cx);
                    }
                    let asking = is_dialog(&message);
                    view.tools.error = Some(message);
                    if asking {
                        view.reload_now(cx);
                    }
                }
            },
            cx,
        );
    }

    pub(super) fn stop(&mut self, cx: &mut Context<Self>) {
        if self.tools.sending || !self.acting(cx).control().enabled {
            return;
        }
        let Some(session) = self.selected_session() else {
            return;
        };
        let id = session.agent.id.clone();
        if self.stopping() {
            return;
        }
        let request = super::tool::of(session.tool).stop(&id);
        self.tools.stopping = Some((id, now_ms()));
        self.spawn_send(
            request,
            |view, result, _| {
                if let Err(message) = result {
                    view.tools.stopping = None;
                    view.tools.error = Some(message);
                }
            },
            cx,
        );
    }

    pub(super) fn retry_stalled(&mut self, cx: &mut Context<Self>) {
        if self.tools.sending {
            return;
        }
        let Some(text) = self.stalled_prompt() else {
            return;
        };
        let Some(id) = self.selected.clone() else {
            return;
        };
        self.tools.retrying = Some((id.clone(), now_ms()));
        self.spawn_send(
            Request::interrupt(&id),
            move |view, result, cx| {
                if let Err(message) = result {
                    view.tools.retrying = None;
                    view.tools.error = Some(message);
                    return;
                }
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(RETRY_AFTER_STOP_MS))
                        .await;
                    this.update(cx, |view, cx| {
                        let (row, sent) = (id.clone(), text.clone());
                        view.spawn_send(
                            Request::reply(&row, &sent),
                            move |view, result, cx| {
                                view.tools.retrying = None;
                                match result {
                                    Ok(_) => {
                                        view.tools.delivered(&row, &sent, now_ms());
                                        view.conversation_state(cx, |s| s.follow_sent = true);
                                        view.reload_now(cx);
                                    }
                                    Err(message) => {
                                        if view.tools.drafts.get(&row).is_none_or(String::is_empty)
                                        {
                                            view.put_draft(&row, sent.clone(), cx);
                                        }
                                        view.tools.error = Some(message);
                                    }
                                }
                            },
                            cx,
                        );
                    })
                    .ok();
                })
                .detach();
            },
            cx,
        );
    }

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

    pub(super) fn answer_agy(&mut self, held: &HeldPrompt, option: String, cx: &mut Context<Self>) {
        let prompt_id = held.prompt_id.clone();
        self.spawn_send(
            agy_permission(&held.session_id, &held.permission.rpc_id, Some(&option)),
            move |view, result, _| match result {
                Ok(_) => {
                    view.tools.answered.insert(prompt_id);
                }
                Err(message) => view.tools.error = Some(message),
            },
            cx,
        );
    }

    pub(super) fn agy_prompt(&self) -> Option<HeldPrompt> {
        let id = self.selected.as_deref()?;
        let prompts = self.source.view().prompts?;
        held_prompt(&prompts, id, &self.tools.answered)
    }

    pub(super) fn menus(&self, cx: &App) -> Vec<crate::chat::pickers::Menu> {
        match self.selected_session() {
            Some(session) => super::tool::of(session.tool).menus(self, session, cx),
            None => Vec::new(),
        }
    }

    pub(super) fn codex_picker(&self) -> CodexPicker {
        let id = self.selected.clone().unwrap_or_default();
        CodexPicker::new(
            self.tools.codex_models.clone().unwrap_or_default(),
            self.tools.picks.get(&id).cloned().unwrap_or_default(),
            &self.conversation.facts,
        )
    }

    pub(super) fn toggle_menu(&mut self, menu: &str, cx: &mut Context<Self>) {
        let open = self.tools.picker.as_ref().is_some_and(|p| p.menu == menu);
        if let Some(tool) = self.selected_session().filter(|_| !open).map(|s| s.tool) {
            super::tool::of(tool).menu_opening(self);
        }
        self.tools.picker = (!open).then(|| Picker {
            menu: menu.to_owned(),
            more: false,
        });
        cx.notify();
    }

    pub(super) fn pick(&mut self, menu: &str, value: &str, cx: &mut Context<Self>) {
        if let Some(tool) = self.selected_session().map(|s| s.tool) {
            super::tool::of(tool).pick(self, menu, value, cx);
        }
    }

    pub(super) fn claude_catalog(&self) -> &[ModelInfo] {
        self.catalog
            .get(&CatalogTool::Claude)
            .map_or(&[], Vec::as_slice)
    }

    pub(super) fn claude_model(&self, cx: &App) -> Option<&str> {
        let hint = self
            .selected
            .as_deref()
            .and_then(|id| self.tools.model_hints.get(id));
        Hint::shown(
            hint,
            self.conversation.facts.model.as_deref(),
            self.parts.conversation.read(cx).s.drawn_ms,
        )
    }

    pub(super) fn claude_effort(&self, cx: &App) -> Option<&str> {
        let hint = self
            .selected
            .as_deref()
            .and_then(|id| self.tools.effort_hints.get(id));
        Hint::shown(
            hint,
            self.conversation.facts.effort.as_deref(),
            self.parts.conversation.read(cx).s.drawn_ms,
        )
    }

    pub(super) fn pick_claude(&mut self, kind: Kind, value: &str, cx: &mut Context<Self>) {
        self.tools.picker = None;
        if self.tools.sending || !self.composer_control(cx).enabled {
            cx.notify();
            return;
        }
        let Some(session) = self.selected_session().filter(|s| s.tool == Tool::Claude) else {
            return;
        };
        let id = session.agent.id.clone();
        let (text, hints, fact) = match kind {
            Kind::Model => (
                claude_picker::model_text(value),
                &mut self.tools.model_hints,
                self.conversation.facts.model.as_deref(),
            ),
            Kind::Effort => (
                claude_picker::effort_text(value),
                &mut self.tools.effort_hints,
                self.conversation.facts.effort.as_deref(),
            ),
        };
        hints.insert(id.clone(), Hint::new(value, fact, now_ms()));
        self.spawn_send(
            Request::reply(&id, &text),
            move |view, result, cx| match result {
                Ok(_) => view.reload_now(cx),
                Err(message) => {
                    match kind {
                        Kind::Model => view.tools.model_hints.remove(&id),
                        Kind::Effort => view.tools.effort_hints.remove(&id),
                    };
                    let asking = is_dialog(&message);
                    view.tools.error = Some(message);
                    if asking {
                        view.reload_now(cx);
                    }
                }
            },
            cx,
        );
    }

    pub(super) fn keep_pick(&mut self, picker: &CodexPicker) {
        if let Some(id) = &self.selected {
            if picker.is_default() {
                self.tools.picks.remove(id);
            } else {
                self.tools.picks.insert(id.clone(), picker.pick.clone());
            }
        }
    }

    pub(super) fn sync_tools(&mut self, cx: &mut Context<Self>) {
        let view = self.source.view();
        self.tools.live = view.tools;
        self.follow_starting_drafts(cx);
        let Some(session) = self.selected_session().cloned() else {
            return;
        };
        let id = session.agent.id;
        let now = now_ms();
        if let Some(list) = self.tools.outgoing.get_mut(&id) {
            list.retain(|o| o.keep(&self.conversation, now));
        }
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

    fn follow_starting_drafts(&mut self, cx: &mut Context<Self>) {
        if self.tools.starting.is_empty() {
            return;
        }
        let drafts: Vec<String> = self.tools.starting.keys().cloned().collect();
        for draft in drafts {
            match self.tools.live.follow(&draft) {
                Follow::Waiting => {}
                Follow::Named(row) => {
                    self.tools.starting.remove(&draft);
                    if let Some(pick) = self.tools.picks.remove(&draft) {
                        self.tools.picks.insert(row.clone(), pick);
                    }
                    if self.selected.as_deref() == Some(draft.as_str()) {
                        self.mark_used(&row);
                        self.selected = Some(row);
                        self.loading = true;
                        self.show_draft(cx);
                        self.reload_now(cx);
                    }
                }
                Follow::NotStarted(why) => {
                    let Some(sent) = self.tools.starting.remove(&draft) else {
                        continue;
                    };
                    let typed = self.tools.drafts.remove(&draft).unwrap_or_default();
                    let back = if typed.is_empty() {
                        sent
                    } else {
                        format!("{sent}\n\n{typed}")
                    };
                    self.put_draft(&draft, back, cx);
                    if self.selected.as_deref() == Some(draft.as_str()) {
                        self.tools.error = Some(why);
                    }
                }
            }
        }
        cx.notify();
    }

    fn fetch_agy(&mut self, id: String, cx: &mut Context<Self>) {
        if let Ask::Start(number) = self.tools.fetches.request(&id) {
            self.read_agy(id, number, cx);
        }
    }

    fn read_agy(&mut self, id: String, number: u64, cx: &mut Context<Self>) {
        let actions = self.acts(cx).actions;
        let asked = id.clone();
        let log = crate::tools::agy_context::log_path(&self.root.home, &id);
        cx.spawn(async move |this, cx| {
            let (conversation, state, context) = cx
                .background_executor()
                .spawn(async move {
                    let conversation = actions.read(&agy_conversation(&id, false)).ok();
                    let state = actions.read(&agy_state(&id)).ok();
                    let context = log
                        .as_deref()
                        .and_then(crate::tools::agy_context::context_tokens);
                    (conversation, state, context)
                })
                .await;
            this.update(cx, |view, cx| {
                let done = view.tools.fetches.finished(&asked, number);
                if done.apply && view.selected.as_deref() == Some(asked.as_str()) {
                    if let Some(c) = conversation.as_ref().and_then(parse_conversation) {
                        view.follow_content(false, cx);
                        let mut shown = c.conversation.clone();
                        if shown.facts.context_tokens == 0 {
                            shown.facts.context_tokens = context.unwrap_or(0);
                        }
                        view.conversation = Arc::new(shown);
                        view.refresh_shells(0);
                        view.loading = false;
                        view.tools.agy = Some(c);
                    }
                    if let Some(state) = state {
                        view.tools.agy_config = parse_config(&state);
                    }
                }
                if let Some(next) = done.next {
                    view.read_agy(asked.clone(), next, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
