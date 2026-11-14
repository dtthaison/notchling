#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityRow {
    pub seq: u64,
    pub ts: i64,
    pub kind: String,
    pub kv: u32,
    pub grp: String,
    pub outcome: String,
    pub actor_tier: Option<String>,
    pub actor_via: Option<String>,
    pub actor_exe: Option<String>,
    pub actor_session: Option<String>,
    pub session: Option<String>,
    pub folder: Option<String>,
    pub account: Option<String>,
    pub device: Option<String>,
    pub op_id: Option<String>,
    pub host: String,
    pub audit_n: Option<u64>,
    pub spooled: bool,
    pub legacy: bool,
    pub f: String,
    pub expires: i64,
}

impl ActivityRow {
    pub fn blank(seq: u64, ts: i64, kind: &str) -> ActivityRow {
        ActivityRow {
            seq,
            ts,
            kind: kind.to_owned(),
            kv: 1,
            grp: String::new(),
            outcome: "ok".to_owned(),
            actor_tier: None,
            actor_via: None,
            actor_exe: None,
            actor_session: None,
            session: None,
            folder: None,
            account: None,
            device: None,
            op_id: None,
            host: String::new(),
            audit_n: None,
            spooled: false,
            legacy: false,
            f: "{}".to_owned(),
            expires: i64::MAX,
        }
    }

    pub fn fields(&self) -> serde_json::Value {
        serde_json::from_str(&self.f).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DbQuery {
    pub since: Option<i64>,
    pub before_seq: Option<u64>,
    pub kinds: Vec<String>,
    pub groups: Vec<String>,
    pub session: Option<String>,
    pub limit: usize,
}
