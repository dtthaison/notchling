use super::*;
use notchling_engine::search::facets::{tally, FacetValue};
use notchling_engine::search::query::filter::{Filter, NAMES};
use notchling_engine::search::SessionMeta;

const DEFAULT_VALUES: usize = 20;
const MAX_VALUES: usize = 100;

pub(super) fn filter_of(args: &Value) -> Result<Filter, ApiError> {
    match args.get("filter") {
        None => Ok(Filter::default()),
        Some(value) => Filter::from_value(value).map_err(ApiError::bad_request),
    }
}

pub(super) fn page_facets(e: &PageEntry, masked: bool) -> Vec<(&'static str, String)> {
    let owner = if masked {
        search_redact::mask(&e.account)
    } else {
        e.account.clone()
    };
    let mut out = Vec::new();
    if !owner.is_empty() {
        out.push(("owner", owner));
    }
    if !e.project.is_empty() {
        out.push(("project", e.project.clone()));
    }
    out
}

pub(super) fn session_facets(meta: &SessionMeta, masked: bool) -> Vec<(&'static str, String)> {
    meta.facets()
        .into_iter()
        .map(|(name, value)| {
            if masked && name == "account" {
                (name, search_redact::mask(&value))
            } else {
                (name, value)
            }
        })
        .collect()
}

pub(super) fn json_of(pairs: &[(&'static str, String)]) -> Value {
    let mut map = serde_json::Map::new();
    for (name, value) in pairs {
        map.insert((*name).to_owned(), json!(value));
    }
    Value::Object(map)
}

pub(super) fn shown_title(page: &PageEntry, masked: bool) -> String {
    if masked {
        search_redact::mask(&page.title)
    } else {
        page.title.clone()
    }
}

pub(super) fn found_in(query: &str, entries: &[PageEntry], masked: bool) -> Vec<PageEntry> {
    if !masked {
        return find_in(query, entries);
    }
    let view: Vec<PageEntry> = entries
        .iter()
        .map(|e| PageEntry {
            title: shown_title(e, true),
            account: search_redact::mask(&e.account),
            ..e.clone()
        })
        .collect();
    let by_key: HashMap<String, &PageEntry> = entries.iter().map(|e| (page_key(e), e)).collect();
    find_in(query, &view)
        .iter()
        .filter_map(|v| by_key.get(&page_key(v)).map(|e| (*e).clone()))
        .collect()
}

pub(super) fn page_matches(filter: &Filter, page: &PageEntry) -> bool {
    let facets = page_facets(page, false);
    let pairs: Vec<(&str, &str)> = facets.iter().map(|(n, v)| (*n, v.as_str())).collect();
    filter.matches(&pairs, Some(page.updated))
}

fn value_json(v: &FacetValue) -> Value {
    json!({"value": v.value, "label": v.label, "count": v.count})
}

impl PagesOps {
    pub(super) fn unmasked(&self, mut filter: Filter, reach: Option<&Reach>) -> Filter {
        let Some(reach) = reach else {
            return filter;
        };
        if let Some(wanted) = filter.account.take() {
            let raws: Vec<String> = self
                .index
                .facet_values("account", "", usize::MAX, &|m| {
                    reach.session(&m.folder, m.modified)
                })
                .into_iter()
                .map(|v| v.value)
                .collect();
            filter.account = Some(expand(&wanted, &raws));
        }
        if let Some(wanted) = filter.owner.take() {
            let raws: Vec<String> = self
                .reachable(self.library.entries(), Some(reach))
                .iter()
                .map(|e| e.account.clone())
                .collect();
            filter.owner = Some(expand(&wanted, &raws));
        }
        filter
    }

    pub(super) fn search_facets(
        &self,
        args: &Value,
        reach: Option<&Reach>,
    ) -> Result<Value, ApiError> {
        let usage = || {
            ApiError::bad_request(
                "search.facets takes {\"corpus\": \"sessions\"|\"pages\", \"name\": string, \"prefix\": string, \"limit\": int}",
            )
        };
        let corpus = args
            .get("corpus")
            .and_then(Value::as_str)
            .ok_or_else(usage)?;
        let name = args.get("name").and_then(Value::as_str).ok_or_else(usage)?;
        if !NAMES.contains(&name) {
            return Err(ApiError::bad_request(format!(
                "name is one of {}",
                NAMES.join(", ")
            )));
        }
        let prefix = match args.get("prefix") {
            None | Some(Value::Null) => "",
            Some(Value::String(s)) => s.as_str(),
            Some(_) => return Err(usage()),
        };
        let limit = match args.get("limit") {
            None | Some(Value::Null) => DEFAULT_VALUES,
            Some(v) => (v.as_u64().ok_or_else(usage)? as usize).clamp(1, MAX_VALUES),
        };
        let settings = self.settled();
        let mut reply = json!({"values": [], "disabled": [], "dropped": []});
        let category = match Category::from_id(corpus) {
            Some(c) if ANSWERED.contains(&c) => c,
            _ => {
                reply["dropped"] = json!([corpus]);
                return Ok(reply);
            }
        };
        if !settings.on.contains(&category) {
            reply["disabled"] = json!([corpus]);
            return Ok(reply);
        }
        let masked = reach.is_some();
        let shown = |v: String| {
            if masked && matches!(name, "account" | "owner") {
                search_redact::mask(&v)
            } else {
                v
            }
        };
        let values = if category == Category::Sessions {
            let first = self.index.is_empty() && !self.index.is_refreshing();
            self.index.load_cache();
            self.refresh(first.then_some(FIRST_REFRESH_WAIT));
            self.index.facet_values_as(
                name,
                prefix,
                limit,
                &|m| reach.is_none_or(|r| r.session(&m.folder, m.modified)),
                &shown,
            )
        } else {
            let values: Vec<String> = self
                .reachable(self.library.entries(), reach)
                .iter()
                .flat_map(|e| page_facets(e, false))
                .filter(|(n, _)| *n == name)
                .map(|(_, v)| shown(v))
                .collect();
            tally(values, name, prefix, limit)
        };
        reply["values"] = Value::Array(values.iter().map(value_json).collect());
        Ok(reply)
    }
}

fn expand(wanted: &[String], raws: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in wanted {
        let before = out.len();
        for raw in raws {
            if search_redact::mask(raw) == *value && !out.contains(raw) {
                out.push(raw.clone());
            }
        }
        if out.len() == before && search_redact::mask(value) == *value {
            out.push(value.clone());
        }
    }
    out
}
