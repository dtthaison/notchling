use serde::{Deserialize, Serialize};

pub const USER_DELETED_KEEP: usize = 16;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct UserDeleted {
    pub months: Vec<u32>,
    pub at: Vec<i64>,
    pub count: u64,
}

impl UserDeleted {
    pub fn push(&mut self, month: u32, at: i64) {
        self.months.push(month);
        self.at.push(at);
        self.count += 1;
        while self.months.len() > USER_DELETED_KEEP {
            self.months.remove(0);
        }
        while self.at.len() > USER_DELETED_KEEP {
            self.at.remove(0);
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Checkpoint {
    pub n: u64,
    pub seal: String,
    pub month: u32,
    pub settings_hash: Option<String>,
    pub prune_anchor: Option<String>,
    pub user_deleted: UserDeleted,
    pub at: i64,
}
