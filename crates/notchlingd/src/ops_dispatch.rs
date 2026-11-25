use super::*;

impl Ops {
    pub(super) fn dispatch(
        &self,
        caller: &Caller,
        op: &str,
        args: &Value,
    ) -> Result<Value, ApiError> {
        if let Some(bridge) = self.bridge.as_ref().filter(|b| b.spec(op).is_some()) {
            return bridge.call(op, args);
        }
        if let Some(codex) = self.codex.as_ref().filter(|c| c.spec(op).is_some()) {
            if op == ops::CODEX_SEND
                && args.get("job").is_some_and(|j| !j.is_null())
                && caller.client != ClientKind::Node
            {
                return Err(ApiError::bad_request(
                    "job is set by the cluster, not by a caller",
                ));
            }
            return codex.call(op, args);
        }
        if let Some(agy) = self.agy.as_ref().filter(|a| a.spec(op).is_some()) {
            return agy.call(op, args);
        }
        if let Some(pages) = self.pages.as_ref().filter(|p| p.spec(op).is_some()) {
            if caller.tier != Tier::App {
                return crate::search_scope::call(self.reads.as_deref(), pages, caller, op, args);
            }
            return pages.call(op, args);
        }
        if let Some(group) = self.group(op) {
            return group.call(caller, op, args);
        }
        match (op, &self.acting) {
            (ops::SESSIONS_LIST, _) => self.sessions_list(caller, args).map(|(list, _)| list),
            (ops::PROMPTS_LIST, Some(acting)) => {
                let local = acting.call(caller, op, args)?;
                Ok(match &self.prompts {
                    Some(prompts) => prompts.merge(local, caller),
                    None => local,
                })
            }
            (op, Some(acting)) => acting.call(caller, op, args),
            (op, None) => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    pub(super) fn sessions_list(
        &self,
        caller: &Caller,
        args: &Value,
    ) -> Result<(Value, bool), ApiError> {
        let every = every_tool(args)?;
        let (shell, codex) = with_remote_kinds(args)?;
        let landed = self.first.wait(self.first_wait);
        let list = self
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let list = if every {
            let mut rows = tagged(list, CLAUDE);
            if let Some(other) = &self.every {
                rows.extend(other.values(crate::acting_ops::now_ms()));
            }
            if let Some(shells) = &self.shells {
                rows.extend(shells.rows());
            }
            Value::Array(rows)
        } else {
            list
        };
        match &self.remote {
            Some(remote) if caller.client != ClientKind::Node && every => {
                Ok((remote.merge_all(&list), landed))
            }
            Some(remote) if caller.client != ClientKind::Node && (shell || codex) => {
                Ok((remote.merge_kinds(&list, shell, codex), landed))
            }
            Some(remote) if caller.client != ClientKind::Node => Ok((remote.merge(&list), landed)),
            _ => Ok((list, landed)),
        }
    }
}
