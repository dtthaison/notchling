//! The second opinions asked in Chat, per session (Swift `SecondOpinionStore`): runs
//! `notchling delegate … --json` through a [`Host`], keeps the answers in memory and in the
//! [`DOC`] document of `<data>/notchling.db` so they survive a relaunch (it used to be the app's
//! `<data>/app/second-opinions.json`, which moves in once).
//!
//! The store is pure state and bookkeeping: the host starts the process (the real one is
//! [`super::second_opinion_host::ProcessHost`], tests use a fake) and calls back with the output
//! when it ends. Cancel marks the card cancelled at once and asks the host to stop the job; the
//! tool's partial answer, if any, is added when the command has ended.
//!
//! The data folder is the caller's choice (`None` keeps everything in memory). The Swift app's own
//! `<data>/second-opinions.json` is never read or written: this document is the Rust side's own.

use super::options::Tool;
use super::second_opinion::{Answer, SecondOpinion, Status, TIMEOUT_SECS};
use crate::store;
use notchling_platform::files::{ensure_private_dir, write_private_no_follow};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How many finished opinions the document keeps.
pub const KEEP: usize = 100;

/// The document that holds the opinions.
pub const DOC: &str = "app/second-opinions";

/// The file the opinions were kept in before the database. Not the Swift app's
/// `<data>/second-opinions.json`, a different file in the same format.
pub fn old_file(data: &Path) -> PathBuf {
    data.join("app").join("second-opinions.json")
}

fn valid(text: &str) -> bool {
    serde_json::from_str::<Vec<SecondOpinion>>(text).is_ok()
}

/// A running delegate command.
pub trait Job: Send {
    /// Asks it to stop (SIGTERM); the host kills it a little later if it is still there. Called
    /// with the store locked: it must not call back into the store.
    fn terminate(&self);

    /// Stops it and does not return until it is gone: SIGTERM, then a kill after `grace` if it is
    /// still there. For the app quitting, when nothing is left to kill it later. Where a job can't
    /// wait this asks to stop, like [`Job::terminate`].
    fn stop_within(&self, _grace: Duration) {
        self.terminate();
    }
}

/// Starts the delegate command for an opinion.
pub trait Host: Send + Sync {
    /// Starts `notchling delegate …` with these arguments in `folder`. `done` is called, once,
    /// with the command's standard output when it ends, from any thread.
    fn launch(
        &self,
        args: Vec<String>,
        folder: &str,
        done: Box<dyn FnOnce(String) + Send>,
    ) -> Result<Box<dyn Job>, String>;
}

/// A second opinion to ask.
#[derive(Clone, Debug)]
pub struct Request {
    pub session: String,
    pub anchor: String,
    pub subject: String,
    pub tool: Tool,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub folder: String,
    pub prompt: String,
}

struct Running {
    job: Option<Box<dyn Job>>,
    prompt_file: PathBuf,
    cancelled: bool,
}

struct State {
    opinions: Vec<SecondOpinion>,
    jobs: HashMap<String, Running>,
}

struct Inner {
    data: Option<PathBuf>,
    /// Prompt files are made here.
    temp_dir: PathBuf,
    host: Arc<dyn Host>,
    now: Box<dyn Fn() -> f64 + Send + Sync>,
    make_id: Box<dyn Fn() -> String + Send + Sync>,
    state: Mutex<State>,
}

#[derive(Clone)]
pub struct SecondOpinionStore {
    inner: Arc<Inner>,
}

impl SecondOpinionStore {
    /// Reads the document in `data`'s database if there is one (moving the old file in first): a
    /// run that was going when the app quit has no process any more, so it is shown as failed.
    pub fn new(
        data: Option<PathBuf>,
        temp_dir: PathBuf,
        host: Arc<dyn Host>,
        now: Box<dyn Fn() -> f64 + Send + Sync>,
        make_id: Box<dyn Fn() -> String + Send + Sync>,
    ) -> SecondOpinionStore {
        let mut opinions: Vec<SecondOpinion> = data
            .as_deref()
            .and_then(|dir| {
                let _ = store::import_file(dir, DOC, &old_file(dir), valid);
                store::doc(dir, DOC).ok().flatten()
            })
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        for run in &mut opinions {
            if run.status == Status::Running {
                run.status = Status::Failed;
                run.error = Some("Notchling quit before it finished.".into());
            }
        }
        SecondOpinionStore {
            inner: Arc::new(Inner {
                data,
                temp_dir,
                host,
                now,
                make_id,
                state: Mutex::new(State {
                    opinions,
                    jobs: HashMap::new(),
                }),
            }),
        }
    }

    pub fn opinions(&self) -> Vec<SecondOpinion> {
        self.inner.lock().opinions.clone()
    }

    /// The opinions asked in `session` under any of `anchors`, oldest first.
    pub fn runs(&self, session: &str, anchors: &[String]) -> Vec<SecondOpinion> {
        let mut runs: Vec<SecondOpinion> = self
            .inner
            .lock()
            .opinions
            .iter()
            .filter(|o| o.session == session && anchors.contains(&o.anchor))
            .cloned()
            .collect();
        runs.sort_by(|a, b| a.started.total_cmp(&b.started));
        runs
    }

    /// Records the opinion as running and starts it. A start that fails shows as a failed card.
    pub fn start(&self, request: &Request) -> SecondOpinion {
        let inner = &self.inner;
        let run = SecondOpinion {
            id: (inner.make_id)(),
            session: request.session.clone(),
            anchor: request.anchor.clone(),
            subject: request.subject.clone(),
            tool: request.tool.raw().into(),
            model: request.model.clone(),
            effort: request.effort.clone(),
            folder: request.folder.clone(),
            started: (inner.now)(),
            status: Status::Running,
            reply: String::new(),
            error: None,
            delegated_session: None,
            seconds: 0.0,
            tokens: None,
        };
        inner.lock().opinions.push(run.clone());
        inner.save();
        inner.launch(&run, &request.prompt);
        run
    }

