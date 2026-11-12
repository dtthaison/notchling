//! Loading the open session and the sessions list: picking, the background read and taking its
//! result, and the half-second tick that decides when to read again.

use super::*;

impl ChatView {
    /// Opens the job New Session started: now when the sidebar lists it, else at the load that
    /// does (`chat::sidebar::row_for_job`).
    pub(super) fn show_started_job(&mut self, job: &str, cx: &mut Context<Self>) {
        match crate::chat::sidebar::row_for_job(&self.sessions, job) {
            Some(id) => {
                self.pending_job = None;
                self.select(&id, cx);
            }
            None => {
                self.pending_job = Some(PendingJob::new(job, now_ms()));
                self.gate.changed();
                cx.notify();
            }
        }
    }

    /// Shows a conversation the search panel found: selects its row when it is listed, else
    /// selects it as soon as a load lists it (`chat::sidebar::row_for`).
    pub fn show_session(&mut self, target: SessionRef, cx: &mut Context<Self>) {
        match row_for(&self.sessions, &target) {
            Some(id) => {
                self.pending_session = None;
                self.select(&id, cx);
            }
            None => {
                self.pending_session = Some(PendingSession::new(target, now_ms()));
                self.gate.changed();
                cx.notify();
            }
        }
    }

    /// Reads everything again on the background executor, for the session picked now. A load
    /// already running is not doubled: another follows it.
    pub(super) fn start_load(&mut self, cx: &mut Context<Self>) {
        self.gate.reloaded(now_ms());
        let Some(mut loader) = self.loader.take() else {
            self.load_again = true;
            return;
        };
        let asked = self.selected.clone();
        let wanted = asked.clone();
        let view = self.source.view();
        let (claude, live) = (view.agents, view.tools);
        // The Swift hook's files are read only while the daemon's held prompts aren't followed.
        loader.files_prompts = view.prompts.is_none();
        loader.subagent_path = self.open_subagent.as_ref().map(|o| o.agent.path.clone());
        loader.subagent_origin = self
            .open_subagent
            .as_ref()
            .map_or_else(Default::default, |o| o.agent.origin);
        cx.spawn(async move |this, cx| {
            let (loader, loaded) = cx
                .background_executor()
                .spawn(async move {
                    let loaded = loader.load_live(wanted.as_deref(), claude, &live, now_ms());
                    (loader, loaded)
                })
                .await;
            this.update(cx, |view, cx| view.apply(asked, loader, loaded, cx))
                .ok();
        })
        .detach();
    }

