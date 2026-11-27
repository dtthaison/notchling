use super::*;
use notchling_engine::search::hybrid::{fuse, plan, Lists, Match, Ranked, Semantic};
use notchling_engine::search::query::filter::Filter;

const OPENING_CHARS: usize = 200;
const SETTINGS_CAP: usize = 20;
const MEANING_REPLY_BYTES: usize = 90_000;
const CORPUS_SETTINGS: &str = "settings";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Text,
    Hybrid,
    Semantic,
}

fn mode_of(args: &Value) -> Result<Mode, ApiError> {
    match args.get("mode") {
        None | Some(Value::Null) => Ok(Mode::Text),
        Some(Value::String(m)) if m == "text" => Ok(Mode::Text),
        Some(Value::String(m)) if m == "hybrid" => Ok(Mode::Hybrid),
        Some(Value::String(m)) if m == "semantic" => Ok(Mode::Semantic),
        Some(_) => Err(ApiError::bad_request(
            "search.query mode is \"text\", \"hybrid\" or \"semantic\"",
        )),
    }
}

fn match_name(matched: Match) -> &'static str {
    match matched {
        Match::Text => "text",
        Match::Semantic => "semantic",
        Match::Both => "both",
    }
}

fn within(rows: Vec<Value>, budget: usize) -> (Vec<Value>, bool) {
    let mut bytes = 0;
    let mut out = Vec::new();
    for row in rows {
        bytes += row.to_string().len() + 1;
        if bytes > budget {
            return (out, true);
        }
        out.push(row);
    }
    (out, false)
}

fn opening(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(OPENING_CHARS).collect()
}

struct Meaning {
    state: Semantic,
    lists: HashMap<String, Lists>,
}

impl Meaning {
    fn list(&self, corpus: &str) -> Option<&Lists> {
        self.lists
            .get(corpus)
            .filter(|l| l.granite.is_some() || l.statics.is_some())
    }
}

fn fused(
    mode: Mode,
    text_keys: Vec<String>,
    lists: Option<&Lists>,
    cap: usize,
    age: &dyn Fn(&str) -> Option<f64>,
    keep: &dyn Fn(&str) -> bool,
) -> Vec<Ranked> {
    match lists {
        None if mode == Mode::Semantic => Vec::new(),
        None => text_keys
            .into_iter()
            .take(cap)
            .map(|key| Ranked {
                key,
                score: 0.0,
                matched: Match::Text,
                chunk: None,
            })
            .collect(),
        Some(lists) => {
            let mut lists = lists.clone();
            lists.text = if mode == Mode::Semantic {
                Vec::new()
            } else {
                text_keys
            };
            for hits in [&mut lists.granite, &mut lists.statics]
                .into_iter()
                .flatten()
            {
                hits.retain(|h| keep(&h.key));
            }
            fuse(&lists, cap, age).into_iter().take(cap).collect()
        }
    }
}

impl PagesOps {
    fn meaning(
        &self,
        text: &str,
        mode: Mode,
        filter: &Filter,
        corpora: &[&str],
        allow: &dyn Fn(&str, &str) -> bool,
    ) -> Meaning {
        let Some(embed) = self.embed.get() else {
            return Meaning {
                state: Semantic::Off,
                lists: HashMap::new(),
            };
        };
        let state = embed.state();
        let asks = !corpora.is_empty()
            && !words(text).is_empty()
            && (mode == Mode::Semantic || plan(text, state).statics);
        if !asks {
            return Meaning {
                state,
                lists: HashMap::new(),
            };
        }
        let (state, lists) = embed.query_lists_within(text, filter, corpora, allow);
        Meaning { state, lists }
    }

    fn meaning_snippet(&self, chunk: Option<[u8; 16]>) -> Option<String> {
        let text = self.embed.get()?.chunk_text(&chunk?)?;
        Some(opening(&text))
    }

