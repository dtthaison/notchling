use super::*;

const CHOOSE_USAGE: &str = "session.choose takes {\"id\": text, \"answers\": [[option numbers] or text, one per question], \"question\": {\"items\": [{\"question\": text, \"options\": [text]}]}}";
const RUN_USAGE: &str =
    "session.run takes {\"id\": text, \"command\": \"/command with its arguments\"}";
const PICK_USAGE: &str =
    "session.pick takes {\"id\": text, \"number\": 1 to 9} or {\"id\": text, \"dismiss\": true}";
const BROWSE_USAGE: &str = "session.browse takes {\"id\": text, \"on\": true|false}";
const TUNE_USAGE: &str = "session.tune takes {\"id\": text, \"option\": text, \"value\": text}";
const COMMAND_BYTES: usize = 2_000;
const MAX_ITEMS: usize = 8;
const MAX_OPTIONS: usize = 30;
const NEVER_COMMANDS: [&str; 4] = [
    "/permissions",
    "/permission-mode",
    "/allowed-tools",
    "/permission",
];
const NEVER_WORDS: [&str; 5] = [
    "bypasspermissions",
    "dangerously",
    "skip-permissions",
    "skippermissions",
    "skip_permissions",
];
const NEVER_OPTION_WORDS: [&str; 3] = ["permission", "bypass", "yolo"];

fn never() -> ApiError {
    forbidden(NEVER_PROMPTS, "never")
}

fn loosens(text: &str) -> bool {
    let low = text.to_ascii_lowercase();
    NEVER_WORDS.iter().any(|w| low.contains(w))
}

fn loosens_option(text: &str) -> bool {
    let low = text.to_ascii_lowercase();
    loosens(text) || NEVER_OPTION_WORDS.iter().any(|w| low.contains(w))
}

pub(super) fn loosening_command(command: &str) -> bool {
    let name = command
        .trim()
        .strip_prefix('/')
        .unwrap_or_default()
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    NEVER_COMMANDS.contains(&format!("/{name}").as_str()) || loosens(command)
}

fn slash_command(args: &Value, usage: &str) -> Result<String, ApiError> {
    let command = text_of(args, "command", usage)?.trim().to_string();
    let name = command.strip_prefix('/').unwrap_or_default();
    let first = name.split(char::is_whitespace).next().unwrap_or_default();
    let tidy = !first.is_empty()
        && first
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'));
    if command.len() > COMMAND_BYTES || !tidy || command.contains(['\n', '\r', '\0']) {
        return Err(bad(usage));
    }
    Ok(command)
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn clean_items(raw: &Value) -> Option<Vec<Value>> {
    let list = raw
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= MAX_ITEMS)?;
    list.iter()
        .map(|item| {
            let question = item.get("question").and_then(Value::as_str)?;
            if question.trim().is_empty() || question.len() > MESSAGE_BYTES {
                return None;
            }
            let options = strings(item.get("options"));
            if options.len() > MAX_OPTIONS || options.iter().any(|o| o.len() > 1_000) {
                return None;
            }
            Some(json!({
                "header": item.get("header").and_then(Value::as_str).unwrap_or_default(),
                "question": question,
                "options": options,
                "details": strings(item.get("details")),
                "multiSelect": item.get("multiSelect").and_then(Value::as_bool).unwrap_or(false),
            }))
        })
        .collect()
}

fn question_key(items: &Value) -> Vec<(String, Vec<String>)> {
    items
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            (
                item.get("question")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                strings(item.get("options")),
            )
        })
        .collect()
}

fn chosen(items: &[Value], answers: &[Value]) -> Result<Vec<String>, ApiError> {
    if items.len() != answers.len() {
        return Err(bad(CHOOSE_USAGE));
    }
    items
        .iter()
        .zip(answers)
        .map(|(item, answer)| match answer {
            Value::String(text) if !text.trim().is_empty() && text.len() <= MESSAGE_BYTES => {
                Ok(text.clone())
            }
            Value::Array(numbers) => {
                let options = strings(item.get("options"));
                let multi = item["multiSelect"] == json!(true);
                if numbers.is_empty() || (!multi && numbers.len() != 1) {
                    return Err(bad(CHOOSE_USAGE));
                }
                let labels: Option<Vec<String>> = numbers
                    .iter()
                    .map(|n| {
                        let n = usize::try_from(n.as_u64()?).ok()?;
                        options.get(n.checked_sub(1)?).cloned()
                    })
                    .collect();
                labels
                    .map(|l| l.join(", "))
                    .ok_or_else(|| bad(CHOOSE_USAGE))
            }
            _ => Err(bad(CHOOSE_USAGE)),
        })
        .collect()
}

