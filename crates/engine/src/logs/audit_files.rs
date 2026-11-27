use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const MS_PER_DAY: i64 = 86_400_000;
const TAIL_WINDOW: u64 = 64 * 1024;

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (if month <= 2 { y + 1 } else { y }, month)
}

pub fn month_of(ms: i64) -> u32 {
    let (year, month) = civil_from_days(ms.div_euclid(MS_PER_DAY));
    (year * 100 + month) as u32
}

pub fn month_name(month: u32) -> String {
    format!("{:04}-{:02}", month / 100, month % 100)
}

pub fn parse_month_name(name: &str) -> Option<u32> {
    let (year, month) = name.split_once('-')?;
    if year.len() != 4 || month.len() != 2 {
        return None;
    }
    let year: u32 = year.parse().ok()?;
    let month: u32 = month.parse().ok()?;
    ((1..=12).contains(&month)).then_some(year * 100 + month)
}

pub fn month_start_ms(month: u32) -> i64 {
    days_from_civil(i64::from(month / 100), i64::from(month % 100), 1) * MS_PER_DAY
}

pub fn month_end_ms(month: u32) -> i64 {
    let (year, mo) = (month / 100, month % 100);
    let next = if mo == 12 {
        (year + 1) * 100 + 1
    } else {
        month + 1
    };
    month_start_ms(next)
}

fn month_file(month: u32) -> String {
    format!("{}.jsonl", month_name(month))
}

fn month_from_file(name: &str) -> Option<u32> {
    parse_month_name(name.strip_suffix(".jsonl")?)
}

fn open_options(append: bool) -> OpenOptions {
    let mut options = OpenOptions::new();
    if append {
        options.append(true).create(true);
    } else {
        options.read(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
        if append {
            options.mode(0o600);
        }
    }
    options
}

fn tighten(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if file.metadata()?.permissions().mode() & 0o077 != 0 {
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
    }
    #[cfg(not(unix))]
    let _ = file;
    Ok(())
}

pub struct AuditDir {
    root: PathBuf,
    open: Option<(u32, File)>,
}

impl AuditDir {
    pub fn new(root: impl Into<PathBuf>) -> AuditDir {
        AuditDir {
            root: root.into(),
            open: None,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn legacy_dir(&self) -> PathBuf {
        self.root.join("legacy")
    }

    pub fn path_of(&self, month: u32) -> PathBuf {
        self.root.join(month_file(month))
    }

    pub fn ensure(&self) -> io::Result<()> {
        if let Ok(meta) = fs::symlink_metadata(&self.root) {
            if meta.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "the audit folder is a symlink",
                ));
            }
        }
        fs::create_dir_all(&self.root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    pub fn months(&self) -> io::Result<Vec<u32>> {
        let mut months = Vec::new();
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(months),
            Err(e) => return Err(e),
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            if let Some(month) = entry.file_name().to_str().and_then(month_from_file) {
                months.push(month);
            }
        }
        months.sort_unstable();
        Ok(months)
    }

    pub fn append(&mut self, month: u32, bytes: &[u8]) -> io::Result<()> {
        let current = matches!(&self.open, Some((open, _)) if *open == month);
        if !current {
            let file = open_options(true).open(self.path_of(month))?;
            tighten(&file)?;
            self.open = Some((month, file));
        }
        match self.open.as_mut() {
            Some((_, file)) => file.write_all(bytes),
            None => Err(io::Error::other("no audit file open")),
        }
    }

    pub fn flush(&mut self, full: bool) -> io::Result<()> {
        let Some((_, file)) = self.open.as_ref() else {
            return Ok(());
        };
        #[cfg(target_os = "macos")]
        if full {
            use std::os::fd::AsRawFd;
            if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } == 0 {
                return Ok(());
            }
            return file.sync_all();
        }
        let _ = full;
        file.sync_data()
    }

    pub fn read(&self, month: u32) -> io::Result<String> {
        let mut file = open_options(false).open(self.path_of(month))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    pub fn remove(&mut self, month: u32) -> io::Result<()> {
        if matches!(&self.open, Some((open, _)) if *open == month) {
            self.open = None;
        }
        fs::remove_file(self.path_of(month))
    }

    pub fn size(&self) -> u64 {
        let months = self.months().unwrap_or_default();
        months
            .into_iter()
            .filter_map(|m| fs::metadata(self.path_of(m)).ok())
            .map(|meta| meta.len())
            .sum()
    }

    pub fn month_size(&self, month: u32) -> u64 {
        fs::metadata(self.path_of(month))
            .map(|m| m.len())
            .unwrap_or(0)
    }

    pub fn tail_line(&self) -> io::Result<Option<String>> {
        let months = self.months()?;
        for month in months.into_iter().rev() {
            if let Some(line) = self.last_line_of(month)? {
                return Ok(Some(line));
            }
        }
        Ok(None)
    }

    fn last_line_of(&self, month: u32) -> io::Result<Option<String>> {
        let mut file = open_options(false).open(self.path_of(month))?;
        let len = file.metadata()?.len();
        let start = len.saturating_sub(TAIL_WINDOW);
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let text = String::from_utf8_lossy(&bytes);
        let body = if start > 0 {
            text.split_once('\n').map_or("", |(_, rest)| rest)
        } else {
            &text
        };
        Ok(body
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .map(str::to_owned))
    }
}
