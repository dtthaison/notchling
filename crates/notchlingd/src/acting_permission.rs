use crate::acting::ACTING_OFF;
use crate::acting_events::{session_ref, who, Who};
use crate::acting_ops::{
    answer_of, bad, expect_arg, forbidden, now_ms, question_arg, switch_off, text_arg,
    typed_or_picked, ActingOps, NoLog,
};
use crate::prompts::{question_json, Kind, Prompt};
use notchling_engine::reply::plan::{self, Decision};
use notchling_engine::reply::question::{choices_from_json, hook_answers};
use notchling_engine::reply::read::read_question;
use notchling_engine::sessions::jobs::config_dir_for_job;
use notchling_engine::sessions::AgentTarget;
use notchling_events::kinds::agents::{AnswerKind, PromptKind};
use notchling_events::{Ref, Session};
use notchling_local_api::{topics, ApiError, Caller, ClientKind, ErrorCode, Tier};
use serde_json::{json, Value};

impl ActingOps {
    pub(crate) fn permission(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let decision = text_arg(args, "decision")?;
        if decision == "always" && caller.client == ClientKind::Node {
            return Err(bad(
                "always can't be granted from another node; allow once, or answer on this machine",
            ));
        }
        let who = who(caller, args);
        let Some(prompt) = self.held(args)? else {
            if args.get("expect").is_some() {
                return self.permission_on_screen(args, decision, &who);
            }
            return Err(bad(
                "no prompt with that id is waiting (answer by the prompt's id, or pass expect to answer a row's dialog)",
            ));
        };
        let response = match (decision, prompt.kind) {
            ("deny", _) => match args.get("message").and_then(Value::as_str) {
                Some(message) => json!({"action": "deny", "message": message}),
                None => json!({"action": "deny"}),
            },
            ("allow", Kind::Permission) => json!({"action": "allow"}),
            ("always", Kind::Permission) => json!({"action": "allow", "always": true}),
            ("allow" | "always", Kind::Question) => {
                return Err(bad("a question is answered with session.answer"))
            }
            _ => return Err(bad("decision is allow, deny or always")),
        };
        self.decide(&prompt, response, decision)?;
        self.note_prompt(&prompt, answer_of(decision), &who);
        Ok(json!({"decided": decision, "promptId": prompt.id}))
    }

    fn note_prompt(&self, prompt: &Prompt, answer: AnswerKind, who: &Who) {
        let kind = match prompt.kind {
            Kind::Question => PromptKind::Question,
            Kind::Permission => PromptKind::Permission,
        };
        let tool = prompt.event.get("tool_name").and_then(Value::as_str);
        let waited = u64::try_from(now_ms().saturating_sub(prompt.created_ms)).ok();
        self.note_answered(
            Ref::<Session>::new(&prompt.session_id),
            kind,
            tool,
            answer,
            who,
            waited,
        );
    }

    fn permission_on_screen(
        &self,
        args: &Value,
        decision: &str,
        who: &Who,
    ) -> Result<Value, ApiError> {
        let spoken = decision;
        let decision =
            Decision::parse(decision).ok_or_else(|| bad("decision is allow, deny or always"))?;
        if args.get("message").is_some() {
            return Err(bad(
                "a deny message can't be typed into the terminal's dialog; deny without one",
            ));
        }
        let expect = expect_arg(args)?;
        let agent = self.agent(args)?;
        self.locked(&agent, |agent| {
            self.play_locked(
                agent,
                &plan::permission(decision, &expect),
                &mut NoLog,
                Some(&expect),
            )
        })?;
        self.note_answered(
            session_ref(&agent),
            PromptKind::Permission,
            Some(&expect.tool),
            answer_of(spoken),
            who,
            None,
        );
        Ok(json!({"decided": decision.word(), "answered": "terminal"}))
    }

    pub(crate) fn answer(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let who = who(caller, args);
        let answers = args
            .get("answers")
            .ok_or_else(|| bad("answers is needed"))?;
        if let Some(prompt) = self.held(args)? {
            let question = prompt
                .question
                .clone()
                .ok_or_else(|| bad("that prompt isn't a question"))?;
            let choices = choices_from_json(answers, &question.items).map_err(bad)?;
            let response =
                json!({"action": "answer", "answers": hook_answers(&choices, &question)});
            self.decide(&prompt, response, "answer")?;
            self.note_prompt(&prompt, typed_or_picked(answers), &who);
            return Ok(json!({"answered": "hook", "promptId": prompt.id}));
        }
        let agent = self.agent(args)?;
        let question = question_arg(args)?;
        let choices = choices_from_json(answers, &question.items).map_err(bad)?;
        self.play(&agent, &plan::answer(&choices, &question), &mut NoLog)?;
        self.note_answered(
            session_ref(&agent),
            PromptKind::Question,
            None,
            typed_or_picked(answers),
            &who,
            None,
        );
        Ok(json!({"answered": "terminal"}))
    }

    fn decide(&self, prompt: &Prompt, response: Value, outcome: &str) -> Result<(), ApiError> {
        let resolved = self
            .prompts
            .resolve(&prompt.id, response)
            .ok_or_else(|| bad("that prompt was already answered or given up"))?;
        self.resolved(&resolved, outcome, "app");
        Ok(())
    }

    pub(crate) fn resolved(&self, prompt: &Prompt, outcome: &str, by: &str) {
        let data =
            json!({"id": prompt.id, "sessionId": prompt.session_id, "outcome": outcome, "by": by});
        self.room
            .publish(topics::PROMPTS, topics::PROMPT_RESOLVED, &data);
    }

    pub(crate) fn question(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        if let Some(prompt) = self.held_for_reading(args)? {
            if let Some(q) = &prompt.question {
                let mut out = question_json(q);
                out["promptId"] = json!(prompt.id);
                return Ok(out);
            }
        }
        if !self.acting.is_on() {
            return Err(switch_off("acting", ACTING_OFF));
        }
        if caller.tier != Tier::App {
            return Err(forbidden(
                "reading a question off the screen moves its tabs: only the app does it",
            ));
        }
        let agent = self.agent(args)?;
        let no_terminal = || bad("this session has no terminal to read");
        if !matches!(&agent.target, AgentTarget::Job(_) if agent.can_reply()) {
            return Err(no_terminal());
        }
        self.locked(&agent, |agent| {
            let job = match &agent.target {
                AgentTarget::Job(now) if agent.can_reply() => now.clone(),
                _ => return Err(no_terminal()),
            };
            let config_dir =
                config_dir_for_job(&self.roots, &job).or_else(|| agent.config_dir.clone());
            let mut pty = self.attacher.attach(&job, config_dir.as_deref())?;
            let read = read_question(pty.as_mut(), &job)
                .map_err(|f| ApiError::new(ErrorCode::Failed, f.to_string()))?;
            Ok(match read {
                Some(q) => {
                    let mut out = question_json(&q);
                    out["id"] = json!(q.tool_use_id);
                    out
                }
                None => json!({"items": []}),
            })
        })
    }
}
