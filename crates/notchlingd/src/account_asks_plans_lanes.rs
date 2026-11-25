use super::*;

impl AccountAsks {
    pub(super) fn plan_move_all(
        &self,
        seen: &Seen,
        to: &str,
        base: impl Fn(&str) -> Value,
        mine: &str,
    ) -> Result<Plan, ApiError> {
        let target = if to == "main" {
            None
        } else {
            Some(self.found(seen, to)?)
        };
        let active = self.main_account(seen);
        let made = MoveAllPlan::make(
            &seen.agents,
            &seen.lanes,
            seen.snap.active.as_deref(),
            target.as_ref().map(|t| t.raw.as_str()),
        );
        let closing = made
            .close_lane
            .as_deref()
            .and_then(|name| seen.lanes.iter().find(|l| l.name == name));
        let switching = made.switch_main.is_some();
        let main_after = if switching {
            target.clone()
        } else {
            active.clone()
        };
        let mut owned = Vec::new();
        if let Some(t) = &target {
            owned.push(Owned::Account(t.kind, t.raw.clone()));
        }
        if let Some(a) = &active {
            owned.push(Owned::Account(Kind::Claude, a.raw.clone()));
        }
        owned.extend(self.main_folders(seen));
        if let Some(lane) = closing {
            owned.push(Owned::Account(Kind::Claude, lane.account_id.clone()));
            owned.extend(self.lane_folders(seen, lane));
        }
        let mut jobs = Vec::new();
        for job in &made.jobs {
            if let Some(agent) = seen.agents.iter().find(|a| a.id == job.agent_id) {
                if let Some(lane) = seen.lane_of(agent) {
                    owned.push(Owned::Account(Kind::Claude, lane.account_id.clone()));
                }
            }
            owned.push(Owned::Folder(job.cwd.clone()));
            jobs.push(job.agent_id.clone());
        }
        self.owners(&owned, mine)?;
        self.gate(!made.jobs.is_empty())?;
        if let Some(t) = &target {
            self.claude_only(t)?;
        }
        if made.is_empty() {
            return Err(bad_why("Nothing would move.", "already_set"));
        }
        if !main_after.as_ref().is_some_and(|a| !a.needs_login) {
            return Err(forbidden(SIGNED_OUT, "signed_out"));
        }
        jobs.sort();
        let close_lane = closing.and_then(|l| self.book.lane_id(&l.name));
        let mut change = base("claude");
        change["to"] = json!(target.as_ref().map(|t| t.id.clone()));
        change["before"] = json!(if switching {
            active.as_ref().map(|a| a.id.clone())
        } else {
            None
        });
        change["closeLane"] = json!(close_lane);
        change["jobs"] = json!(jobs);
        change["signedOut"] = json!(made.signed_out.len());
        change["apply"] = json!({"op": ops::ACCOUNTS_MOVE_ALL,
            "args": {"target": target.as_ref().map(|t| t.id.clone())}});
        let mut facts = Vec::new();
        if let (true, Some(t)) = (switching, &target) {
            let shown = masked(&t.email, &t.id);
            facts.push(fact(
                "consent-account",
                json!({"tool": "claude", "account": shown, "organization": mine}),
            ));
            facts.push(match &active {
                Some(a) => fact(
                    "consent-account-switch",
                    json!({"tool": "claude", "account": shown, "before": masked(&a.email, &a.id)}),
                ),
                None => fact(
                    "consent-account-switch-first",
                    json!({"tool": "claude", "account": shown}),
                ),
            });
        }
        facts.push(fact(
            "consent-account-move-all",
            json!({"count": jobs.len(),
                "account": main_after.as_ref().map(|a| masked(&a.email, &a.id))}),
        ));
        if let Some(lane) = closing {
            let acct = self.lane_acct(seen, lane);
            facts.push(fact(
                "consent-lane-close",
                json!({"account": acct.map(|a| masked(&a.email, &a.id))}),
            ));
        }
        if !made.signed_out.is_empty() {
            facts.push(fact(
                "consent-account-signed-out",
                json!({"count": made.signed_out.len()}),
            ));
        }
        Ok(Plan {
            target: format!(
                "move_all:{}",
                target.as_ref().map_or("main", |t| t.id.as_str())
            ),
            change,
            facts: json!(facts),
        })
    }

