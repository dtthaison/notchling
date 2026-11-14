use super::*;

impl ChatView {
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

    pub fn show_session(
        &mut self,
        target: SessionRef,
        child: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.pending_child = child.as_deref().map(PendingChild::new);
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
        loader.files_prompts = view.prompts.is_none();
        loader.more_subagents = std::mem::take(&mut self.more_subagents);
        loader.open_groups = self.open_groups.clone();
        loader.more_under = std::mem::take(&mut self.more_under);
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
        self.conversation_state(cx, |s| s.markdown.get_mut().reloaded());
        let selected_before = self.selected_session().cloned();
        let mut conversation_changed = false;
        let mut layout_changed = false;
        self.sessions = self.open_rows(all);
        conversation_changed |= self.selected_session() != selected_before.as_ref();
        if self.selected == asked {
            let was = self.selected.take();
            self.selected = if draft_asked {
                asked.clone()
            } else {
                loaded.selected
            };
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
            let agy = self.selected_session().map(|s| s.tool) == Some(Tool::Antigravity);
            let session_changed = self.take_open_at_end(cx);
            conversation_changed |= self.selected != was || session_changed;
            layout_changed |= self.selected != was || session_changed;
            if !agy {
                let content_changed = match &loaded.change {
                    Some((from, change)) if Arc::ptr_eq(from, &self.conversation) => {
                        *change != Change::Same
                    }
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
            let links = crate::chat::subagent_view::links;
            let others = links(&self.subagents) != links(&loaded.subagents)
                || self.open_subagent.is_some()
                || self.prompt.transcript != loaded.question;
            conversation_changed |= others;
            layout_changed |= others;
            self.subagents = loaded.subagents;
            self.subagent_rows = loaded.subagent_rows;
            self.subagent_counts = loaded.subagent_counts;
            if loaded.partial {
                self.load_again = true;
            } else {
                self.open_pending_child(cx);
            }
            self.refresh_shells(0);
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
        if conversation_changed && !self.selected_working() {
            self.git_ask(crate::chat::git::Why::TurnEnded, cx);
        }
        if layout_changed {
            self.redraw_conversation(cx);
        }
        if conversation_changed {
            cx.notify();
        } else {
            self.redraw_all_but_conversation(cx);
        }
    }

    pub(super) fn reload_now(&mut self, cx: &mut Context<Self>) {
        self.gate.changed();
        self.start_load(cx);
    }

    pub(super) fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.selected.as_deref() == Some(id) {
            return;
        }
        self.selected = Some(id.to_owned());
        self.close_git_diff(cx);
        select::clear();
        self.conversation_state(cx, |s| {
            s.sel.borrow_mut().selection.clear();
            s.markdown.get_mut().switched();
        });
        self.mark_used(id);
        self.conversation = Arc::default();
        self.subagents.clear();
        self.subagent_rows.clear();
        self.subagent_counts = Default::default();
        self.more_subagents = 0;
        self.open_groups.clear();
        self.more_under.clear();
        self.shells = Default::default();
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
        self.git_ask(crate::chat::git::Why::Folder, cx);
        cx.notify();
    }

    pub(super) fn tick(&mut self, cx: &mut Context<Self>) {
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
        let catalog = self.source.catalog_generation();
        if catalog != self.seen_catalog {
            self.seen_catalog = catalog;
            self.catalog.insert(
                CatalogTool::Claude,
                self.source.catalog(CatalogTool::Claude),
            );
            cx.notify();
        }
        if crate::chat::activity::clock_due(self.activity_drawn.get(), now_ms()) {
            self.redraw_chrome(cx);
        }
        self.inbox_tick(cx);
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
        shell_probe::tick(self, cx);
        let running = opinions::any_running(self, cx);
        let was_running =
            self.conversation_state(cx, |s| std::mem::replace(&mut s.opinions_running, running));
        if running || was_running {
            self.redraw_conversation(cx);
        }
        self.refresh_file(cx);
        self.git_ask(crate::chat::git::Why::Tick, cx);
        self.git_diff_tick(cx);
        if self.gate.due(now_ms()) {
            self.start_load(cx);
        }
        let now = now_ms();
        if now - self.accounts_polled_ms >= accounts::POLL.as_millis() as i64 {
            self.accounts_polled_ms = now;
            self.codex_installed = accounts::codex_installed(&self.root.home);
            self.poll_accounts(cx);
        }
    }
}
