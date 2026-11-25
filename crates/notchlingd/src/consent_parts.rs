use crate::consent_ops::{forbidden, ConsentOps};
use notchling_engine::consent::{spec, Consent, State};
use notchling_local_api::ApiError;
use serde_json::{json, Value};

pub const MAX_PARTS: usize = 200;
pub const MAX_ID_CHARS: usize = 160;

fn bad_value(text: &str) -> ApiError {
    ApiError::bad_request(text).with_reason("bad_value")
}

fn has_parts(kind: &str) -> bool {
    spec(kind).is_some_and(|k| k.parts)
}

pub(crate) fn ids_of(parts: &Value) -> Option<Vec<String>> {
    parts
        .as_array()?
        .iter()
        .map(|part| part.get("id")?.as_str().map(str::to_string))
        .collect()
}

pub(crate) fn check_opening(kind: &str, parts: Option<&Value>) -> Result<(), ApiError> {
    match (has_parts(kind), parts) {
        (false, None) => Ok(()),
        (false, Some(_)) => Err(ApiError::internal("this kind has no parts")),
        (true, None) => Err(ApiError::internal("this kind needs its parts")),
        (true, Some(parts)) => {
            let ids = ids_of(parts)
                .filter(|ids| !ids.is_empty() && ids.len() <= MAX_PARTS)
                .ok_or_else(|| ApiError::internal("parts are a list of objects with an id"))?;
            let mut seen = std::collections::HashSet::new();
            let fits = ids
                .iter()
                .all(|id| !id.is_empty() && id.chars().count() <= MAX_ID_CHARS && seen.insert(id));
            if fits {
                Ok(())
            } else {
                Err(ApiError::internal("part ids are short and used once"))
            }
        }
    }
}

pub(crate) fn vet(
    row: &Consent,
    allow: bool,
    given: Option<&Value>,
) -> Result<Option<Value>, ApiError> {
    let has = has_parts(&row.kind);
    let Some(given) = given.filter(|v| !v.is_null()) else {
        return if allow && has {
            Err(bad_value("Name the parts to apply."))
        } else {
            Ok(None)
        };
    };
    if !has || !allow {
        return Err(bad_value("This request has no parts to choose."));
    }
    let offered = row.parts.as_ref().and_then(ids_of).unwrap_or_default();
    let named: Vec<&str> = given
        .as_array()
        .map(|list| list.iter().filter_map(Value::as_str).collect())
        .filter(|names: &Vec<&str>| names.len() == given.as_array().map_or(0, Vec::len))
        .ok_or_else(|| bad_value("Parts are a list of part ids."))?;
    if named.is_empty() {
        return Err(bad_value("Keep at least one part."));
    }
    if named
        .iter()
        .any(|name| !offered.iter().any(|id| id == name))
    {
        return Err(bad_value("A part you named is not in this request."));
    }
    let kept: Vec<&String> = offered
        .iter()
        .filter(|id| named.contains(&id.as_str()))
        .collect();
    Ok(Some(json!(kept)))
}

impl ConsentOps {
    pub fn kept_parts(row: &Consent, named: &[String]) -> Result<Vec<String>, ApiError> {
        if !matches!(row.state, State::Applying | State::Applied) {
            return Err(forbidden(
                "That request has not been allowed.",
                "needs_consent",
            ));
        }
        let allowed: Vec<&str> = row
            .parts_allowed
            .as_ref()
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let outside = named.iter().any(|name| !allowed.contains(&name.as_str()));
        if named.is_empty() || outside {
            return Err(forbidden(
                "That part was not allowed on the card.",
                "needs_consent",
            ));
        }
        Ok(allowed
            .into_iter()
            .filter(|id| named.iter().any(|name| name == id))
            .map(str::to_string)
            .collect())
    }
}

#[cfg(test)]
#[path = "consent_parts_tests.rs"]
mod tests;