    /// Takes a finished load. `asked` is the session it was started for: if another was picked
    /// meanwhile, its sessions list still counts but its conversation does not.
    pub(super) fn apply(
        &mut self,
        asked: Option<String>,
        loader: Loader,
        loaded: Loaded,
        cx: &mut Context<Self>,
    ) {
        self.loader = Some(loader);
        self.now_ms = loaded.now_ms;
        let mut all = loaded.sessions;
        self.delegated = loaded.delegated;
        let (selected, starting) = (self.selected.clone(), &self.tools.starting);
        self.new_drafts.retain(|d| {
            selected.as_deref() == Some(d.agent.id.as_str()) || starting.contains_key(&d.agent.id)
        });
        all.extend(self.new_drafts.iter().cloned());
        let draft_asked = asked
            .as_deref()
            .is_some_and(|id| self.new_drafts.iter().any(|d| d.agent.id == id));
        self.prompt.files = loaded.requests;
        // The search panel's session, now that the list has it (closed or not), or never.
        if let Some(pending) = self.pending_session.as_mut() {
            match pending.after_load(&all, now_ms()) {
                Pending::Found(id) => {
                    self.pending_session = None;
                    self.reopen_row(&id);
                    self.sessions = self.open_rows(all);
                    self.select(&id, cx);
                    return;
                }
                Pending::Waiting => {}
                Pending::GaveUp => self.pending_session = None,
            }
        }
        if let Some(pending) = self.pending_job.clone() {
            match pending.after_load(&all, now_ms()) {
                Pending::Found(id) => {
                    self.pending_job = None;
                    self.sessions = self.open_rows(all);
                    self.select(&id, cx);
                    return;
                }
                Pending::Waiting => {}
                Pending::GaveUp => self.pending_job = None,
            }
        }
        // What the disk says about the paths in messages is asked again once per load.
        self.conversation_state(cx, |s| s.markdown.get_mut().reloaded());
        // Whether the conversation shows anything new: a load of the files another working session
        // changed leaves this one as it was, and its long layout needn't be done again.
        let selected_before = self.selected_session().cloned();
        let mut conversation_changed = false;
        // What changes the column's size or what it follows to the end, which a draw of it
        // applies (a scroll to the end waits for one, and a question card shrinks its room): the
        // conversation is drawn again for these even when its input is equal. The row's own
        // fields changing (its age, its status) aren't among them.
        let mut layout_changed = false;
        self.sessions = self.open_rows(all);
        conversation_changed |= self.selected_session() != selected_before.as_ref();
        if self.selected == asked {
            let was = self.selected.take();
            // The loader doesn't know a draft New Session opened; it stays picked.
            self.selected = if draft_asked {
                asked.clone()
            } else {
                loaded.selected
            };
            // The loader picks from every row; a closed one gives way to the first one shown.
            if let Some(sel) = self.selected.clone() {
                if !self.sessions.iter().any(|s| s.agent.id == sel) {
                    self.selected = crate::chat::sidebar::sections(&self.sessions, self.now_ms)
                        .into_iter()
                        .flat_map(|s| s.rows)
                        .map(|r| r.id)
                        .next();
                    if self.selected.is_some() {
                        self.open_at_end(cx);
                        self.reload_now(cx);
                    }
                }
            }
            if self.selected != was {
                self.open_subagent = None;
                self.show_draft(cx);
            }
            // An Antigravity row's conversation is the daemon's, fetched by `sync_tools`; the
            // files have none to put in its place.
            let agy = self.selected_session().map(|s| s.tool) == Some(Tool::Antigravity);
            let session_changed = self.take_open_at_end(cx);
            conversation_changed |= self.selected != was || session_changed;
            layout_changed |= self.selected != was || session_changed;
            if !agy {
                let content_changed = match &loaded.change {
                    // The loader compared against the conversation on show: trust it.
                    Some((from, change)) if Arc::ptr_eq(from, &self.conversation) => {
                        *change != Change::Same
                    }
                    // Anything else (first load, a subagent in between): pointer, then content.
                    _ => {
                        !Arc::ptr_eq(&self.conversation, &loaded.conversation)
                            && *self.conversation != *loaded.conversation
                    }
                };
                conversation_changed |= self.loading || content_changed;
                layout_changed |= self.loading || content_changed;
                if conversation_changed {
                    self.follow_content(session_changed, cx);
                }
                self.conversation = loaded.conversation;
                let now = self.conversation.clone();
                self.parts
                    .conversation
                    .update(cx, |v, _| v.take_change(loaded.change, &now));
                self.loading = false;
            } else if session_changed {
                self.scroll_to_end_now(cx);
            }
            // The conversation shows which subagent an Agent step opens, not how it's doing.
            let links = crate::chat::subagent_view::links;
            let others = links(&self.subagents) != links(&loaded.subagents)
                || self.open_subagent.is_some()
                || self.prompt.transcript != loaded.question;
            conversation_changed |= others;
            layout_changed |= others;
            self.subagents = loaded.subagents;
            if let Some(open) = self.open_subagent.as_mut() {
                open.apply_shared(&self.subagents, loaded.subagent);
            }
            self.prompt.transcript = loaded.question;
        } else {
            self.load_again = true;
        }
        if std::mem::take(&mut self.load_again) {
            self.start_load(cx);
        }
        self.sync_tools(cx);
        if layout_changed {
            self.redraw_conversation(cx);
        }
        if conversation_changed {
            cx.notify();
        } else {
            self.redraw_all_but_conversation(cx);
        }
    }

