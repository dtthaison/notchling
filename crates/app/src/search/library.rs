use std::sync::Mutex;

use notchling_local_api::{ClientError, SOCKET_NAME};

use super::wire::{parse_pages, parse_results, Call, PageList, Results};
use crate::acting::{self, Acting};
use crate::actions::{Socket, Transport};
use crate::chat::data::DataRoot;

pub const DETACHED_MESSAGE: &str =
    "Notchling's daemon isn't attached, so there is nothing to search";

pub struct Library {
    transport: Option<Box<dyn Transport>>,
    fixed: Mutex<Results>,
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
        }
    }

    pub fn fixed(results: Results) -> Library {
        Library {
            transport: None,
            fixed: Mutex::new(results),
        }
    }

    pub fn for_run(root: Option<&DataRoot>) -> Library {
        match root.filter(|r| r.fixture.is_none()) {
            Some(root) => Library::over(Box::new(Socket {
                socket: root.data_dir.join(SOCKET_NAME),
                version: env!("CARGO_PKG_VERSION").to_string(),
            })),
            None => Library::fixed(Results::default()),
        }
    }

    pub fn is_live(&self) -> bool {
        self.transport.is_some()
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

    pub fn search(&self, text: &str, limit: usize) -> Result<Results, String> {
        if self.transport.is_none() {
            return Ok(self.fixed_results());
        }
        self.call(&Call::query(text, limit))
            .map(|v| parse_results(&v))
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
    type Script = Box<dyn Fn(&str) -> Result<Value, ClientError> + Send + Sync>;

    struct Fake {
        calls: Calls,
        answer: Script,
    }

    impl Transport for Fake {
        fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
            self.calls.lock().unwrap().push((op.to_owned(), args));
            (self.answer)(op)
        }
    }

    fn fake(
        answer: impl Fn(&str) -> Result<Value, ClientError> + Send + Sync + 'static,
    ) -> (Library, Calls) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let library = Library::over(Box::new(Fake {
            calls: calls.clone(),
            answer: Box::new(answer),
        }));
        (library, calls)
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
        let results = library.search("plan", 30).unwrap();
        assert_eq!(results.pages.len(), 1);
        assert_eq!(results.sessions.len(), 1);
        assert!(results.indexing);
        assert_eq!(
            calls.lock().unwrap().clone(),
            vec![(
                "search.query".to_owned(),
                json!({"text": "plan", "limit": 30})
            )]
        );
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
        assert_eq!(library.search("x", 30).unwrap_err(), DETACHED_MESSAGE);
        assert_eq!(library.pages().unwrap_err(), DETACHED_MESSAGE);
        let (library, _) = fake(|_| {
            Err(ClientError::Api(ApiError::new(
                ErrorCode::Forbidden,
                "search is for the app only",
            )))
        });
        assert_eq!(
            library.search("x", 30).unwrap_err(),
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
        assert_eq!(library.search("anything", 6).unwrap(), results);
        let list = library.pages().unwrap();
        assert_eq!(list.pages, results.pages);
        assert!(!list.truncated);
        let none = Library::for_run(None);
        assert!(!none.is_live());
        assert_eq!(none.search("", 6).unwrap(), Results::default());
        let fixture = DataRoot::for_render(None);
        assert!(!Library::for_run(Some(&fixture)).is_live());
    }
}
