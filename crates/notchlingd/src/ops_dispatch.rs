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
            if op == ops::SEARCH_QUERY && caller.tier != Tier::App {
                let Some((reads, org)) = self
                    .reads
                    .as_ref()
                    .and_then(|r| r.search_org(caller).map(|org| (r, org)))
                else {
                    return Ok(crate::reads_ops::no_session_search(args));
                };
                let mut reply = pages.call(op, args)?;
                crate::reads_ops::scope_search(&mut reply, Some(&org), args, &|f| {
                    reads.folder_org(f)
                });
                return Ok(reply);
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
}
