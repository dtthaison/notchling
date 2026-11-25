use std::path::PathBuf;

pub(crate) const ASK_USAGE: &str = "Usage: notchling ask [--choice <text>]... [--main-may-answer] [--wait] [--timeout <seconds>] [--json] [--socket <path>] -- <question>";
pub(crate) const ASKS_USAGE: &str = "Usage: notchling asks [--wait] [--json] [--socket <path>]";
pub(crate) const ANSWER_USAGE: &str =
    "Usage: notchling answer <id> (--choice <n> | -- <text>) [--json] [--socket <path>]";

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct AskArgs {
    pub question: String,
    pub choices: Vec<String>,
    pub main_may_answer: bool,
    pub wait: bool,
    pub timeout_s: Option<u64>,
    pub json: bool,
    pub socket: Option<PathBuf>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct AsksArgs {
    pub wait: bool,
    pub json: bool,
    pub socket: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AnswerArgs {
    pub id: String,
    pub choice: Option<u8>,
    pub text: Option<String>,
    pub json: bool,
    pub socket: Option<PathBuf>,
}

fn flag_value(rest: &mut std::slice::Iter<String>, usage: &str) -> Result<String, String> {
    match rest.next() {
        Some(v) if !v.is_empty() => Ok(v.clone()),
        _ => Err(usage.to_string()),
    }
}

pub(crate) fn parse_ask(args: &[String]) -> Result<AskArgs, String> {
    let mut parsed = AskArgs::default();
    let mut rest = args.iter();
    let mut question = None;
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--" => {
                let words: Vec<&str> = rest.by_ref().map(String::as_str).collect();
                question = Some(words.join(" "));
            }
            "--choice" => parsed.choices.push(flag_value(&mut rest, ASK_USAGE)?),
            "--main-may-answer" => parsed.main_may_answer = true,
            "--wait" => parsed.wait = true,
            "--json" => parsed.json = true,
            "--timeout" => {
                let seconds = flag_value(&mut rest, ASK_USAGE)?
                    .parse::<u64>()
                    .ok()
                    .filter(|s| *s > 0);
                parsed.timeout_s = Some(seconds.ok_or("--timeout takes a number of seconds.")?);
            }
            "--socket" => parsed.socket = Some(PathBuf::from(flag_value(&mut rest, ASK_USAGE)?)),
            other => return Err(format!("Unknown option {other}.\n{ASK_USAGE}")),
        }
    }
    parsed.question = question
        .filter(|q| !q.trim().is_empty())
        .ok_or_else(|| ASK_USAGE.to_string())?;
    Ok(parsed)
}

pub(crate) fn parse_asks(args: &[String]) -> Result<AsksArgs, String> {
    let mut parsed = AsksArgs::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--wait" => parsed.wait = true,
            "--json" => parsed.json = true,
            "--socket" => parsed.socket = Some(PathBuf::from(flag_value(&mut rest, ASKS_USAGE)?)),
            other => return Err(format!("Unknown option {other}.\n{ASKS_USAGE}")),
        }
    }
    Ok(parsed)
}

pub(crate) fn parse_answer(args: &[String]) -> Result<AnswerArgs, String> {
    let Some(id) = args.first().filter(|a| !a.starts_with("--")) else {
        return Err(ANSWER_USAGE.to_string());
    };
    let mut parsed = AnswerArgs {
        id: id.clone(),
        choice: None,
        text: None,
        json: false,
        socket: None,
    };
    let mut rest = args[1..].iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--" => {
                let words: Vec<&str> = rest.by_ref().map(String::as_str).collect();
                parsed.text = Some(words.join(" ")).filter(|t| !t.trim().is_empty());
                if parsed.text.is_none() {
                    return Err(ANSWER_USAGE.to_string());
                }
            }
            "--choice" => {
                let n = flag_value(&mut rest, ANSWER_USAGE)?
                    .parse::<u8>()
                    .ok()
                    .filter(|n| *n > 0);
                parsed.choice = Some(n.ok_or("--choice takes the number of a choice, from 1.")?);
            }
            "--json" => parsed.json = true,
            "--socket" => parsed.socket = Some(PathBuf::from(flag_value(&mut rest, ANSWER_USAGE)?)),
            other => return Err(format!("Unknown option {other}.\n{ANSWER_USAGE}")),
        }
    }
    if parsed.choice.is_some() == parsed.text.is_some() {
        return Err(ANSWER_USAGE.to_string());
    }
    Ok(parsed)
}