    pub(super) fn search(&self, args: &Value, reach: Option<&Reach>) -> Result<Value, ApiError> {
        let usage = || {
            ApiError::bad_request(
                "search.query takes {\"text\": string, \"limit\": int, \"kind\": string, \"since\": seconds|duration|date, \"corpora\": [string], \"mode\": \"text\"|\"hybrid\"|\"semantic\"}",
            )
        };
        let mode = mode_of(args)?;
        let text = match args.get("text") {
            None | Some(Value::Null) => "",
            Some(Value::String(s)) => s.as_str(),
            Some(_) => return Err(usage()),
        };
        let limit = match args.get("limit") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64().ok_or_else(usage)? as usize),
        };
        let kind = args.get("kind").and_then(Value::as_str);
        let filter = facets::filter_of(args)?;
        let now = (self.now)();
        let since = match args.get("since") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(
                search_since::parse(s, now)
                    .ok_or_else(|| ApiError::bad_request(search_since::USAGE))?,
            ),
            Some(v) => Some(
                v.as_f64()
                    .filter(|s| s.is_finite() && *s >= 0.0)
                    .ok_or_else(usage)?,
            ),
        };
        let asked: Vec<String> = match args.get("corpora") {
            None | Some(Value::Null) => vec!["pages".into(), "sessions".into()],
            Some(Value::Array(ids)) => ids
                .iter()
                .map(|id| id.as_str().map(str::to_string).ok_or_else(usage))
                .collect::<Result<_, _>>()?,
            Some(_) => return Err(usage()),
        };
        let settings = self.settled();
        let mut want: Vec<Category> = Vec::new();
        let (mut disabled, mut dropped): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
        let mut seen_ids: HashSet<&str> = HashSet::new();
        let mut want_settings = false;
        for id in &asked {
            if !seen_ids.insert(id.as_str()) {
                continue;
            }
            if id == CORPUS_SETTINGS {
                if reach.is_some() {
                    dropped.push(id.clone());
                } else if !settings.on.contains(&Category::Settings) {
                    disabled.push(id.clone());
                } else if mode == Mode::Semantic {
                    want_settings = true;
                } else {
                    dropped.push(id.clone());
                }
                continue;
            }
            match Category::from_id(id) {
                None => dropped.push(id.clone()),
                Some(c) if !settings.on.contains(&c) => disabled.push(id.clone()),
                Some(c) if ANSWERED.contains(&c) => want.push(c),
                Some(_) => dropped.push(id.clone()),
            }
        }
        let want_pages = want.contains(&Category::Pages) && kind != Some("sessions");
        let want_sessions = want.contains(&Category::Sessions) && kind != Some("pages");
        let sessions_on = settings.on.contains(&Category::Sessions);
        let query_words = words(text);
        let empty = query_words.is_empty();
        let session_limit = limit
            .unwrap_or(if empty { 6 } else { 30 })
            .min(MAX_SESSIONS);
        let page_limit = (if empty { 6 } else { 20 }).min(limit.unwrap_or(usize::MAX));
        let masked = reach.is_some();
        let by_text = mode != Mode::Semantic;

        if sessions_on {
            let first = self.index.is_empty() && !self.index.is_refreshing();
            self.index.load_cache();
            self.refresh(first.then_some(FIRST_REFRESH_WAIT));
        }
        let filter = self.unmasked(filter, reach);
        let live = |m: &SessionMeta| {
            reach.is_none_or(|r| r.session(&m.folder, m.modified))
                && since.is_none_or(|t| m.modified >= t)
                && m.matches(&filter)
        };

        let mut vector_corpora: Vec<&str> = Vec::new();
        if want_pages {
            vector_corpora.push("pages");
        }
        if want_sessions {
            vector_corpora.push("sessions");
        }
        if want_settings {
            vector_corpora.push(CORPUS_SETTINGS);
        }
        let entries: Vec<PageEntry> = self
            .reachable(self.library.entries(), reach)
            .into_iter()
            .filter(|p| since.is_none_or(|t| p.updated >= t))
            .filter(|p| facets::page_matches(&filter, p))
            .collect();
        let meaning = if mode == Mode::Text {
            None
        } else {
            let restricted = reach.is_some() || since.is_some() || !filter.is_empty();
            let page_keys: HashSet<String> = entries.iter().map(page_key).collect();
            let session_keys: Option<HashSet<String>> = want_sessions.then(|| {
                if restricted {
                    self.index.keys_where(&live)
                } else {
                    self.index.keys_where(&|_| true)
                }
            });
            let allow = |corpus: &str, key: &str| match corpus {
                "pages" => page_keys.contains(key),
                "sessions" => session_keys.as_ref().is_none_or(|keys| keys.contains(key)),
                _ => true,
            };
            Some(self.meaning(text, mode, &filter, &vector_corpora, &allow))
        };

        let found = if !want_pages || !by_text {
            Vec::new()
        } else {
            facets::found_in(text, &entries, masked)
        };
        let (in_title, rest): (Vec<PageEntry>, Vec<PageEntry>) = found.into_iter().partition(|p| {
            let title = notchling_engine::search::lowercase(&facets::shown_title(p, masked));
            query_words.iter().all(|w| title.contains(w.as_str()))
        });
        let content = if want_pages && by_text {
            self.content_hits(&entries, &query_words, kind, now)
        } else {
            Vec::new()
        };
        let content_keys: HashSet<String> = content.iter().map(page_key).collect();
        let mut seen: HashSet<String> = HashSet::new();
        let ordered: Vec<&PageEntry> = in_title
            .iter()
            .chain(rest.iter())
            .chain(content.iter())
            .filter(|p| seen.insert(page_key(p)))
            .collect();
        let page_row = |p: &PageEntry| -> Value {
            let mut row = page_json_for(&self.library, p, masked);
            if content_keys.contains(&page_key(p)) {
                let snippet = self.index.page_snippet(p, &query_words);
                let (snippet, marks) = marked(snippet, &query_words, masked);
                row["snippet"] = json!(snippet);
                row["highlights"] = Value::Array(marks);
            }
            row
        };
        let pages: Vec<Value> = match &meaning {
            None => ordered
                .iter()
                .take(page_limit)
                .map(|p| page_row(p))
                .collect(),
            Some(meaning) => {
                let by_key: HashMap<String, &PageEntry> =
                    entries.iter().map(|e| (page_key(e), e)).collect();
                let text_keys: Vec<String> = ordered.iter().map(|p| page_key(p)).collect();
                let text_set: HashSet<&str> = text_keys.iter().map(String::as_str).collect();
                let age = |key: &str| by_key.get(key).map(|e| (now - e.updated).max(0.0));
                fused(
                    mode,
                    text_keys.clone(),
                    meaning.list("pages"),
                    page_limit,
                    &age,
                    &|key| by_key.contains_key(key),
                )
                .into_iter()
                .filter_map(|ranked| {
                    let entry = *by_key.get(&ranked.key)?;
                    let mut row = if text_set.contains(ranked.key.as_str()) && by_text {
                        page_row(entry)
                    } else {
                        let mut row = page_json_for(&self.library, entry, masked);
                        let (snippet, marks) =
                            marked(self.meaning_snippet(ranked.chunk), &[], masked);
                        row["snippet"] = json!(snippet);
                        row["highlights"] = Value::Array(marks);
                        row
                    };
                    row["match"] = json!(match_name(ranked.matched));
                    Some(row)
                })
                .collect()
            }
        };
        let scoped: Vec<SessionHit> = if !want_sessions || !by_text {
            Vec::new()
        } else {
            let pool = if reach.is_some() || since.is_some() || mode != Mode::Text {
                POOL.max(session_limit)
            } else {
                session_limit
            };
            self.index
                .search_filtered(&query_words, &filter, now, pool)
                .into_iter()
                .filter(|h| reach.is_none_or(|r| r.session(&h.meta.folder, h.meta.modified)))
                .filter(|h| since.is_none_or(|t| h.meta.modified >= t))
                .collect()
        };
        let mut cut = false;
        let sessions: Vec<Value> = match &meaning {
            None => scoped
                .iter()
                .take(session_limit)
                .map(|hit| session_json_for(hit, &query_words, masked))
                .collect(),
            Some(meaning) => {
                let text_keys: Vec<String> = scoped.iter().map(|h| h.meta.key.clone()).collect();
                let by_key: HashMap<&str, &SessionHit> =
                    scoped.iter().map(|h| (h.meta.key.as_str(), h)).collect();
                let age = |key: &str| self.index.meta_of(key).map(|m| (now - m.modified).max(0.0));
                let keep = |key: &str| {
                    by_key.contains_key(key) || self.index.meta_of(key).is_some_and(|m| live(&m))
                };
                fused(
                    mode,
                    text_keys,
                    meaning.list("sessions"),
                    session_limit,
                    &age,
                    &keep,
                )
                .into_iter()
                .filter_map(|ranked| {
                    let mut row = match by_key.get(ranked.key.as_str()) {
                        Some(hit) => session_json_for(hit, &query_words, masked),
                        None => {
                            if !want_sessions {
                                return None;
                            }
                            let meta = self.index.meta_of(&ranked.key)?;
                            if !live(&meta) {
                                return None;
                            }
                            let hit = SessionHit {
                                meta,
                                snippet: self.meaning_snippet(ranked.chunk),
                                score: ranked.score,
                            };
                            session_json_for(&hit, &[], masked)
                        }
                    };
                    row["match"] = json!(match_name(ranked.matched));
                    Some(row)
                })
                .collect()
            }
        };
        let sessions = if meaning.is_some() {
            let (rows, over) = within(sessions, MEANING_REPLY_BYTES);
            cut = over;
            rows
        } else {
            sessions
        };
        let mut reply = json!({
            "pages": pages,
            "sessions": sessions,
            "indexing": self.index.is_refreshing(),
            "indexedSince": self.index.indexed_since(now),
            "refreshing": self.index.is_refreshing(),
            "disabled": disabled,
            "dropped": dropped,
        });
        if cut {
            reply["truncated"] = json!(true);
        }
        if let Some(meaning) = &meaning {
            reply["semantic"] = match self.embed.get() {
                Some(embed) => embed.semantic_json(meaning.state),
                None => json!({"state": "off", "coverage": {"chunks": 0, "vectors": 0}}),
            };
            if want_settings {
                let list = fused(
                    Mode::Semantic,
                    Vec::new(),
                    meaning.list(CORPUS_SETTINGS),
                    SETTINGS_CAP,
                    &|_| None,
                    &|_| true,
                );
                reply["settings"] = Value::Array(
                    list.into_iter()
                        .map(|r| json!({"id": r.key, "score": r.score}))
                        .collect(),
                );
            }
        }
        if masked {
            reply["masked"] = json!(true);
            reply["window_days"] = json!(AGENT_WINDOW_DAYS);
        }
        Ok(reply)
    }
}

#[cfg(test)]
#[path = "search_query_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "search_query_scope_tests.rs"]
mod scope_tests;
