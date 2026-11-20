use crate::acting_events::why_of;
use crate::acting_ops::keep_screen;
use notchling_engine::reply::exec::Failure;
pub(crate) use notchling_engine::terminal::input_box::masked;
use notchling_engine::terminal::input_box::{span, MASK, RULE};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

pub(crate) const KEPT_SCREENS: usize = 20;
pub(crate) const SCREENS_DIR: &str = "reply-screens";
const SESSION_SHORT: usize = 8;
const ROWS_ABOVE: usize = 3;
const ROWS_BELOW: usize = 3;
const ROWS_WITHOUT_BOX: usize = 20;
const SAME_NAME_TRIES: usize = 1_000;
const SCREEN_WORDS: [&str; 3] = ["starting: ", "last row: ", "under it: "];
static KEEPING: Mutex<()> = Mutex::new(());

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

fn regular(entry: &std::fs::DirEntry) -> bool {
    entry.file_type().is_ok_and(|kind| kind.is_file())
        && entry.path().extension().is_some_and(|ext| ext == "txt")
}

fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut kept: Vec<(Option<SystemTime>, PathBuf)> = entries
        .flatten()
        .filter(regular)
        .map(|entry| {
            let written = entry.metadata().and_then(|meta| meta.modified()).ok();
            (written, entry.path())
        })
        .collect();
    kept.sort();
    let extra = kept.len().saturating_sub(KEPT_SCREENS);
    for (_, old) in kept.into_iter().take(extra) {
        let _ = std::fs::remove_file(old);
    }
}

pub(crate) fn shape(line: &str) -> String {
    line.chars()
        .map(|c| match c.is_whitespace() || c == RULE {
            true => c,
            false => MASK,
        })
        .collect()
}

fn shaped_notes(head: &[String]) -> Vec<String> {
    head.iter()
        .map(|note| {
            for marker in SCREEN_WORDS {
                if let Some(at) = note.find(marker) {
                    let cut = at + marker.len();
                    return format!("{}{}", &note[..cut], shape(&note[cut..]));
                }
            }
            note.clone()
        })
        .collect()
}

pub(crate) fn excerpt(screen: &[String]) -> Vec<String> {
    let lines = masked(screen);
    let boxed = span(screen);
    let (from, to) = match boxed {
        Some((top, bottom)) => (
            top.saturating_sub(ROWS_ABOVE),
            (bottom + ROWS_BELOW + 1).min(lines.len()),
        ),
        None => {
            let end = lines.iter().rposition(|row| !row.trim().is_empty());
            let end = end.map_or(0, |last| last + 1);
            (end.saturating_sub(ROWS_WITHOUT_BOX), end)
        }
    };
    (from..to)
        .map(|at| match boxed {
            Some((top, bottom)) if at > top && at < bottom => lines[at].clone(),
            _ => shape(&lines[at]),
        })
        .collect()
}

fn private_dir(dir: &Path) -> bool {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let made = std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir);
    let plain = std::fs::symlink_metadata(dir).is_ok_and(|meta| meta.is_dir());
    if made.is_err() || !plain {
        return false;
    }
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    true
}

fn write_new(dir: &Path, name: &str, head: &[String], lines: &[String]) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let stem = name.strip_suffix(".txt").unwrap_or(name);
    for n in 0..SAME_NAME_TRIES {
        let file = match n {
            0 => dir.join(name),
            _ => dir.join(format!("{stem}-{n}.txt")),
        };
        let opened = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&file);
        match opened {
            Ok(mut out) => {
                for line in head {
                    let _ = writeln!(out, "{line}");
                }
                let _ = writeln!(out, "----- screen ({} lines) -----", lines.len());
                for line in lines {
                    let _ = writeln!(out, "{line}");
                }
                return;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return,
        }
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
    let _one_at_a_time = KEEPING.lock().unwrap_or_else(|held| held.into_inner());
    keep_screen(file, head, &masked(screen));
    let Some(dir) = file.parent().map(|parent| parent.join(SCREENS_DIR)) else {
        return;
    };
    if !private_dir(&dir) {
        return;
    }
    let name = screen_name(now_ms, job, failure);
    write_new(&dir, &name, &shaped_notes(head), &excerpt(screen));
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
