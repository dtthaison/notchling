use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use notchling_engine::pages::clock::local_offset_secs;
use notchling_engine::search::facets::FacetValue;
use notchling_engine::search::query::parse::{parse, Parsed};
use notchling_local_api::{ClientError, SOCKET_NAME};
use serde_json::Value;

use super::chips::{self, ChipView, Known};
use super::embed_feed::{EmbedFeed, Meaning};
use super::groups::Prefs;
use super::wire::{
    facets_whole, parse_facets, parse_pages, parse_results, parse_settings_by_meaning, Call,
    PageList, Results,
};
use crate::acting::{self, Acting};
use crate::actions::{Socket, Transport};
use crate::chat::data::DataRoot;

pub const DETACHED_MESSAGE: &str =
    "Notchling's daemon isn't attached, so there is nothing to search";

pub const KNOWN_FOR: Duration = Duration::from_secs(30);
const FACET_LIMIT: usize = 100;

pub struct Library {
    transport: Option<Box<dyn Transport>>,
    fixed: Mutex<Results>,
    data_dir: Option<PathBuf>,
    known: Mutex<Held>,
    embed: EmbedFeed,
}

#[derive(Default)]
struct Held {
    known: Known,
    at: Option<Instant>,
    generation: u64,
}

struct Fetched {
    values: Vec<FacetValue>,
    whole: bool,
    capped: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Answer {
    pub results: Results,
    pub rest: String,
    pub vault: bool,
    pub chips: Vec<ChipView>,
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

fn merge(into: &mut Vec<FacetValue>, more: Vec<FacetValue>) {
    for value in more {
        match into.iter_mut().find(|v| v.value == value.value) {
            Some(have) => have.count += value.count,
            None => into.push(value),
        }
    }
    into.sort_by_key(|v| std::cmp::Reverse(v.count));
}

fn message(e: &ClientError) -> String {
    if acting::after_error(e) == Some(Acting::Detached) {
        return DETACHED_MESSAGE.to_owned();
    }
    match e {
        ClientError::Api(api) => api.message.clone(),
        other => other.to_string(),
    }
}

impl Library {
    pub fn over(transport: Box<dyn Transport>) -> Library {
        Library {
            transport: Some(transport),
            fixed: Mutex::new(Results::default()),
            data_dir: None,
            known: Mutex::default(),
            embed: EmbedFeed::none(),
        }
    }

    pub fn fixed(results: Results) -> Library {
        Library {
            transport: None,
            fixed: Mutex::new(results),
            data_dir: None,
            known: Mutex::default(),
            embed: EmbedFeed::none(),
        }
    }

    pub fn for_run(root: Option<&DataRoot>) -> Library {
        match root.filter(|r| r.fixture.is_none()) {
            Some(root) => Library {
                data_dir: Some(root.data_dir.clone()),
                embed: EmbedFeed::for_run(Some(root)),
                ..Library::over(Box::new(Socket {
                    socket: root.data_dir.join(SOCKET_NAME),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                }))
            },
            None => Library::fixed(Results::default()),
        }
    }

    pub fn with_embed(mut self, embed: EmbedFeed) -> Library {
        self.embed = embed;
        self
    }

    pub fn is_live(&self) -> bool {
        self.transport.is_some()
    }

    pub fn meaning(&self) -> Meaning {
        self.embed.meaning()
    }

    fn query_call(&self, call: Call) -> Call {
        if self.embed.meaning().asks() {
            call.hybrid()
        } else {
            call
        }
    }

    fn fixed_results(&self) -> Results {
        self.fixed.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn call(&self, call: &Call) -> Result<serde_json::Value, String> {
        match &self.transport {
            Some(transport) => transport
                .call(call.op, call.args.clone())
                .map_err(|e| message(&e)),
            None => Err(DETACHED_MESSAGE.to_owned()),
        }
    }

    pub fn prefs(&self) -> Prefs {
        self.data_dir
            .as_deref()
            .map(Prefs::read)
            .unwrap_or_default()
    }

    pub fn search(&self, text: &str, limit: usize, prefs: &Prefs) -> Result<Results, String> {
        self.ask(text, limit, prefs).map(|answer| answer.results)
    }

    pub fn ask(&self, text: &str, limit: usize, prefs: &Prefs) -> Result<Answer, String> {
        if self.transport.is_none() {
            return Ok(Answer {
                results: self.fixed_results(),
                rest: text.to_owned(),
                vault: true,
                chips: Vec::new(),
            });
        }
        let parsed = parse(text);
        if parsed.chips.is_empty() {
            let results = self
                .call(&self.query_call(Call::query(text, limit, &prefs.corpora())))
                .map(|v| parse_results(&v))?;
            return Ok(Answer {
                results,
                rest: text.to_owned(),
                vault: true,
                chips: Vec::new(),
            });
        }
        let known = self.known_for(&parsed)?;
        let now = now_secs();
        let filter = chips::sent_filter(&parsed, &known, now, local_offset_secs(now));
        let corpora = chips::corpora(&parsed, prefs.corpora());
        let results = if corpora.is_empty() {
            Results::default()
        } else {
            let call = Call::filtered(&parsed.rest, limit, &corpora, &filter);
            self.call(&self.query_call(call))
                .map(|v| parse_results(&v))?
        };
        Ok(Answer {
            results,
            vault: chips::vault_allowed(&parsed, &filter),
            chips: chips::chips(&parsed, &known),
            rest: parsed.rest,
        })
    }

    fn fetch(&self, name: &'static str, prefix: &str) -> Result<Fetched, String> {
        let mut values = Vec::new();
        let mut whole = true;
        let mut capped = false;
        for corpus in chips::corpora_for(name) {
            let found = self.call(&Call::facets(corpus, name, prefix, FACET_LIMIT))?;
            whole &= facets_whole(&found);
            let more = parse_facets(&found);
            capped |= more.len() >= FACET_LIMIT;
            merge(&mut values, more);
        }
        Ok(Fetched {
            values,
            whole,
            capped,
        })
    }

    fn held(&self) -> (Known, u64) {
        let mut held = self.known.lock().unwrap_or_else(|e| e.into_inner());
        if held.at.is_some_and(|at| at.elapsed() > KNOWN_FOR) {
            *held = Held {
                generation: held.generation + 1,
                ..Held::default()
            };
        }
        (held.known.clone(), held.generation)
    }

    fn load(&self, names: &[&'static str]) -> Result<Known, String> {
        let (known, generation) = self.held();
        self.load_from(known, generation, names)
    }

    fn load_from(
        &self,
        mut known: Known,
        generation: u64,
        names: &[&'static str],
    ) -> Result<Known, String> {
        let mut kept: Vec<(&'static str, Vec<FacetValue>, bool)> = Vec::new();
        for name in names {
            if !known.has(name) {
                let fetched = self.fetch(name, "")?;
                if fetched.whole {
                    kept.push((name, fetched.values.clone(), fetched.capped));
                }
                known.set(name, fetched.values);
                if fetched.capped {
                    known.mark_capped(name);
                }
            }
        }
        if !kept.is_empty() {
            let mut held = self.known.lock().unwrap_or_else(|e| e.into_inner());
            if held.generation == generation {
                for (name, values, capped) in kept {
                    if !held.known.has(name) {
                        held.known.set(name, values);
                        if capped {
                            held.known.mark_capped(name);
                        }
                    }
                }
                held.at.get_or_insert_with(Instant::now);
            }
        }
        Ok(known)
    }

    fn known_for(&self, parsed: &Parsed) -> Result<Known, String> {
        let (held, generation) = self.held();
        let mut known =
            self.load_from(held, generation, &chips::wanted(parsed, &Known::default()))?;
        let mut found: Vec<(&'static str, String, Vec<FacetValue>)> = Vec::new();
        for (name, prefix) in chips::unresolved(parsed, &known) {
            let fetched = self.fetch(name, &prefix)?;
            known.add(name, fetched.values.clone());
            if fetched.whole {
                found.push((name, prefix, fetched.values));
            }
        }
        if !found.is_empty() {
            let mut held = self.known.lock().unwrap_or_else(|e| e.into_inner());
            if held.generation == generation {
                for (name, prefix, values) in found {
                    if held.known.has(name) {
                        held.known.add(name, values);
                        held.known.mark_asked(name, &prefix);
                    }
                }
            }
        }
        Ok(known)
    }

    pub fn complete(&self, text: &str) -> Option<String> {
        self.transport.as_ref()?;
        let (name, partial) = chips::typing_partial(text)?;
        let mut known = self.load(&[name]).ok()?;
        if !partial.is_empty() {
            if let Ok(fetched) = self.fetch(name, partial) {
                known.add(name, fetched.values);
            }
        }
        chips::complete(text, &known)
    }

    pub fn forget(&self) {
        let mut held = self.known.lock().unwrap_or_else(|e| e.into_inner());
        *held = Held {
            generation: held.generation + 1,
            ..Held::default()
        };
    }

    pub fn refresh(&self) -> Result<(), String> {
        if self.transport.is_none() {
            return Ok(());
        }
        self.forget();
        self.call(&Call::embed_refresh()).map(|_| ())
    }

    pub fn embed_status(&self) -> Value {
        self.embed.status()
    }

    pub fn embed_model_op(&self, op: &'static str, model: &str) -> Result<(), String> {
        if self.transport.is_none() {
            return Ok(());
        }
        self.call(&Call::embed_model(op, model)).map(|_| ())
    }

    pub fn settings_by_meaning(&self, text: &str, limit: usize) -> Vec<(String, f64)> {
        if self.transport.is_none() || !self.embed.meaning().asks() || text.trim().is_empty() {
            return Vec::new();
        }
        self.call(&Call::settings_by_meaning(text, limit))
            .map(|v| parse_settings_by_meaning(&v))
            .unwrap_or_default()
    }

    pub fn send_settings(&self, docs: &[(String, String)]) -> Result<(), String> {
        if self.transport.is_none() {
            return Ok(());
        }
        self.call(&Call::embed_settings(docs)).map(|_| ())
    }

    pub fn pages(&self) -> Result<PageList, String> {
        if self.transport.is_none() {
            return Ok(PageList {
                pages: self.fixed_results().pages,
                truncated: false,
            });
        }
        self.call(&Call::pages_list()).map(|v| parse_pages(&v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_local_api::{ApiError, ErrorCode};
    use serde_json::{json, Value};
    use std::sync::Arc;

    type Calls = Arc<Mutex<Vec<(String, Value)>>>;
    type Script = Box<dyn Fn(&str, &Value) -> Result<Value, ClientError> + Send + Sync>;

    struct Fake {
        calls: Calls,
        answer: Script,
    }

    impl Transport for Fake {
        fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
            self.calls
                .lock()
                .unwrap()
                .push((op.to_owned(), args.clone()));
            (self.answer)(op, &args)
        }
    }

    fn fake_args(
        answer: impl Fn(&str, &Value) -> Result<Value, ClientError> + Send + Sync + 'static,
    ) -> (Library, Calls) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let library = Library::over(Box::new(Fake {
            calls: calls.clone(),
            answer: Box::new(answer),
        }));
        (library, calls)
    }

    fn fake(
        answer: impl Fn(&str) -> Result<Value, ClientError> + Send + Sync + 'static,
    ) -> (Library, Calls) {
        fake_args(move |op, _| answer(op))
    }

    #[test]
    fn a_search_sends_the_text_and_limit_and_reads_both_groups() {
        let (library, calls) = fake(|op| match op {
            "search.query" => Ok(json!({
                "pages": [{"id": "abcdefghijklmnopqrst", "folder": "plan-1", "title": "Plan"}],
                "sessions": [{"key": "codex:1", "title": "Fix"}],
                "indexing": true
            })),
            _ => Ok(json!({})),
        });
        assert!(library.is_live());
        let results = library.search("plan", 30, &Prefs::default()).unwrap();
        assert_eq!(results.pages.len(), 1);
        assert_eq!(results.sessions.len(), 1);
        assert!(results.indexing);
        assert_eq!(
            calls.lock().unwrap().clone(),
            vec![(
                "search.query".to_owned(),
                json!({"text": "plan", "limit": 30, "corpora": ["sessions", "pages"]})
            )]
        );
    }

    fn facet_answer(op: &str) -> Result<Value, ClientError> {
        match op {
            "search.facets" => Ok(json!({"values": [
                {"value": "codex", "label": "codex", "count": 2},
                {"value": "claude", "label": "claude", "count": 5}
            ]})),
            "search.query" => Ok(json!({"sessions": [{"key": "codex:1", "title": "Fix"}]})),
            _ => Ok(json!({})),
        }
    }

    #[test]
    fn a_search_with_chips_loads_the_values_once_and_sends_the_filter() {
        let (library, calls) = fake(facet_answer);
        let answer = library
            .ask("fix tool:codex", 30, &Prefs::default())
            .unwrap();
        assert_eq!(answer.rest, "fix");
        assert!(!answer.vault);
        assert_eq!(answer.chips.len(), 1);
        assert!(answer.chips[0].known);
        library
            .ask("fix tool:claude", 30, &Prefs::default())
            .unwrap();
        let calls = calls.lock().unwrap().clone();
        let ops: Vec<&str> = calls.iter().map(|c| c.0.as_str()).collect();
        assert_eq!(ops, ["search.facets", "search.query", "search.query"]);
        assert_eq!(
            calls[0].1,
            json!({"corpus": "sessions", "name": "tool", "limit": 100})
        );
        assert_eq!(
            calls[1].1,
            json!({"text": "fix", "limit": 30, "corpora": ["sessions", "pages"],
                   "filter": {"tool": ["codex"]}})
        );
    }

    #[test]
    fn the_values_are_loaded_again_after_they_expire() {
        let (library, calls) = fake(facet_answer);
        library.ask("tool:codex", 30, &Prefs::default()).unwrap();
        library.known.lock().unwrap().at = Instant::now().checked_sub(KNOWN_FOR * 2);
        library.ask("tool:codex", 30, &Prefs::default()).unwrap();
        let facets = calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.0 == "search.facets")
            .count();
        assert_eq!(facets, 2);
    }

    #[test]
    fn a_category_chip_for_a_corpus_that_is_off_asks_nothing() {
        let (library, calls) = fake(facet_answer);
        let prefs = Prefs::default();
        let answer = library.ask("v: token", 30, &prefs).unwrap();
        assert!(answer.vault);
        assert!(answer.results.sessions.is_empty());
        assert!(calls.lock().unwrap().is_empty());
    }

    #[test]
    fn text_with_no_chips_is_sent_untouched() {
        let (library, calls) = fake(facet_answer);
        library
            .search("  plan  the thing ", 30, &Prefs::default())
            .unwrap();
        assert_eq!(
            calls.lock().unwrap().clone(),
            vec![(
                "search.query".to_owned(),
                json!({"text": "  plan  the thing ", "limit": 30, "corpora": ["sessions", "pages"]})
            )]
        );
    }

    #[test]
    fn tab_loads_the_values_and_completes() {
        let (library, _) = fake(facet_answer);
        assert_eq!(
            library.complete("fix tool:co").as_deref(),
            Some("fix tool:codex ")
        );
        assert_eq!(library.complete("fix tool:x"), None);
        assert_eq!(library.complete("fix it"), None);
        assert_eq!(Library::fixed(Results::default()).complete("tool:co"), None);
    }

    fn counted(calls: &Calls, name: &str) -> usize {
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.0 == "search.facets" && c.1["name"] == name)
            .count()
    }

    fn deep_facets(op: &str, args: &Value) -> Result<Value, ClientError> {
        match op {
            "search.facets" => {
                let prefix = args.get("prefix").and_then(Value::as_str).unwrap_or("");
                let all = [
                    ("/w/busy", "busy", 9),
                    ("/w/zebra-crossing", "zebra-crossing", 1),
                ];
                let cap = if prefix.is_empty() { 1 } else { 100 };
                let values: Vec<Value> = all
                    .iter()
                    .filter(|(_, label, _)| label.starts_with(prefix))
                    .take(cap)
                    .map(|(value, label, count)| json!({"value": value, "label": label, "count": count}))
                    .collect();
                Ok(json!({ "values": values }))
            }
            "search.query" => Ok(json!({"sessions": [{"key": "codex:1", "title": "Fix"}]})),
            _ => Ok(json!({})),
        }
    }

    #[test]
    fn a_value_past_the_first_hundred_is_still_found_by_what_was_typed() {
        let (library, calls) = fake_args(deep_facets);
        let answer = library
            .ask("fix project:zebra-crossing", 30, &Prefs::default())
            .unwrap();
        assert!(answer.chips[0].known);
        assert!(!answer.vault);
        let calls = calls.lock().unwrap().clone();
        let query = calls.iter().find(|c| c.0 == "search.query").unwrap();
        assert_eq!(query.1["filter"], json!({"project": ["/w/zebra-crossing"]}));
        assert!(calls
            .iter()
            .any(|c| c.0 == "search.facets" && c.1["prefix"] == "zebra-crossing"));
    }

    fn crowded_facets(op: &str, args: &Value) -> Result<Value, ClientError> {
        match op {
            "search.facets" => {
                let prefix = args["prefix"].as_str().unwrap_or("");
                let mut all: Vec<(String, String, u64)> = Vec::new();
                if args["corpus"] == "sessions" {
                    for n in 0..FACET_LIMIT {
                        all.push((format!("/s/p{n:03}"), format!("p{n:03}"), 1000 - n as u64));
                    }
                    all.push(("/s/notchling".into(), "notchling".into(), 1));
                } else {
                    all.push(("/w/notchling".into(), "notchling".into(), 5));
                }
                let values: Vec<Value> = all
                    .into_iter()
                    .filter(|(_, label, _)| label.starts_with(prefix))
                    .take(FACET_LIMIT)
                    .map(|(value, label, count)| json!({"value": value, "label": label, "count": count}))
                    .collect();
                Ok(json!({ "values": values }))
            }
            "search.query" => Ok(json!({"sessions": [{"key": "codex:1", "title": "Fix"}]})),
            _ => Ok(json!({})),
        }
    }

    #[test]
    fn a_label_shared_past_the_cap_still_reaches_the_filter() {
        let (library, calls) = fake_args(crowded_facets);
        let answer = library
            .ask("fix project:notchling", 30, &Prefs::default())
            .unwrap();
        assert!(answer.chips[0].known);
        let sent = |calls: &Calls| {
            let calls = calls.lock().unwrap();
            let query = calls.iter().rev().find(|c| c.0 == "search.query").unwrap();
            let mut project: Vec<String> = query.1["filter"]["project"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect();
            project.sort();
            project
        };
        assert_eq!(sent(&calls), ["/s/notchling", "/w/notchling"]);
        let typed = |calls: &Calls| {
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.0 == "search.facets" && c.1["prefix"] == "notchling")
                .count()
        };
        assert_eq!(typed(&calls), 2);
        library
            .ask("fix project:notchling", 30, &Prefs::default())
            .unwrap();
        assert_eq!(sent(&calls), ["/s/notchling", "/w/notchling"]);
        assert_eq!(typed(&calls), 2);
    }

    #[test]
    fn a_forget_during_a_load_leaves_nothing_stale() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::OnceLock;
        let slot: Arc<OnceLock<Arc<Library>>> = Arc::new(OnceLock::new());
        let inner = slot.clone();
        let once = AtomicBool::new(true);
        let (library, calls) = fake_args(move |op, _| {
            if op == "search.facets" && once.swap(false, Ordering::SeqCst) {
                if let Some(library) = inner.get() {
                    library.forget();
                }
            }
            facet_answer(op)
        });
        let library = Arc::new(library);
        slot.set(library.clone()).ok();
        library.ask("tool:codex", 30, &Prefs::default()).unwrap();
        library.ask("tool:codex", 30, &Prefs::default()).unwrap();
        assert_eq!(counted(&calls, "tool"), 2);
    }

    #[test]
    fn a_forget_during_a_typed_lookup_leaves_nothing_stale() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::OnceLock;
        let slot: Arc<OnceLock<Arc<Library>>> = Arc::new(OnceLock::new());
        let inner = slot.clone();
        let once = AtomicBool::new(true);
        let (library, _) = fake_args(move |op, args| {
            let typed = args["prefix"].as_str().is_some_and(|p| !p.is_empty());
            if op == "search.facets" && typed && once.swap(false, Ordering::SeqCst) {
                if let Some(library) = inner.get() {
                    library.forget();
                    library.load(&["project"]).unwrap();
                }
            }
            deep_facets(op, args)
        });
        let library = Arc::new(library);
        slot.set(library.clone()).ok();
        library
            .ask("fix project:zebra-crossing", 30, &Prefs::default())
            .unwrap();
        let held = library.known.lock().unwrap();
        let values: Vec<&str> = held
            .known
            .values("project")
            .iter()
            .map(|v| v.value.as_str())
            .collect();
        assert_eq!(values, ["/w/busy"]);
    }

    #[test]
    fn tab_asks_the_daemon_for_what_was_typed() {
        let (library, _) = fake_args(deep_facets);
        assert_eq!(
            library.complete("project:zebra").as_deref(),
            Some("project:zebra-crossing ")
        );
    }

    #[test]
    fn values_for_a_corpus_that_is_off_are_not_kept() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let off = Arc::new(AtomicBool::new(true));
        let seen = off.clone();
        let (library, _) = fake_args(move |op, args| match op {
            "search.facets" if seen.load(Ordering::SeqCst) => {
                Ok(json!({"values": [], "disabled": [args["corpus"]]}))
            }
            _ => facet_answer(op),
        });
        let answer = library.ask("tool:codex", 30, &Prefs::default()).unwrap();
        assert!(!answer.chips[0].known);
        off.store(false, Ordering::SeqCst);
        let answer = library.ask("tool:codex", 30, &Prefs::default()).unwrap();
        assert!(answer.chips[0].known);
    }

    #[test]
    fn a_pass_asked_for_forgets_the_values() {
        let (library, calls) = fake(facet_answer);
        library.ask("tool:codex", 30, &Prefs::default()).unwrap();
        library.refresh().unwrap();
        library.ask("tool:codex", 30, &Prefs::default()).unwrap();
        assert_eq!(counted(&calls, "tool"), 2);
        library.forget();
        library.ask("tool:codex", 30, &Prefs::default()).unwrap();
        assert_eq!(counted(&calls, "tool"), 3);
    }

    #[test]
    fn two_loads_at_once_keep_each_others_values() {
        use std::sync::OnceLock;
        let slot: Arc<OnceLock<Arc<Library>>> = Arc::new(OnceLock::new());
        let inner = slot.clone();
        let (library, calls) = fake_args(move |op, args| {
            if op == "search.facets" && args["name"] == "account" {
                if let Some(library) = inner.get() {
                    library.load(&["owner"]).unwrap();
                }
            }
            Ok(json!({"values": [{"value": "v", "label": "v", "count": 1}]}))
        });
        let library = Arc::new(library);
        slot.set(library.clone()).ok();
        library.load(&["account"]).unwrap();
        library.load(&["owner"]).unwrap();
        assert_eq!(counted(&calls, "owner"), 1);
    }

    #[test]
    fn a_refresh_asks_the_daemon_to_run_a_pass() {
        let (library, calls) = fake(|_| Ok(json!({})));
        library.refresh().unwrap();
        assert_eq!(
            calls.lock().unwrap().clone(),
            vec![("embed.refresh".to_owned(), json!({}))]
        );
        let (library, _) = fake(|_| Err(ClientError::Closed));
        assert_eq!(library.refresh().unwrap_err(), DETACHED_MESSAGE);
        assert_eq!(Library::fixed(Results::default()).refresh(), Ok(()));
    }

    #[test]
    fn the_pages_list_is_read_from_pages_list() {
        let (library, calls) = fake(|op| match op {
            "pages.list" => {
                Ok(json!({"pages": [{"folder": "a-1"}, {"folder": "b-2"}], "truncated": true}))
            }
            _ => Ok(json!({})),
        });
        let list = library.pages().unwrap();
        assert_eq!(list.pages.len(), 2);
        assert!(list.truncated);
        assert_eq!(calls.lock().unwrap()[0].0, "pages.list");
    }

    #[test]
    fn a_lost_daemon_says_detached_and_a_refusal_says_why() {
        let (library, _) = fake(|_| Err(ClientError::Closed));
        assert_eq!(
            library.search("x", 30, &Prefs::default()).unwrap_err(),
            DETACHED_MESSAGE
        );
        assert_eq!(library.pages().unwrap_err(), DETACHED_MESSAGE);
        let (library, _) = fake(|_| {
            Err(ClientError::Api(ApiError::new(
                ErrorCode::Forbidden,
                "search is for the app only",
            )))
        });
        assert_eq!(
            library.search("x", 30, &Prefs::default()).unwrap_err(),
            "search is for the app only"
        );
    }

    #[test]
    fn a_fixed_library_answers_what_it_was_given_and_never_calls() {
        let results = parse_results(&json!({
            "pages": [{"folder": "plan-1", "title": "Plan"}],
            "sessions": [{"key": "claude:1", "title": "A"}]
        }));
        let library = Library::fixed(results.clone());
        assert!(!library.is_live());
        assert_eq!(
            library.search("anything", 6, &Prefs::default()).unwrap(),
            results
        );
        let list = library.pages().unwrap();
        assert_eq!(list.pages, results.pages);
        assert!(!list.truncated);
        let none = Library::for_run(None);
        assert!(!none.is_live());
        assert_eq!(
            none.search("", 6, &Prefs::default()).unwrap(),
            Results::default()
        );
        let fixture = DataRoot::for_render(None);
        assert!(!Library::for_run(Some(&fixture)).is_live());
    }
}
