use super::action::{Env, Options, Plan};
use super::apply::{apply_only, Applied, Only};
use super::check::{check, resolve, Choice, Resolved, Row};
use super::keys::Probe;
use super::manifest::Manifest;
use super::model::{Profile, APP_KEY, KEY_PICKS, KEY_PRESET, KEY_TOOLS};
use super::plan::plan;
use super::resolve::Available;
use super::undo::{current_hash, undo_only, UndoResult};
use crate::catalog::{Catalog, LiveModel, Tool};
use crate::install::ToolRoots;
use crate::settings::Settings;
use crate::tools::codex::models as codex_models;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct UndoPart {
    pub id: String,
    pub path: PathBuf,
    pub removes: bool,
    pub now: Option<String>,
}

pub type Live = Arc<dyn Fn() -> Vec<LiveModel> + Send + Sync>;

pub struct Service {
    pub roots: ToolRoots,
    pub data_dir: PathBuf,
    pub catalog: Arc<Catalog>,
    pub live: Live,
    pub probe: Arc<dyn Probe + Send + Sync>,
    pub codex_snapshot: Option<PathBuf>,
}

impl Service {
    pub fn settings_file(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    pub fn profile(&self) -> Profile {
        Profile::load(&self.settings_file())
    }

    pub fn save_profile(&self, profile: &Profile) -> Result<(), String> {
        let stored = profile.to_stored();
        let mut store = Settings::open(&self.settings_file()).map_err(|e| e.to_string())?;
        let said = |e: crate::settings::SettingsError| e.to_string();
        if let Some(preset) = stored.preset {
            store.set(KEY_PRESET, Value::from(preset)).map_err(said)?;
        }
        if let Some(picks) = stored.picks {
            store.set(KEY_PICKS, Value::from(picks)).map_err(said)?;
        }
        for (key, on) in KEY_TOOLS.iter().zip(&stored.tools) {
            if let Some(on) = on {
                store.set(key, Value::from(*on)).map_err(said)?;
            }
        }
        if let Some(app) = stored.app {
            store.set(APP_KEY, app).map_err(said)?;
        }
        Ok(())
    }

    pub fn available(&self) -> Available {
        let live = (self.live)();
        let models = codex_models::load_with(&self.roots.codex, self.codex_snapshot.as_deref());
        Available::from_catalog(&self.catalog, &live).with_codex(&models)
    }

    pub fn manifest(&self) -> Result<Manifest, String> {
        Manifest::load(&self.data_dir)
    }

    fn with_env<T>(&self, run: impl FnOnce(&Env, &Profile) -> T) -> Result<T, String> {
        let manifest = self.manifest()?;
        let (avail, profile) = (self.available(), self.profile());
        let env = Env {
            roots: &self.roots,
            data_dir: &self.data_dir,
            avail: &avail,
            probe: self.probe.as_ref(),
            manifest: &manifest,
        };
        Ok(run(&env, &profile))
    }

    pub fn plan(&self, opts: &Options) -> Result<Plan, String> {
        self.with_env(|env, profile| plan(env, profile, opts))
    }

    pub fn apply(
        &self,
        opts: &Options,
        stamp: u64,
    ) -> Result<(Plan, Applied, Option<String>), String> {
        self.apply_only(opts, stamp, None)
    }

    pub fn apply_only(
        &self,
        opts: &Options,
        stamp: u64,
        only: Option<&Only>,
    ) -> Result<(Plan, Applied, Option<String>), String> {
        self.apply_narrowed(opts, stamp, |_| only.cloned())
    }

    pub fn apply_narrowed(
        &self,
        opts: &Options,
        stamp: u64,
        narrow: impl FnOnce(&Plan) -> Option<Only>,
    ) -> Result<(Plan, Applied, Option<String>), String> {
        let (planned, applied) = self.with_env(|env, profile| {
            let planned = plan(env, profile, opts);
            let only = narrow(&planned);
            let applied = apply_only(env, &planned, stamp, only.as_ref());
            (planned, applied)
        })?;
        let saved = applied
            .results
            .iter()
            .any(|r| r.done)
            .then(|| self.save_profile(&applied.profile).err())
            .flatten();
        Ok((planned, applied, saved))
    }

    pub fn undo(&self, tool: Tool, stamp: u64) -> Result<Vec<UndoResult>, String> {
        self.undo_only(tool, stamp, None)
    }

    pub fn undo_only(
        &self,
        tool: Tool,
        stamp: u64,
        only: Option<&super::undo::Only>,
    ) -> Result<Vec<UndoResult>, String> {
        self.with_env(|env, _| undo_only(env, tool, stamp, only).0)
    }

    pub fn undo_parts(&self, tool: Tool) -> Result<Vec<UndoPart>, String> {
        self.with_env(|env, _| {
            env.manifest
                .of_tool(tool)
                .into_iter()
                .map(|e| UndoPart {
                    id: e.id.clone(),
                    path: e.path.clone(),
                    removes: !e.existed && !e.removed,
                    now: current_hash(env, e),
                })
                .collect()
        })
    }

    pub fn check(&self) -> Result<Vec<Row>, String> {
        self.with_env(check)
    }

    pub fn resolve(&self, id: &str, choice: Choice, stamp: u64) -> Result<Resolved, String> {
        let done = self.with_env(|env, profile| resolve(env, profile, id, choice, stamp))??;
        self.save_profile(&done.profile)?;
        Ok(done)
    }
}
