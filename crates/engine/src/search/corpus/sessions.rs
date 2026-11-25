use super::{Corpus, Doc, DocStamp, Tier};
use crate::search::chunk::{self, hash, session_chunks, turns};
use crate::search::index::read::read_with_lines;
use crate::search::index::scan::{self, Candidate};
use crate::search::index::{stamp_account, AccountOf, IndexRoots};
use crate::search::SessionMeta;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct SessionsCorpus {
    roots: IndexRoots,
    accounts: Option<AccountOf>,
    found: Mutex<HashMap<String, Candidate>>,
}

impl SessionsCorpus {
    pub fn new(roots: IndexRoots) -> SessionsCorpus {
        SessionsCorpus {
            roots,
            accounts: None,
            found: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_accounts(mut self, accounts: AccountOf) -> SessionsCorpus {
        self.accounts = Some(accounts);
        self
    }
}

impl Corpus for SessionsCorpus {
    fn id(&self) -> &'static str {
        "sessions"
    }

    fn tier(&self) -> Tier {
        Tier::Agent
    }

    fn scan(&self, window: Option<f64>, now: f64) -> Vec<DocStamp> {
        let mut candidates = scan::claude(&self.roots.claude, now, window);
        if let Some(sessions) = &self.roots.codex_sessions {
            candidates.extend(scan::codex(sessions, now, window));
        }
        let mut stamps: Vec<DocStamp> = candidates
            .iter()
            .map(|c| DocStamp {
                key: c.meta.key.clone(),
                stamp: format!(
                    "{}:{}:{}",
                    c.meta.modified,
                    if now - c.meta.modified >= chunk::QUIET_SECS {
                        "q"
                    } else {
                        "o"
                    },
                    c.meta.size
                ),
                at: c.meta.modified,
            })
            .collect();
        stamps.extend(
            self.roots
                .titled
                .iter()
                .filter(|row| window.is_none_or(|w| now - row.modified < w))
                .map(|row| DocStamp {
                    key: row.meta().key,
                    stamp: format!(
                        "{}:{}",
                        row.modified,
                        hash(&format!("{}\n{}", row.title, row.folder))
                            .iter()
                            .map(|b| format!("{b:02x}"))
                            .collect::<String>()
                    ),
                    at: row.modified,
                }),
        );
        *self.found.lock().unwrap_or_else(|e| e.into_inner()) = candidates
            .into_iter()
            .map(|c| (c.meta.key.clone(), c))
            .collect();
        stamps
    }

    fn read(&self, doc: &DocStamp, now: f64) -> Option<Doc> {
        if let Some(row) = self.roots.titled.iter().find(|r| r.meta().key == doc.key) {
            let mut meta = row.meta();
            stamp_account(self.accounts.as_ref(), &mut meta);
            return Some(Doc {
                key: meta.key.clone(),
                title: meta.title.clone(),
                at: meta.modified,
                facets: facets_of(&meta),
                chunks: session_chunks(&meta.title, &meta.folder, &[]),
            });
        }
        let candidate = self
            .found
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&doc.key)
            .cloned()?;
        let (mut session, lines) = read_with_lines(&candidate.meta, candidate.tool)?;
        stamp_account(self.accounts.as_ref(), &mut session.meta);
        let quiet = now - candidate.meta.modified >= chunk::QUIET_SECS;
        let turns = turns(lines, quiet);
        let facets = facets_of(&session.meta);
        Some(Doc {
            key: session.meta.key.clone(),
            title: session.meta.title.clone(),
            at: candidate.meta.modified,
            facets,
            chunks: session_chunks(&session.meta.title, &session.meta.folder, &turns),
        })
    }
}

fn facets_of(meta: &SessionMeta) -> Vec<(String, String)> {
    meta.facets()
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect()
}
