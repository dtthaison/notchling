//! Chat acting on sessions (Milestone 5's app side): the reply box sends `session.reply`,
//! `codex.send` or `agy.send` by the row's tool, Stop sends `session.interrupt`, `codex.stop` or
//! `agy.cancel`, the pickers send nothing (Codex) or `agy.set_option`, and an Antigravity
//! permission is answered with `agy.permission`. An Antigravity row's conversation comes from the
//! daemon (`agy.conversation`), fetched again whenever the `tools` topic says it changed. Every
//! control follows `Acting::control`; nothing is sent while acting isn't on.

use std::collections::{HashMap, HashSet};

use gpui::{AppContext, Context, Entity, FocusHandle, Focusable, Global, Subscription};

/// Every row's unsent text, kept by the app rather than the Chat window, so closing Chat and
/// opening it again finds what you were typing.
#[derive(Clone, Default)]
struct SavedDrafts(HashMap<String, String>);

impl Global for SavedDrafts {}
use notchling_engine::tools::agy::ConfigOption;
use notchling_engine::tools::codex::models;
use serde_json::Value;

use super::ChatView;
use crate::acting::{Acting, Control, Request};
use crate::chat::claude_picker::{self, Hint, Kind};
use crate::chat::data::Tool;
use crate::chat::outgoing::Outgoing;
use crate::chat::prompts::is_dialog;
use crate::phone::bridge::now_ms;
use crate::tools::agy_options::parse_config;
use crate::tools::agy_permission::{held_prompt, HeldPrompt};
use crate::tools::codex_picker::CodexPicker;
use crate::tools::fetch_gate::{Ask, FetchGate};
use crate::tools::live::{Follow, ToolLive};
use crate::tools::open_gate::OpenGate;
use crate::tools::requests::{
    agy_cancel, agy_conversation, agy_open, agy_permission, agy_send, agy_send_draft,
    agy_set_option, agy_state, codex_send, codex_send_draft, codex_stop, is_draft,
    parse_conversation, sent_row, sent_state, AgyConversation, Pick, Sent,
};
use notchling_kit::ui::{TextEditor, TextEditorEvent};

/// How long after Stop a stalled prompt is sent again.
const RETRY_AFTER_STOP_MS: u64 = 1_500;

/// Why a Claude Code row that isn't a background job takes no reply.
pub const READ_ONLY: &str = "Read only here. Reply where the session runs.";
/// Antigravity takes one turn at a time (Codex queues; the daemon says so).
pub const AGY_BUSY: &str = "Antigravity is still on the last turn. Send again when it ends.";
pub const EXITED: &str = "This session has exited.";

/// A pop-up over the composer.
pub enum Picker {
    Codex(CodexPicker),
    Agy,
    /// A Claude Code session's Model menu; `more` is its More Models list open.
    ClaudeModel {
        more: bool,
    },
    /// A Claude Code session's Effort menu.
    ClaudeEffort,
}

/// What Chat keeps about acting on the other tools.
pub struct ChatTools {
    /// What the `tools` topic last said, from the session source.
    pub live: ToolLive,
    /// The reply box's text per row, so switching sessions keeps what was typed. The selected
    /// row's is what [`ChatTools::editor`] shows, kept the same on every change.
    pub drafts: HashMap<String, String>,
    pub sending: bool,
    pub error: Option<String>,
    /// Each Codex row's model and effort for its next turns (kept for this run).
    pub picks: HashMap<String, Pick>,
    /// The selected Antigravity row's conversation and state from the daemon.
    pub agy: Option<AgyConversation>,
    /// Which row and change count the last fetch was for.
    pub agy_fetched: Option<(String, u64)>,
    /// The reads of Antigravity conversations: one in flight per row, none out of order.
    pub fetches: FetchGate,
    pub agy_config: Vec<ConfigOption>,
    /// Which Antigravity rows this Chat asked the daemon to open, and when to ask again (the
    /// daemon closes a session after ten idle minutes).
    pub opens: OpenGate,
    /// Messages the daemon queued behind a Codex turn, shown until that turn ends.
    pub queued: HashMap<String, Vec<String>>,
    /// Each row's messages sent from here and not in its transcript yet, drawn as "Sending…".
    pub outgoing: HashMap<String, Vec<Outgoing>>,
    /// Draft rows whose first message the daemon is still starting a Codex session for, with the
    /// text sent: the row follows the session when it is named, or gets the text back.
    pub starting: HashMap<String, String>,
    /// Antigravity prompts answered here, hidden until the daemon resolves them.
    pub answered: HashSet<String>,
    pub picker: Option<Picker>,
    /// A model or effort picked for a Claude Code row, shown until its transcript says so.
    pub model_hints: HashMap<String, Hint>,
    pub effort_hints: HashMap<String, Hint>,
    /// The row a Stop was sent to and when (ms), until it stops working or 20 s pass.
    pub stopping: Option<(String, i64)>,
    /// A stalled prompt being sent again (row, when): Retry hides meanwhile.
    pub retrying: Option<(String, i64)>,
    /// The reply box: the kit's editor (caret, selection, input method), one for every row.
    pub editor: Entity<TextEditor>,
    /// The editor's focus handle.
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
            model_hints: HashMap::new(),
            effort_hints: HashMap::new(),
            stopping: None,
            retrying: None,
            editor,
            focus,
            _editor_events: events,
        }
    }

    /// The box's text for a row.
    pub fn draft(&self, id: &str) -> &str {
        self.drafts.get(id).map(String::as_str).unwrap_or("")
    }

    /// The daemon typed `text` into `row`'s session: its bubble now waits only so long.
    fn delivered(&mut self, row: &str, text: &str, now_ms: i64) {
        if let Some(out) = self.outgoing.get_mut(row).and_then(|list| {
            list.iter_mut()
                .find(|o| o.text == text && o.delivered_at.is_none())
        }) {
            out.delivered(now_ms);
        }
    }

    /// Drops `text`'s bubble from `row` (it failed, or another bubble shows it now).
    fn take_outgoing(&mut self, row: &str, text: &str) {
        if let Some(list) = self.outgoing.get_mut(row) {
            if let Some(i) = list.iter().position(|o| o.text == text) {
                list.remove(i);
            }
        }
    }
}