    pub(super) fn plan_move(
        &self,
        seen: &Seen,
        session: &str,
        to: &str,
        base: impl Fn(&str) -> Value,
        mine: &str,
    ) -> Result<Plan, ApiError> {
        let agent = seen
            .agents
            .iter()
            .find(|a| a.id == session)
            .ok_or_else(|| bad_why(NO_AGENT, "no_agent"))?;
        let dest_lane = if to == "main" {
            None
        } else {
            Some(
                self.lane_by(seen, to)
                    .ok_or_else(|| bad_why(NO_LANE, "no_lane"))?,
            )
        };
        let from_lane = seen.lane_of(agent);
        if from_lane.is_none() && !seen.on_main(agent) {
            return Err(unknown_owner());
        }
        let now_acct = match from_lane {
            Some(lane) => self.lane_acct(seen, lane),
            None => self.main_account(seen),
        };
        let dest = match dest_lane {
            Some(lane) => self.lane_acct(seen, lane),
            None => self.main_account(seen),
        };
        let mut owned = vec![Owned::Folder(agent.cwd.clone())];
        for acct in [&now_acct, &dest].into_iter().flatten() {
            owned.push(Owned::Account(Kind::Claude, acct.raw.clone()));
        }
        self.owners(&owned, mine)?;
        self.gate(true)?;
        let from = from_lane
            .and_then(|l| self.book.lane_id(&l.name))
            .unwrap_or_else(|| "main".to_string());
        if from == to {
            return Err(bad_why("That session already runs there.", "already_set"));
        }
        let ours = matches!(agent.target, AgentTarget::Job(_))
            && agent.transcript_path.is_some()
            && agent.label != "Stopped";
        if !ours {
            return Err(value_error(
                "Notchling can move only the sessions it started.",
            ));
        }
        let Some(dest) = dest else {
            return Err(forbidden(SIGNED_OUT, "signed_out"));
        };
        if dest.needs_login {
            return Err(forbidden(SIGNED_OUT, "signed_out"));
        }
        let shown = masked(&dest.email, &dest.id);
        let mut change = base("claude");
        change["session"] = json!(session);
        change["from"] = json!(from);
        change["to"] = json!(to);
        change["account"] = json!(dest.id);
        change["apply"] =
            json!({"op": ops::SESSION_MOVE, "args": {"id": session, "to_config_dir": to}});
        Ok(Plan {
            target: format!("move:{session}"),
            change,
            facts: json!([
                fact(
                    "consent-account-move-session",
                    json!({"name": agent.name, "folder": tilde(&agent.cwd, &seen.home),
                        "account": shown})
                ),
                fact(
                    "consent-account",
                    json!({"tool": "claude", "account": shown, "organization": mine})
                ),
            ]),
        })
    }

    pub(super) fn plan_lane_create(
        &self,
        seen: &Seen,
        id: &str,
        base: impl Fn(&str) -> Value,
        mine: &str,
    ) -> Result<Plan, ApiError> {
        let acct = self.found(seen, id)?;
        self.owners(&[Owned::Account(acct.kind, acct.raw.clone())], mine)?;
        self.gate(false)?;
        self.claude_only(&acct)?;
        if seen.lanes.iter().any(|l| l.account_id == acct.raw) {
            return Err(bad_why("That account already has a lane.", "already_set"));
        }
        if seen.snap.active.as_deref() == Some(acct.raw.as_str()) {
            return Err(bad_why("That account is the main login.", "main_login"));
        }
        if acct.needs_login {
            return Err(forbidden(SIGNED_OUT, "signed_out"));
        }
        let shown = masked(&acct.email, &acct.id);
        let mut change = base("claude");
        change["account"] = json!(acct.id);
        change["apply"] = json!({"op": ops::LANES_CREATE, "args": {"account": acct.id}});
        Ok(Plan {
            target: format!("lane_create:{}", acct.id),
            change,
            facts: json!([
                fact(
                    "consent-account",
                    json!({"tool": "claude", "account": shown, "organization": mine})
                ),
                fact("consent-lane-create", json!({"account": shown})),
            ]),
        })
    }

