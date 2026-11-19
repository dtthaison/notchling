use super::action::{Env, Options, Plan};
use super::apply::{apply, Applied};
use super::check::{check, resolve, Choice, Resolved, Row};
use super::keys::Probe;
use super::manifest::Manifest;
use super::model::{Profile, Stored, APP_KEY, KEY_PICKS, KEY_PRESET, KEY_TOOLS};
use super::plan::plan;
use super::resolve::Available;
use super::undo::{undo, UndoResult};
use crate::catalog::{Catalog, LiveModel, Tool};
use crate::install::ToolRoots;
use crate::settings::Settings;
use crate::tools::codex::models as codex_models;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

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
        match Settings::open(&self.settings_file()) {
            Ok(store) => Profile::from_stored(&Stored {
                preset: store.get_str(KEY_PRESET),
                picks: store.get_str(KEY_PICKS),
                tools: KEY_TOOLS.iter().map(|k| store.get_bool(k)).collect(),
                app: store.raw(APP_KEY).cloned(),
            }),
            Err(_) => Profile::default(),
        }
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
        let (planned, applied) = self.with_env(|env, profile| {
            let planned = plan(env, profile, opts);
            let applied = apply(env, &planned, stamp);
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
        self.with_env(|env, _| undo(env, tool, stamp).0)
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
