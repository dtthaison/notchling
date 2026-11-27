use std::sync::{Arc, Mutex};

use notchling_local_api::ClientError;
use serde_json::{json, Value};

use super::*;
use crate::actions::Transport;
use crate::search::groups::Prefs;
use crate::search::library::Library;
use crate::search::model::SearchModel;
use crate::search::wire::{parse_results, parse_settings_by_meaning};

type Calls = Arc<Mutex<Vec<(String, Value)>>>;

struct Fake(Calls);

impl Transport for Fake {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        self.0.lock().unwrap().push((op.to_owned(), args));
        Ok(json!({"sessions": [], "pages": [], "settings": [{"id": "chatHistory", "score": 0.4}]}))
    }
}

fn library(state: &str) -> (Library, Calls) {
    let calls = Calls::default();
    let meaning = Meaning::from_value(&json!({"state": state, "coverage": []})).unwrap();
    let library =
        Library::over(Box::new(Fake(calls.clone()))).with_embed(EmbedFeed::fixed(meaning));
    (library, calls)
}

fn event(data: Value) -> Event {
    Event {
        event: topics::EMBED_CHANGED.to_owned(),
        data,
    }
}

#[test]
fn semantic_hits_show_related() {
    let results = parse_results(&json!({
        "pages": [{"id": "KVBHDXYvY8f1b1AZG6PbAR", "title": "Plan", "match": "semantic"}],
        "sessions": [
            {"key": "claude:1", "title": "Both", "match": "both"},
            {"key": "claude:2", "title": "Meaning", "match": "semantic"},
            {"key": "claude:3", "title": "Words", "match": "text"},
            {"key": "claude:4", "title": "Old reply"}
        ]
    }));
    assert!(results.pages[0].related);
    let related: Vec<bool> = results.sessions.iter().map(|s| s.related).collect();
    assert_eq!(related, [false, true, false, false]);
    assert_eq!(notchling_locale::search::related(), "Related");
}

#[test]
fn progress_text_counts_passages() {
    let corpora = json!([
        {"corpus": "sessions", "docs": 40, "chunks": 15000, "embedded": {"granite-97m-r2": 1240, "static-mrl": 15000}}
    ]);
    let meaning = Meaning::from_value(&json!({"state": "indexing", "coverage": corpora})).unwrap();
    assert_eq!((meaning.chunks, meaning.embedded), (15000, 1240));
    assert_eq!(
        meaning.progress_text().as_deref(),
        Some("Indexing 1,240 of 15,000 passages")
    );
    let mut model = SearchModel::new();
    assert_eq!(model.footer_text(0), "Pages, Vault and Sessions");
    model.meaning = meaning;
    assert_eq!(model.footer_text(0), "Indexing 1,240 of 15,000 passages");
    model.meaning.state = EmbedState::Ready;
    assert_eq!(model.footer_text(0), "Pages, Vault and Sessions");
    let totals = Meaning::from_value(&json!({
        "state": "indexing", "coverage": {"chunks": 10, "vectors": 4}
    }))
    .unwrap();
    assert_eq!((totals.chunks, totals.embedded), (10, 4));
}

#[test]
fn the_feed_follows_embed_changed() {
    let feed = EmbedFeed::fixed(Meaning::default());
    assert!(!feed.meaning().asks());
    feed.shared.apply(&event(
        json!({"state": "ready", "coverage": [], "fetch": null}),
    ));
    assert_eq!(feed.meaning().state, EmbedState::Ready);
    feed.shared.apply(&event(json!({"state": "mystery"})));
    assert_eq!(feed.meaning().state, EmbedState::Ready);
    feed.shared.lost();
    assert_eq!(feed.meaning().state, EmbedState::Off);
    assert!(!feed.following());
}

#[test]
fn nothing_is_asked_while_not_ready() {
    for state in ["off", "no_model", "warming", "paused"] {
        let (library, calls) = library(state);
        library.ask("bridge", 30, &Prefs::default()).unwrap();
        assert!(library.settings_by_meaning("history", 8).is_empty());
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "{state}");
        assert!(calls[0].1.get("mode").is_none(), "{state}");
    }
    for state in ["ready", "indexing"] {
        let (library, calls) = library(state);
        library.ask("bridge", 30, &Prefs::default()).unwrap();
        let found = library.settings_by_meaning("history", 8);
        assert_eq!(found, vec![("chatHistory".to_owned(), 0.4)], "{state}");
        let calls = calls.lock().unwrap();
        assert_eq!(calls[0].1["mode"], "hybrid", "{state}");
        assert_eq!(calls[1].1["mode"], "semantic", "{state}");
        assert_eq!(calls[1].1["corpora"], json!(["settings"]), "{state}");
    }
}

#[test]
fn the_settings_text_goes_to_the_daemon_as_docs() {
    let (library, calls) = library("off");
    library
        .send_settings(&[("showPet".into(), "Show pet".into())])
        .unwrap();
    let calls = calls.lock().unwrap();
    assert_eq!(calls[0].0, "embed.settings");
    assert_eq!(
        calls[0].1,
        json!({"docs": [{"id": "showPet", "text": "Show pet"}]})
    );
    assert_eq!(
        parse_settings_by_meaning(&json!({"settings": [{"id": "a"}, {"id": "b", "score": 1.0}]})),
        vec![("b".to_owned(), 1.0)]
    );
}