    pub(super) fn plan_lane_remove(
        &self,
        seen: &Seen,
        id: &str,
        base: impl Fn(&str) -> Value,
        mine: &str,
    ) -> Result<Plan, ApiError> {
        let lane = self
            .lane_by(seen, id)
            .ok_or_else(|| bad_why(NO_LANE, "no_lane"))?;
        let acct = self
            .lane_acct(seen, lane)
            .ok_or_else(|| bad_why(NO_LANE, "no_lane"))?;
        self.owners(&[Owned::Account(Kind::Claude, acct.raw.clone())], mine)?;
        self.gate(false)?;
        if seen.in_lane(lane).next().is_some() {
            return Err(bad_why(
                "Sessions are running in that lane. Move them first.",
                "in_use",
            ));
        }
        let shown = masked(&acct.email, &acct.id);
        let mut change = base("claude");
        change["lane"] = json!(id);
        change["account"] = json!(acct.id);
        change["apply"] = json!({"op": ops::LANES_REMOVE, "args": {"name": id}});
        Ok(Plan {
            target: format!("lane_remove:{id}"),
            change,
            facts: json!([
                fact(
                    "consent-account",
                    json!({"tool": "claude", "account": shown, "organization": mine})
                ),
                fact("consent-lane-close", json!({"account": shown})),
            ]),
        })
    }

    pub(super) fn plan_lane_switch(
        &self,
        seen: &Seen,
        id: &str,
        account: &str,
        base: impl Fn(&str) -> Value,
        mine: &str,
    ) -> Result<Plan, ApiError> {
        let lane = self
            .lane_by(seen, id)
            .ok_or_else(|| bad_why(NO_LANE, "no_lane"))?;
        let acct = self.found(seen, account)?;
        let before = self
            .lane_acct(seen, lane)
            .ok_or_else(|| bad_why(NO_LANE, "no_lane"))?;
        let mut owned = vec![
            Owned::Account(Kind::Claude, before.raw.clone()),
            Owned::Account(acct.kind, acct.raw.clone()),
        ];
        owned.extend(self.lane_folders(seen, lane));
        self.owners(&owned, mine)?;
        self.gate(false)?;
        self.claude_only(&acct)?;
        if lane.account_id == acct.raw {
            return Err(bad_why(
                "That lane already uses that account.",
                "already_set",
            ));
        }
        if seen.snap.active.as_deref() == Some(acct.raw.as_str())
            || seen.lanes.iter().any(|l| l.account_id == acct.raw)
        {
            return Err(bad_why(
                "That account is in use in the main login or another lane.",
                "in_use",
            ));
        }
        if acct.needs_login {
            return Err(forbidden(SIGNED_OUT, "signed_out"));
        }
        let shown = masked(&acct.email, &acct.id);
        let mut change = base("claude");
        change["lane"] = json!(id);
        change["account"] = json!(acct.id);
        change["before"] = json!(before.id);
        change["apply"] =
            json!({"op": ops::LANES_SWITCH, "args": {"name": id, "account": acct.id}});
        Ok(Plan {
            target: format!("lane_switch:{id}"),
            change,
            facts: json!([
                fact(
                    "consent-account",
                    json!({"tool": "claude", "account": shown, "organization": mine})
                ),
                fact(
                    "consent-lane-switch",
                    json!({"before": masked(&before.email, &before.id), "account": shown})
                ),
                fact(
                    "consent-lane-sessions",
                    json!({"count": seen.in_lane(lane).count()})
                ),
            ]),
        })
    }
}
