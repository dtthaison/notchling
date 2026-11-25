use notchling_engine::cost::account_log;
use notchling_engine::search::index::AccountOf;
use notchling_engine::sessions::{lane_at_path, Roots};
use std::path::PathBuf;
use std::sync::Arc;

pub fn resolver(roots: Roots, data_dir: PathBuf) -> AccountOf {
    Arc::new(move |config_dir, at| match config_dir {
        Some(path) => lane_at_path(&roots, path)
            .map(|lane| lane.account_id)
            .unwrap_or_default(),
        None => account_log::account_at(at, &account_log::load(&account_log::path(&data_dir))),
    })
}

#[cfg(test)]
#[path = "search_accounts_tests.rs"]
mod tests;
