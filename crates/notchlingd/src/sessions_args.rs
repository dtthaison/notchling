use notchling_local_api::ApiError;
use serde_json::Value;

pub(crate) fn every_tool(args: &Value) -> Result<bool, ApiError> {
    match args.get("tools") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::String(tools)) if tools == "claude" => Ok(false),
        Some(Value::String(tools)) if tools == "all" => Ok(true),
        Some(_) => Err(ApiError::bad_request(
            "sessions.list takes {\"tools\": \"all\" | \"claude\"}",
        )),
    }
}

pub(crate) fn with_remote_kinds(args: &Value) -> Result<(bool, bool), ApiError> {
    let usage = "sessions.list takes {\"remote\": \"shell\" | \"codex\" | [\"shell\", \"codex\"]}";
    let kinds: Vec<&str> = match args.get("remote") {
        None | Some(Value::Null) => return Ok((false, false)),
        Some(Value::String(kind)) => vec![kind.as_str()],
        Some(Value::Array(list)) => list
            .iter()
            .map(|k| k.as_str().ok_or_else(|| ApiError::bad_request(usage)))
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(ApiError::bad_request(usage)),
    };
    if kinds.iter().any(|k| *k != "shell" && *k != "codex") {
        return Err(ApiError::bad_request(usage));
    }
    Ok((kinds.contains(&"shell"), kinds.contains(&"codex")))
}

pub(crate) fn tagged(list: Value, tool: &str) -> Vec<Value> {
    let Value::Array(rows) = list else {
        return Vec::new();
    };
    rows.into_iter()
        .map(|mut row| {
            row["tool"] = Value::String(tool.to_string());
            row
        })
        .collect()
}
