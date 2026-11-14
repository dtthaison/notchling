use super::db::{DbError, EventsDb};
use super::import::{
    assign_seq, import_account_log, import_legacy_audit, import_remote_log, read_text,
};
use super::row::ActivityRow;
use std::path::PathBuf;

pub const AUDIT_CAP: u64 = 16 * 1024 * 1024;
pub const PHONE_CAP: u64 = 16 * 1024 * 1024;
pub const ACCOUNT_CAP: u64 = 1024 * 1024;

pub const META_AUDIT: &str = "imported_audit";
pub const META_REMOTE_LOG: &str = "imported_remote_log";
pub const META_ACCOUNT_LOG: &str = "imported_account_log";

#[derive(Clone, Debug)]
pub struct ImportFiles {
    pub audit: Vec<PathBuf>,
    pub remote_log: PathBuf,
    pub account_log: PathBuf,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub audit_rows: u64,
    pub remote_rows: u64,
    pub account_rows: u64,
    pub skipped: u64,
}

pub fn import_once(
    db: &mut EventsDb,
    files: &ImportFiles,
    host: &str,
) -> Result<ImportReport, DbError> {
    let mut report = ImportReport::default();
    let mut rows: Vec<ActivityRow> = Vec::new();
    let mut done: Vec<&str> = Vec::new();
    if db.meta_get(META_AUDIT).is_none() {
        for path in &files.audit {
            let imported = import_legacy_audit(&read_text(path, AUDIT_CAP), host);
            report.audit_rows += imported.rows.len() as u64;
            report.skipped += imported.skipped;
            rows.extend(imported.rows);
        }
        done.push(META_AUDIT);
    }
    if db.meta_get(META_REMOTE_LOG).is_none() {
        let imported = import_remote_log(&read_text(&files.remote_log, PHONE_CAP), host);
        report.remote_rows = imported.rows.len() as u64;
        report.skipped += imported.skipped;
        rows.extend(imported.rows);
        done.push(META_REMOTE_LOG);
    }
    if db.meta_get(META_ACCOUNT_LOG).is_none() {
        let imported = import_account_log(&read_text(&files.account_log, ACCOUNT_CAP), host);
        report.account_rows = imported.rows.len() as u64;
        report.skipped += imported.skipped;
        rows.extend(imported.rows);
        done.push(META_ACCOUNT_LOG);
    }
    if !rows.is_empty() {
        assign_seq(&mut rows, db.max_seq() + 1);
        db.insert(&rows)?;
    }
    for key in done {
        db.meta_set(key, "1")?;
    }
    Ok(report)
}
