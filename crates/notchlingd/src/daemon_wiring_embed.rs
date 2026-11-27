use crate::daemon_wiring::{Ops, Wired};
use crate::embed_fetch::ModelNet;
use crate::embed_ops::{EmbedOps, EmbedParts, EmbedSettings, PluggedIn, PowerSource, SystemPower};
use crate::embed_paths;
use crate::layout::Layout;
use crate::pages_ops::{PagesOps, SearchSettings};
use notchling_engine::search::embed::helper::ProcessSpawn;
use notchling_engine::search::embed::{Embedder, ModelId, StaticEmbedder};
use notchling_engine::settings::Settings;
use std::path::PathBuf;
use std::sync::{Arc, Weak};

pub fn read(file: &std::path::Path, search: &SearchSettings) -> EmbedSettings {
    let store = Settings::open(file).ok();
    let flag = |key: &str| {
        store
            .as_ref()
            .and_then(|s| s.get(key))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    };
    let models = store
        .as_ref()
        .and_then(|s| s.get("semanticModel"))
        .and_then(|v| {
            v.as_array().map(|a| {
                a.iter()
                    .filter_map(|m| m.as_str().map(str::to_string))
                    .collect()
            })
        })
        .unwrap_or_default();
    EmbedSettings {
        on: flag("semanticSearch"),
        paused: flag("semanticPaused"),
        models,
        range: search.range,
        categories: search.on.to_vec(),
    }
}

pub(crate) fn statics(data: PathBuf) -> Result<Option<Arc<dyn Embedder>>, String> {
    if !embed_paths::static_present(&data) {
        return Ok(None);
    }
    let dir = embed_paths::static_dir(&data);
    let table = dir.join(embed_paths::TABLE);
    let opened = StaticEmbedder::open(&table, &dir.join("tokenizer.json"))
        .map_err(|e| format!("The static model could not be opened ({e})."))?;
    Ok(Some(Arc::new(opened)))
}

pub fn build(wired: &Wired, layout: &Layout, pages: &Arc<PagesOps>) -> Arc<EmbedOps> {
    let data = layout.data_dir.clone();
    let file = data.join("settings.json");
    let exe = std::env::current_exe().unwrap_or_default();
    let weak: Weak<PagesOps> = Arc::downgrade(pages);
    let real = std::env::var("NOTCHLING_EMBED_REAL").is_ok_and(|v| v == "1");
    let search_file = file.clone();
    let static_data = data.clone();
    let fixture = layout.fixture && !real;
    let power: Arc<dyn PowerSource> = if fixture {
        Arc::new(PluggedIn)
    } else {
        Arc::new(SystemPower)
    };
    let net = if fixture {
        ModelNet::offline()
    } else {
        ModelNet::real(embed_paths::helper_exe(&exe))
    };
    let embed = EmbedOps::with_parts(EmbedParts {
        data_dir: data.clone(),
        room: wired.acting.room.clone(),
        settings: Arc::new(move || read(&file, &SearchSettings::read(&search_file))),
        corpora: Arc::new(move |settings| match weak.upgrade() {
            Some(pages) => pages.embed_corpora(&settings.categories),
            None => Vec::new(),
        }),
        spawn: Arc::new(ProcessSpawn {
            exe: embed_paths::helper_exe(&exe),
            model_dir: embed_paths::granite_dir(&data),
        }),
        granite_id: ModelId(embed_paths::GRANITE_ID.to_string()),
        granite_present: false,
        statics: Arc::new(move || statics(static_data.clone())),
        fixture,
        power,
        net,
        now: Arc::new(crate::pages_ops::system_now),
    });
    pages.bind_embed(embed.clone());
    embed
}

pub fn wire(ops: Ops, wired: &Wired, _layout: &Layout) -> Ops {
    match wired.embed.get() {
        Some(embed) => {
            let read = embed.clone();
            let asks = crate::model_asks::ModelAsks::new(
                wired.consent.clone(),
                Arc::new(move || read.status()),
            );
            ops.with_group(embed.clone())
                .with_group(asks.clone())
                .with_cited(asks)
        }
        None => ops,
    }
}

impl Wired {
    pub fn load(&self) {
        self.inbox.load();
        self.ask.load(crate::acting_ops::now_ms());
        self.consent.load();
        self.outbox.load(crate::acting_ops::now_ms());
    }

    pub fn accounts_switch(&self) -> &Arc<crate::switches::SwitchFlag> {
        &self.switches.accounts
    }
}
