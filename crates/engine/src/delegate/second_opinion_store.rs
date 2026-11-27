use super::options::Tool;
use super::second_opinion::{Answer, SecondOpinion, Status, TIMEOUT_SECS};
use crate::store;
use notchling_platform::files::{ensure_private_dir, write_private_no_follow};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const KEEP: usize = 100;

pub const DOC: &str = "app/second-opinions";

pub fn old_file(data: &Path) -> PathBuf {
    data.join("app").join("second-opinions.json")
}

fn valid(text: &str) -> bool {
    serde_json::from_str::<Vec<SecondOpinion>>(text).is_ok()
}

pub trait Job: Send {
    fn terminate(&self);

    fn stop_within(&self, _grace: Duration) {
        self.terminate();
    }
}

pub trait Host: Send + Sync {
    fn launch(
        &self,
        args: Vec<String>,
        folder: &str,
        done: Box<dyn FnOnce(String) + Send>,
    ) -> Result<Box<dyn Job>, String>;
}

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
            let _ = std::fs::remove_file(old_file(data));
        }
    }
}

#[cfg(test)]
#[path = "second_opinion_store_tests.rs"]
mod tests;