fn dialog_option(dialog: &Value, number: u64) -> Option<String> {
    dialog
        .get("options")
        .and_then(Value::as_array)?
        .iter()
        .find(|o| o.get("number").and_then(Value::as_u64) == Some(number))
        .and_then(|o| o.get("label").and_then(Value::as_str))
        .map(str::to_string)
}

impl SessionAsks {
    fn aimed(
        &self,
        caller: &Caller,
        args: &Value,
        usage: &str,
        tool: &str,
    ) -> Result<(Target, String), ApiError> {
        let id = text_of(args, "id", usage)?;
        let target = self.target_of(id)?;
        let who = self.consent.who(caller);
        let organization = self.check_org(&who, &target.folder)?;
        if target.tool != tool {
            return Err(bad("That can't be done to this kind of session.").with_reason("not_yet"));
        }
        Ok((target, organization))
    }

    fn raise(
        &self,
        caller: &Caller,
        args: &Value,
        action: Action,
        aimed: (Target, String),
        extra: Value,
    ) -> Result<Value, ApiError> {
        let (target, organization) = aimed;
        let mut change = json!({
            "action": action.word(), "tool": target.tool, "folder": target.folder,
            "name": target.name, "session": target.id, "organization": organization,
        });
        if let (Some(into), Some(from)) = (change.as_object_mut(), extra.as_object()) {
            into.extend(from.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        self.open(caller, args, action, &target.id, change, false)
    }

    fn read_question(&self, id: &str) -> Option<Vec<Value>> {
        let reply = (self.run)(ops::SESSION_QUESTION, &json!({"id": id})).ok()?;
        clean_items(reply.get("items")?)
    }

    fn read_dialog(&self, id: &str) -> Result<Value, ApiError> {
        let reply = (self.run)(ops::SESSION_DIALOG, &json!({"id": id}))?;
        match reply.get("dialog").filter(|d| !d.is_null()) {
            Some(dialog) => Ok(dialog.clone()),
            None if reply.get("busy").and_then(Value::as_bool) == Some(true) => {
                Err(bad("The session is busy. Try again.").with_reason("busy"))
            }
            None => Err(bad("No dialog is open in that session.").with_reason("no_dialog")),
        }
    }

    pub(super) fn choose(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let answers = args
            .get("answers")
            .and_then(Value::as_array)
            .filter(|a| !a.is_empty() && a.len() <= MAX_ITEMS)
            .ok_or_else(|| bad(CHOOSE_USAGE))?;
        self.gate(args)?;
        let aimed = self.aimed(caller, args, CHOOSE_USAGE, "claude")?;
        let items = match args.get("question").and_then(|q| q.get("items")) {
            Some(raw) => clean_items(raw).ok_or_else(|| bad(CHOOSE_USAGE))?,
            None => self.read_question(&aimed.0.id).ok_or_else(|| {
                bad("No question is open in that session.").with_reason("no_question")
            })?,
        };
        let labels = chosen(&items, answers)?;
        if labels.iter().any(|l| loosens_option(l)) {
            return Err(never());
        }
        let shown: Vec<String> = items
            .iter()
            .zip(&labels)
            .map(|(item, label)| format!("{}: {label}", item["question"].as_str().unwrap_or("")))
            .collect();
        let extra = json!({
            "question": {"items": items, "answers": answers},
            "message": shown.join("\n"),
        });
        self.raise(caller, args, Action::Choose, aimed, extra)
    }

    pub(super) fn run_command(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let command = slash_command(args, RUN_USAGE)?;
        if loosening_command(&command) {
            return Err(never());
        }
        self.gate(args)?;
        let aimed = self.aimed(caller, args, RUN_USAGE, "claude")?;
        self.raise(
            caller,
            args,
            Action::Run,
            aimed,
            json!({"message": command}),
        )
    }

    pub(super) fn pick(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let dismiss = args.get("dismiss").and_then(Value::as_bool) == Some(true);
        let number = args
            .get("number")
            .and_then(Value::as_u64)
            .filter(|n| (1..=9).contains(n));
        if dismiss == number.is_some() {
            return Err(bad(PICK_USAGE));
        }
        self.gate(args)?;
        let aimed = self.aimed(caller, args, PICK_USAGE, "claude")?;
        let dialog = self.read_dialog(&aimed.0.id)?;
        let title = dialog["title"].as_str().unwrap_or_default().to_string();
        let extra = match number {
            Some(n) => {
                let label = dialog_option(&dialog, n).ok_or_else(|| {
                    bad("The dialog has no such option.").with_reason("bad_value")
                })?;
                if loosens_option(&label) {
                    return Err(never());
                }
                json!({"dismiss": false, "number": n, "title": title, "label": label,
                    "message": format!("{title}\n{n}. {label}")})
            }
            None => json!({"dismiss": true, "title": title, "message": title}),
        };
        self.raise(caller, args, Action::Pick, aimed, extra)
    }

    pub(super) fn browse(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let on = args
            .get("on")
            .and_then(Value::as_bool)
            .ok_or_else(|| bad(BROWSE_USAGE))?;
        self.gate(args)?;
        let aimed = self.aimed(caller, args, BROWSE_USAGE, "claude")?;
        let command = if on { "/chrome" } else { "/chrome off" };
        self.raise(
            caller,
            args,
            Action::Browse,
            aimed,
            json!({"on": on, "message": command}),
        )
    }

    pub(super) fn tune(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let word = |key: &str| -> Result<String, ApiError> {
            let text = text_of(args, key, TUNE_USAGE)?;
            if word_ok(text) {
                Ok(text.to_string())
            } else {
                Err(bad(TUNE_USAGE))
            }
        };
        let (option, value) = (word("option")?, word("value")?);
        if loosens_option(&option) || loosens_option(&value) {
            return Err(never());
        }
        self.gate(args)?;
        let aimed = self.aimed(caller, args, TUNE_USAGE, "antigravity")?;
        self.raise(
            caller,
            args,
            Action::Tune,
            aimed,
            json!({"option": option, "value": value, "message": format!("{option} = {value}")}),
        )
    }

    pub(super) fn act(&self, action: Action, row: &Consent, id: &str) -> Result<Value, ApiError> {
        let change = &row.change;
        match action {
            Action::Run => (self.run)(
                ops::SESSION_COMMAND,
                &json!({"id": id, "command": change["message"]}),
            ),
            Action::Browse => {
                let command = if change["on"] == json!(true) {
                    "/chrome"
                } else {
                    "/chrome off"
                };
                (self.run)(ops::SESSION_COMMAND, &json!({"id": id, "command": command}))
            }
            Action::Pick if change["dismiss"] == json!(true) => {
                (self.run)(ops::SESSION_DIALOG_DISMISS, &json!({"id": id}))
            }
            Action::Pick => (self.run)(
                ops::SESSION_DIALOG_PICK,
                &json!({"id": id, "title": change["title"], "number": change["number"],
                    "label": change["label"]}),
            ),
            Action::Choose => (self.run)(
                ops::SESSION_ANSWER,
                &json!({"id": id, "answers": change["question"]["answers"],
                    "question": {"items": change["question"]["items"]}}),
            ),
            Action::Tune => (self.run)(
                ops::AGY_SET_OPTION,
                &json!({"id": id, "option": change["option"], "value": change["value"]}),
            ),
            _ => Err(ApiError::internal("not a session act")),
        }
    }

    pub(super) fn unchanged(&self, row: &Consent, id: &str) -> Result<(), String> {
        let change = &row.change;
        match change["action"].as_str() {
            Some("pick") => {
                let dialog = self.read_dialog(id).map_err(|e| e.message)?;
                if dialog["title"] != change["title"] {
                    return Err("the dialog changed".to_string());
                }
                if change["dismiss"] != json!(true) {
                    let now = change["number"]
                        .as_u64()
                        .and_then(|n| dialog_option(&dialog, n));
                    if now.as_deref() != change["label"].as_str() {
                        return Err("the dialog changed".to_string());
                    }
                }
                Ok(())
            }
            Some("choose") => {
                let now = (self.run)(ops::SESSION_QUESTION, &json!({"id": id}))
                    .map_err(|_| "the question is gone".to_string())?;
                let asked = &change["question"]["items"];
                let screen = now.get("items").unwrap_or(&Value::Null);
                if question_key(asked) == question_key(screen) {
                    Ok(())
                } else {
                    Err("the question changed".to_string())
                }
            }
            _ => Ok(()),
        }
    }
}
