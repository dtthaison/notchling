use super::{Kind, Organizations};
use crate::machines::registry::RegistryFiles;
use crate::sessions::testutil::TempDir;
use crate::store;
use std::path::PathBuf;

pub const CLAUDE: &str = r#"[{"id":"c1","email":"a@example.com","plan":"Max 5x","needsLogin":false},{"id":"c2","email":"b@example.com","plan":"Pro","needsLogin":true,"futureField":7}]"#;

pub const CODEX: &str = r#"{
  "accounts": [
    {
      "email": "x@example.com",
      "id": "u1:w1",
      "plan": "Plus"
    },
    {
      "email": "y@example.com",
      "id": "u2:w2",
      "plan": null
    }
  ],
  "version": 1
}"#;

pub const ANTIGRAVITY: &str =
    r#"{"version":1,"accounts":[{"id":"g1","email":"g@example.com","plan":"Pro"}]}"#;

pub const VAULT: &str = r#"[
  {
    "kind": "token",
    "name": "github-read",
    "note": "",
    "secretVar": "GITHUB_READ_TOKEN",
    "updated": "2026-11-01",
    "url": "https://github.com",
    "userVar": ""
  },
  {
    "kind": "login",
    "name": "Aptus-Deploy",
    "note": "deploy login",
    "secretVar": "APTUS_DEPLOY_PASSWORD",
    "updated": "2026-11-02",
    "userVar": "APTUS_DEPLOY_USER"
  }
]"#;

pub const REGISTRY_V2: &str = r#"{
  "machines": [
    {
      "claudeSecret": "claude-worker",
      "host": "m1.tail",
      "name": "m1",
      "user": "sondoan"
    },
    {
      "claudeSecret": "claude-worker",
      "host": "oracle-sg",
      "name": "oracle-sg",
      "user": "opc"
    }
  ],
  "version": 2
}"#;

pub struct Rig {
    pub tmp: TempDir,
    pub data: PathBuf,
    pub orgs: Organizations,
    pub registry: RegistryFiles,
}

impl Rig {
    pub fn new(tag: &str) -> Rig {
        let tmp = TempDir::new(tag);
        let data = tmp.path("data");
        let orgs = Organizations::new(&data);
        let registry = RegistryFiles::single(&data);
        Rig {
            tmp,
            data,
            orgs,
            registry,
        }
    }

    pub fn full(tag: &str) -> Rig {
        let rig = Rig::new(tag);
        rig.put(Kind::Claude, CLAUDE);
        rig.put(Kind::Codex, CODEX);
        rig.put(Kind::Antigravity, ANTIGRAVITY);
        rig.put(Kind::Vault, VAULT);
        store::put_doc(&rig.data, "machines/registry", REGISTRY_V2).unwrap();
        rig
    }

    pub fn put(&self, kind: Kind, text: &str) {
        store::put_doc(&self.data, kind.doc(), text).unwrap();
    }

    pub fn text(&self, kind: Kind) -> Option<String> {
        store::doc(&self.data, kind.doc()).unwrap()
    }

    pub fn orgs_text(&self) -> Option<String> {
        store::doc(&self.data, super::DOC).unwrap()
    }

    pub fn registry_text(&self) -> Option<String> {
        store::doc(&self.data, "machines/registry").unwrap()
    }

    pub fn backups(&self) -> Vec<String> {
        let dir = self.data.join(super::backup::DIR);
        let Ok(read) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = read
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    pub fn backup_text(&self, kind: Kind) -> Option<String> {
        std::fs::read_to_string(super::backup::path(&self.data, kind.doc())).ok()
    }
}