impl ChatView {
    /// The selected row's messages on their way, for the conversation's "Sending…" bubbles.
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

    /// The editor's text changed: it is the selected row's draft.
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
        cx.notify();
    }

    /// Sets a row's draft; the editor follows when that row is the one on show.
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

    /// Hands the drafts to the app, which outlives this window.
    fn share_drafts(&self, cx: &mut Context<Self>) {
        cx.set_global(SavedDrafts(self.tools.drafts.clone()));
    }

    /// Writes one row's draft to the app's database, which outlives the app (a quit, a crash, an
    /// install). Nothing is written over a fixture or a render.
    fn save_draft(&self, id: &str, text: &str, _cx: &mut Context<Self>) {
        if let Some(dir) = self.drafts_dir() {
            // A draft that can't be written still lives in memory; the next keystroke tries again.
            let _ = crate::chat::drafts::put(&dir, id, text);
        }
    }

    fn drafts_dir(&self) -> Option<std::path::PathBuf> {
        self.root
            .fixture
            .is_none()
            .then(|| self.root.data_dir.clone())
    }

    /// The first Chat window since the app started: the drafts saved on disk come back.
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

    /// Puts the selected row's draft in the editor (after a switch of rows).
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

    /// Whether a Stop sent to the selected row is still on its way (a second Esc to a session
    /// that has just stopped would open the terminal's rewind menu).
    pub(super) fn stopping(&self) -> bool {
        let Some((row, at)) = &self.tools.stopping else {
            return false;
        };
        self.selected.as_deref() == Some(row.as_str())
            && crate::chat::activity::is_stopping(Some(*at), self.selected_working(), now_ms())
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
        // The first message is still starting this session: another would start a second one.
        if self.tools.starting.contains_key(&id) {
            return;
        }
        // The editor's text, which is this row's draft, read live rather than from the copy.
        let text = self.tools.editor.read(cx).text().trim().to_owned();
        if text.is_empty() {
            return;
        }
        // A command that opens a menu runs in a terminal tab attached to the session, where the
        // menu shows; typed through the hidden attach it would hold the session on a menu unseen.
        if session.tool == Tool::Claude {
            use crate::chat::terminal::{menu_route, MenuRoute};
            match menu_route(&text, &session.agent.target) {
                MenuRoute::Reply => {}
                MenuRoute::Attach { job } => {
                    let opened = self.open_menu_command(
                        &job,
                        session.agent.config_dir.as_deref(),
                        &session.agent.cwd,
                        &text,
                        cx,
                    );
                    match opened {
                        Ok(()) => self.put_draft(&id, String::new(), cx),
                        Err(message) => self.tools.error = Some(message),
                    }
                    cx.notify();
                    return;
                }
                MenuRoute::OwnTerminal => {
                    self.tools.error = Some(format!(
                        "{} opens a menu: run it in this session's own terminal",
                        text.split_whitespace().next().unwrap_or("This command")
                    ));
                    cx.notify();
                    return;
                }
            }
        }
        let draft = is_draft(&id);
        let request = match session.tool {
            Tool::Claude => Request::reply(&id, &text),
            Tool::Codex => {
                let pick = self.tools.picks.get(&id).cloned().unwrap_or_default();
                if draft {
                    codex_send_draft(&id, &session.agent.cwd, &text, &pick)
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
        self.put_draft(&id, String::new(), cx);
        // Shown at once as your bubble: typing it into the session takes seconds.
        self.tools
            .outgoing
            .entry(id.clone())
            .or_default()
            .push(Outgoing::new(&text, &self.conversation));
        // The next reload goes to the end, wherever you were reading.
        self.follow_sent = true;
        self.conversation_scroll.scroll_to_bottom();
        let sent_text = text;
        self.spawn_send(
            request,
            move |view, result, cx| match result {
                Ok(reply) => {
                    view.tools.delivered(&id, &sent_text, now_ms());
                    if session.tool == Tool::Codex {
                        match sent_state(&reply) {
                            Sent::Queued(_) => {
                                // Its Queued bubble takes over from the Sending one.
                                view.tools.take_outgoing(&id, &sent_text);
                                view.tools
                                    .queued
                                    .entry(id.clone())
                                    .or_default()
                                    .push(sent_text.clone());
                            }
                            // Not named yet: the draft follows it when the daemon says so.
                            Sent::Starting => {
                                view.tools.starting.insert(id.clone(), sent_text.clone());
                            }
                            Sent::Started => {}
                        }
                    }
                    // A draft's first message names the real row: follow it.
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
                    // Back into the box, in front of anything typed since.
                    let typed = view.tools.drafts.remove(&id).unwrap_or_default();
                    let back = if typed.is_empty() {
                        sent_text.clone()
                    } else {
                        format!("{sent_text}\n\n{typed}")
                    };
                    view.put_draft(&id, back, cx);
                    // A question or permission is open: read it again, for the card.
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

    /// Stop: the row's tool is asked to end its turn.
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
        let request = match session.tool {
            Tool::Claude => Request::interrupt(&id),
            Tool::Codex => codex_stop(&id),
            Tool::Antigravity => agy_cancel(&id),
        };
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

    /// Retry, on a prompt that got nothing back (`activity::stalled`): the stuck turn is stopped,
    /// so a late answer can't come with a second copy queued behind it, then the same text goes
    /// again on a fresh request.
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
                // Claude Code takes a moment to drop the turn before the box takes keys again.
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
                                        view.follow_sent = true;
                                        view.reload_now(cx);
                                    }
                                    Err(message) => {
                                        // Into an empty box, so the words aren't lost.
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

    /// An option of the Antigravity permission card: the answer goes for the session the prompt
    /// belongs to, which is what the daemon named, not whichever row is selected now.
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

    /// The Antigravity permission waiting on the selected row, from the daemon's held prompts.
    pub(super) fn agy_prompt(&self) -> Option<HeldPrompt> {
        let id = self.selected.as_deref()?;
        let prompts = self.source.view().prompts?;
        held_prompt(&prompts, id, &self.tools.answered)
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

    /// Opens or closes a Claude Code session's Model or Effort menu (the other closes).
    pub(super) fn toggle_claude_picker(&mut self, kind: Kind, cx: &mut Context<Self>) {
        let open = matches!(
            (&self.tools.picker, kind),
            (Some(Picker::ClaudeModel { .. }), Kind::Model)
                | (Some(Picker::ClaudeEffort), Kind::Effort)
        );
        self.tools.picker = (!open).then_some(match kind {
            Kind::Model => Picker::ClaudeModel { more: false },
            Kind::Effort => Picker::ClaudeEffort,
        });
        cx.notify();
    }

    /// The model the selected row shows: the pick not yet in the transcript, else the transcript's.
    pub(super) fn claude_model(&self) -> Option<&str> {
        let hint = self
            .selected
            .as_deref()
            .and_then(|id| self.tools.model_hints.get(id));
        Hint::shown(
            hint,
            self.conversation.facts.model.as_deref(),
            self.drawn_ms,
        )
    }

    /// The effort the selected row shows, the same way.
    pub(super) fn claude_effort(&self) -> Option<&str> {
        let hint = self
            .selected
            .as_deref()
            .and_then(|id| self.tools.effort_hints.get(id));
        Hint::shown(
            hint,
            self.conversation.facts.effort.as_deref(),
            self.drawn_ms,
        )
    }

    /// A pick in the Model or Effort menu: `/model <id>` or `/effort <level>` goes to the session
    /// as a reply (`session.reply`), as the reply box would send it. The box's own text stays.
    pub(super) fn pick_claude(&mut self, kind: Kind, value: &'static str, cx: &mut Context<Self>) {
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
                    // Not sent: what the label showed goes back to the transcript's.
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
        self.follow_starting_drafts(cx);
        let Some(session) = self.selected_session().cloned() else {
            return;
        };
        let id = session.agent.id;
        // Messages the transcript now holds give way to the real bubbles.
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

    /// A draft whose session was still starting when the daemon replied: once Codex names it the
    /// row becomes the session's (if it is the one on show, it is shown), and if it never starts
    /// the text goes back into the box with the reason.
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

    /// Reads the row's conversation and options from the daemon, off the UI thread: one read at a
    /// time per row, and a change that comes meanwhile is covered by the read that follows
    /// (`FetchGate`).
    fn fetch_agy(&mut self, id: String, cx: &mut Context<Self>) {
        if let Ask::Start(number) = self.tools.fetches.request(&id) {
            self.read_agy(id, number, cx);
        }
    }

    fn read_agy(&mut self, id: String, number: u64, cx: &mut Context<Self>) {
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
                let done = view.tools.fetches.finished(&asked, number);
                if done.apply && view.selected.as_deref() == Some(asked.as_str()) {
                    if let Some(c) = conversation.as_ref().and_then(parse_conversation) {
                        view.follow_content(false);
                        view.conversation = c.conversation.clone();
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
