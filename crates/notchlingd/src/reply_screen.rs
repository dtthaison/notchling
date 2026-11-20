use crate::acting_events::why_of;
use crate::acting_ops::keep_screen;
use notchling_engine::reply::exec::Failure;
pub(crate) use notchling_engine::terminal::input_box::masked;
use std::path::{Path, PathBuf};

pub(crate) const KEPT_SCREENS: usize = 20;
pub(crate) const SCREENS_DIR: &str = "reply-screens";
const SESSION_SHORT: usize = 8;

fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

pub(crate) fn stamp(ms: i64) -> String {
    let (year, month, day) = civil(ms.div_euclid(86_400_000));
    let of_day = ms.rem_euclid(86_400_000);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}.{:03}Z",
        of_day / 3_600_000,
        of_day / 60_000 % 60,
        of_day / 1_000 % 60,
        of_day % 1_000
    )
}

fn why(failure: &Failure) -> String {
    match failure {
        Failure::Mixed => "mixed".to_string(),
        other => why_of(other).name().to_string(),
    }
}

fn word(text: &str) -> String {
    text.chars()
        .take(SESSION_SHORT)
        .map(|c| match c.is_ascii_alphanumeric() || c == '_' {
            true => c,
            false => '-',
        })
        .collect()
}

pub(crate) fn screen_name(ms: i64, job: &str, failure: &Failure) -> String {
    format!("{}-{}-{}.txt", stamp(ms), word(job), why(failure))
}

fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut kept: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "txt"))
        .collect();
    kept.sort();
    let extra = kept.len().saturating_sub(KEPT_SCREENS);
    for old in kept.into_iter().take(extra) {
        let _ = std::fs::remove_file(old);
    }
}

pub(crate) fn keep(
    file: &Path,
    job: &str,
    failure: &Failure,
    head: &[String],
    screen: &[String],
    now_ms: i64,
) {
    let lines = masked(screen);
    keep_screen(file, head, &lines);
    let Some(dir) = file.parent().map(|parent| parent.join(SCREENS_DIR)) else {
        return;
    };
    {
        use std::os::unix::fs::DirBuilderExt;
        if std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .is_err()
        {
            return;
        }
    }
    keep_screen(&dir.join(screen_name(now_ms, job, failure)), head, &lines);
    prune(&dir);
}

pub(crate) fn head(
    job: &str,
    failure: &Failure,
    chars: usize,
    cursor: Option<(usize, usize)>,
    trail: &[String],
) -> Vec<String> {
    let cursor = match cursor {
        Some((row, col)) => format!("cursor: row {row}, column {col}"),
        None => "cursor: hidden".to_string(),
    };
    let mut head = vec![
        format!("job: {job}"),
        format!("refused: {failure}"),
        format!("message: {chars} characters"),
        cursor,
        format!("----- what each step saw ({} notes) -----", trail.len()),
    ];
    head.extend(trail.iter().cloned());
    head
}