    /// Stops the run now: the card turns to Cancelled at once.
    pub fn cancel(&self, id: &str) {
        {
            let mut state = self.inner.lock();
            let State { opinions, jobs } = &mut *state;
            let Some(running) = jobs.get_mut(id).filter(|r| !r.cancelled) else {
                return;
            };
            running.cancelled = true;
            if let Some(job) = &running.job {
                job.terminate();
            }
            if let Some(run) = opinions.iter_mut().find(|o| o.id == id) {
                run.status = Status::Cancelled;
            }
        }
        self.inner.save();
    }

    pub fn remove(&self, id: &str) {
        self.cancel(id);
        self.inner.lock().opinions.retain(|o| o.id != id);
        self.inner.save();
    }

    /// Stops every running job and returns once they are gone, or killed after `grace` (the app is
    /// quitting, so nothing would be left to do it later). The jobs stop side by side, so this
    /// takes about `grace` however many there are. Their prompt files go too, since the command
    /// that would have removed each when it ended has no process left to report from.
    pub fn stop_all_within(&self, grace: Duration) {
        let (jobs, files): (Vec<Box<dyn Job>>, Vec<PathBuf>) = {
            let mut state = self.inner.lock();
            let mut jobs = Vec::new();
            let mut files = Vec::new();
            for running in state.jobs.values_mut() {
                running.cancelled = true;
                files.push(running.prompt_file.clone());
                jobs.extend(running.job.take());
            }
            (jobs, files)
        };
        std::thread::scope(|scope| {
            for job in jobs {
                scope.spawn(move || job.stop_within(grace));
            }
        });
        for file in files {
            let _ = std::fs::remove_file(file);
        }
    }

    /// Asks every running job to stop (SIGTERM; the host kills a straggler a while later).
    pub fn stop_all(&self) {
        let mut state = self.inner.lock();
        for running in state.jobs.values_mut() {
            running.cancelled = true;
            if let Some(job) = &running.job {
                job.terminate();
            }
        }
    }
}

impl Inner {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn launch(self: &Arc<Self>, run: &SecondOpinion, prompt: &str) {
        let file = self
            .temp_dir
            .join(format!("notchling-second-opinion-{}.txt", run.id));
        let wrote = ensure_private_dir(&self.temp_dir)
            .and_then(|()| write_private_no_follow(&file, prompt.as_bytes()));
        if wrote.is_err() {
            return self.fail(&run.id, "Couldn't write the prompt.");
        }
        let args = SecondOpinion::arguments(
            run.tool_value(),
            run.model.as_deref(),
            run.effort.as_deref(),
            &run.folder,
            &file.to_string_lossy(),
            TIMEOUT_SECS,
        );
        // Registered before the host starts, so a job that ends at once finds its entry.
        self.lock().jobs.insert(
            run.id.clone(),
            Running {
                job: None,
                prompt_file: file.clone(),
                cancelled: false,
            },
        );
        let (inner, id) = (self.clone(), run.id.clone());
        let done: Box<dyn FnOnce(String) + Send> =
            Box::new(move |output| inner.finished(&id, &output));
        match self.host.launch(args, &run.folder, done) {
            Ok(job) => {
                if let Some(running) = self.lock().jobs.get_mut(&run.id) {
                    running.job = Some(job);
                } else {
                    // Already finished.
                    drop(job);
                }
            }
            Err(why) => {
                self.lock().jobs.remove(&run.id);
                let _ = std::fs::remove_file(&file);
                self.fail(&run.id, &why);
            }
        }
    }

    fn finished(&self, id: &str, output: &str) {
        let running = self.lock().jobs.remove(id);
        if let Some(r) = &running {
            let _ = std::fs::remove_file(&r.prompt_file);
        }
        let cancelled = running.as_ref().is_some_and(|r| r.cancelled);
        let answer = Answer::parse(output);
        self.update(id, |run| run.finish(answer.as_ref(), output, cancelled));
    }

    fn fail(&self, id: &str, message: &str) {
        self.update(id, |run| {
            run.status = Status::Failed;
            run.error = Some(message.into());
        });
    }

    fn update(&self, id: &str, change: impl FnOnce(&mut SecondOpinion)) {
        {
            let mut state = self.lock();
            let Some(run) = state.opinions.iter_mut().find(|o| o.id == id) else {
                return;
            };
            change(run);
        }
        self.save();
    }

    /// The document keeps the running ones and the last [`KEEP`] minus those.
    fn save(&self) {
        let Some(data) = &self.data else {
            return;
        };
        let list = {
            let state = self.lock();
            let (running, done): (Vec<_>, Vec<_>) = state
                .opinions
                .iter()
                .cloned()
                .partition(|o| o.status == Status::Running);
            let room = KEEP.saturating_sub(running.len());
            let mut list: Vec<SecondOpinion> = done
                .into_iter()
                .rev()
                .take(room)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .chain(running)
                .collect();
            list.sort_by(|a, b| a.started.total_cmp(&b.started));
            list
        };
        let Ok(text) = serde_json::to_string(&list) else {
            return;
        };
        if store::put_doc(data, DOC, &text).is_ok() {
            // an old file left behind (an unreadable one) would come back as the list
            let _ = std::fs::remove_file(old_file(data));
        }
    }
}

#[cfg(test)]
#[path = "second_opinion_store_tests.rs"]
mod tests;
