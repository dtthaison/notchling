use std::path::Path;

use serde_json::Value;

const MAIN_KEY: &str = "promptCacheTtl";
const HELPERS_KEY: &str = "subagentPromptCacheTtl";
const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ttl {
    FiveMinutes,
    OneHour,
    Other(String),
}

impl Ttl {
    fn parse(text: &str) -> Ttl {
        match text {
            "5m" => Ttl::FiveMinutes,
            "1h" => Ttl::OneHour,
            other => Ttl::Other(other.to_owned()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cache {
    pub main: Ttl,
    pub helpers: Ttl,
    pub set_main: bool,
    pub set_helpers: bool,
}

impl Default for Cache {
    fn default() -> Cache {
        Cache {
            main: Ttl::OneHour,
            helpers: Ttl::FiveMinutes,
            set_main: false,
            set_helpers: false,
        }
    }
}

impl Cache {
    pub fn parse(settings: &str) -> Cache {
        let mut cache = Cache::default();
        let Ok(v) = serde_json::from_str::<Value>(settings) else {
            return cache;
        };
        if let Some(text) = v.get(MAIN_KEY).and_then(Value::as_str) {
            cache.main = Ttl::parse(text);
            cache.set_main = true;
        }
        if let Some(text) = v.get(HELPERS_KEY).and_then(Value::as_str) {
            cache.helpers = Ttl::parse(text);
            cache.set_helpers = true;
        }
        cache
    }

    pub fn read(claude_dir: &Path) -> Cache {
        let path = claude_dir.join("settings.json");
        let small = std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= MAX_BYTES);
        if !small {
            return Cache::default();
        }
        std::fs::read_to_string(&path)
            .map(|text| Cache::parse(&text))
            .unwrap_or_default()
    }
}