    /// Reads again now, past the gate (something this window did changed the files).
    pub(super) fn reload_now(&mut self, cx: &mut Context<Self>) {
        self.gate.changed();
        self.start_load(cx);
    }

    pub(super) fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.selected.as_deref() == Some(id) {
            return;
        }
        self.selected = Some(id.to_owned());
        // Both selections let go: the conversation's, and the file panel's own.
        select::clear();
        self.conversation_state(cx, |s| {
            s.sel.borrow_mut().selection.clear();
            s.markdown.get_mut().switched();
        });
        self.mark_used(id);
        self.conversation = Arc::default();
        self.subagents.clear();
        self.open_subagent = None;
        self.prompt.transcript = None;
        self.loading = true;
        self.open_at_end(cx);
        self.tools.error = None;
        self.tools.picker = None;
        self.tools.agy = None;
        self.tools.agy_fetched = None;
        self.tools.agy_config.clear();
        self.show_draft(cx);
        self.start_load(cx);
        cx.notify();
    }

    pub(super) fn tick(&mut self, cx: &mut Context<Self>) {
        // New rows or a lost daemon: look again (the gate still spaces the reads).
        let generation = self.source.generation();
        if generation != self.seen_generation {
            self.seen_generation = generation;
            self.gate.changed();
            let note = self.source.view().note();
            if note != self.daemon_note {
                self.daemon_note = note;
            }
            self.sync_tools(cx);
            cx.notify();
        }
        // A new model list only redraws (the pickers, the meter); it never reloads the conversation.
        let catalog = self.source.catalog_generation();
        if catalog != self.seen_catalog {
            self.seen_catalog = catalog;
            self.catalog.insert(
                CatalogTool::Claude,
                self.source.catalog(CatalogTool::Claude),
            );
            cx.notify();
        }
        // The live line's clock: its spinner is Core Animation's and draws no frames, so the
        // window's outer parts are drawn once a second while it shows, which reads the clock again.
        if crate::chat::activity::clock_due(self.activity_drawn.get(), now_ms()) {
            self.redraw_chrome(cx);
        }
        let dot = self.bridge_on();
        if dot != self.phone_dot {
            self.phone_dot = dot;
            self.redraw_chrome(cx);
        }
        if let Some((_, batches)) = &self.watch {
            while batches.try_recv().is_ok() {
                self.gate.changed();
            }
        }
        // The check on Copy goes after a moment; a hovered row's time is kept fresh.
        let now = now_ms();
        let redraw = self.conversation_state(cx, |s| {
            let copy_over = s
                .copied
                .as_ref()
                .is_some_and(|(_, at)| !message_meta::copied_shows(*at, now));
            if copy_over {
                s.copied = None;
            }
            copy_over || (s.hovered_message.is_some() && message_meta::stale(s.drawn_ms, now))
        });
        if redraw {
            self.redraw_conversation(cx);
        }
        // A second opinion's card changes when its command ends, which nothing else signals; the
        // tick that sees it end draws it once more.
        shell_probe::tick(self, cx);
        let running = opinions::any_running(self, cx);
        let was_running =
            self.conversation_state(cx, |s| std::mem::replace(&mut s.opinions_running, running));
        if running || was_running {
            self.redraw_conversation(cx);
        }
        self.refresh_file(cx);
        if self.gate.due(now_ms()) {
            self.start_load(cx);
        }
        // The header's meters and account menu: the accounts store, every few seconds.
        let now = now_ms();
        if now - self.accounts_polled_ms >= accounts::POLL.as_millis() as i64 {
            self.accounts_polled_ms = now;
            self.codex_installed = accounts::codex_installed(&self.root.home);
            self.poll_accounts(cx);
        }
    }
}
