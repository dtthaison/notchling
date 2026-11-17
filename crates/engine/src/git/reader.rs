use super::branch::{remote_default, DIFF_FLAGS, FALLBACK_BASES};
use super::guard::clean_ref;
use super::read::{
    command, drivers, refusal, run, run_capped, whole_records, Failure, MAX_LIST_BYTES,
};
use super::refs::{
    parse_ahead_behind, parse_left_right, parse_refs, parse_shortstat, plain_label, remote_hosts,
    Counts, Lines, RefRow, REF_FORMAT, SUBJECT_CHARS,
};
use super::worktrees::{parse_worktrees, WorktreeEntry};
use crate::control::{LaunchOutput, Launcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const GITHUB: &str = "github.com";

pub struct Reader<'a> {
    cwd: String,
    launcher: &'a dyn Launcher,
    drivers: Vec<String>,
}

fn commit_arg(text: &str) -> Result<&str, Failure> {
    let hex = text.len() == 40 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if hex {
        Ok(text)
    } else {
        Err(Failure::Refused("not a commit".to_owned()))
    }
}

fn rev_arg(text: &str) -> Result<&str, Failure> {
    clean_ref(text).map_err(|why| Failure::Refused(why.to_owned()))
}

fn whole_lines(text: &str, cut: bool) -> &str {
    if !cut {
        return text;
    }
    match text.rfind('\n') {
        Some(at) => &text[..=at],
        None => "",
    }
}

fn millis(time: std::time::SystemTime) -> Option<i64> {
    let since = time.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(since.as_millis()).ok()
}

impl<'a> Reader<'a> {
    pub fn open(cwd: &str, launcher: &'a dyn Launcher) -> Result<Reader<'a>, Failure> {
        let drivers = drivers(cwd, launcher)?;
        Ok(Reader {
            cwd: cwd.to_owned(),
            launcher,
            drivers,
        })
    }

    pub fn at(&self, cwd: &str) -> Reader<'a> {
        Reader {
            cwd: cwd.to_owned(),
            launcher: self.launcher,
            drivers: self.drivers.clone(),
        }
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    fn run(&self, args: &[&str]) -> Result<LaunchOutput, Failure> {
        run(self.launcher, &command(&self.cwd, &self.drivers, args))
    }

    fn run_list(&self, args: &[&str]) -> Result<(LaunchOutput, bool), Failure> {
        run_capped(
            self.launcher,
            &command(&self.cwd, &self.drivers, args),
            MAX_LIST_BYTES,
        )
    }

    fn checked(&self, out: &LaunchOutput) -> Result<(), Failure> {
        if out.code == 0 {
            Ok(())
        } else {
            Err(refusal(self.launcher, &self.cwd, out))
        }
    }

    pub fn common_dir(&self) -> Result<PathBuf, Failure> {
        let out = self.run(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
        self.checked(&out)?;
        let dir = out.output.trim_end_matches(['\n', '\r']);
        if dir.is_empty() {
            return Err(Failure::Failed("no repository folder".to_owned()));
        }
        Ok(PathBuf::from(dir))
    }

    pub fn worktrees(&self) -> Result<(Vec<WorktreeEntry>, bool), Failure> {
        let (out, cut) = self.run_list(&["worktree", "list", "--porcelain", "-z"])?;
        self.checked(&out)?;
        let (list, listed_cut) = parse_worktrees(whole_records(&out.output, cut));
        Ok((list, listed_cut || cut))
    }

    pub fn refs(&self) -> Result<(Vec<RefRow>, bool), Failure> {
        let format = format!("--format={REF_FORMAT}");
        let (out, cut) = self.run_list(&[
            "for-each-ref",
            "--sort=-committerdate",
            &format,
            "refs/heads",
            "refs/remotes",
        ])?;
        self.checked(&out)?;
        let (rows, listed_cut) = parse_refs(whole_lines(&out.output, cut));
        Ok((rows, listed_cut || cut))
    }

    pub fn ahead_behind_all(
        &self,
        base_rev: &str,
    ) -> Result<Option<HashMap<String, Counts>>, Failure> {
        let base = rev_arg(base_rev)?;
        let format = format!("--format=%(refname)%00%(ahead-behind:{base})");
        let (out, cut) = self.run_list(&["for-each-ref", &format, "refs/heads", "refs/remotes"])?;
        if out.code != 0 {
            return Ok(None);
        }
        Ok(Some(parse_ahead_behind(whole_lines(&out.output, cut))))
    }

    pub fn left_right(&self, base_rev: &str, head: &str) -> Result<Option<Counts>, Failure> {
        let range = format!("{}...{}", rev_arg(base_rev)?, commit_arg(head)?);
        let out = self.run(&["rev-list", "--left-right", "--count", &range, "--"])?;
        if out.code != 0 {
            return Ok(None);
        }
        Ok(parse_left_right(&out.output))
    }

    pub fn lines(&self, base: &str, head: &str) -> Result<Option<Lines>, Failure> {
        let range = format!("{}...{}", rev_arg(base)?, commit_arg(head)?);
        let mut args = vec!["diff", "--shortstat"];
        args.extend(DIFF_FLAGS);
        args.push(&range);
        args.push("--");
        let out = self.run(&args)?;
        match out.code {
            0 => Ok(Some(parse_shortstat(&out.output))),
            128 => Ok(None),
            _ => Err(refusal(self.launcher, &self.cwd, &out)),
        }
    }

    pub fn related(&self, base_commit: &str, head_commit: &str) -> Result<bool, Failure> {
        let base = commit_arg(base_commit)?;
        let head = commit_arg(head_commit)?;
        let out = self.run(&["merge-base", base, head])?;
        match out.code {
            0 => Ok(true),
            1 => Ok(false),
            code => Err(Failure::Failed(format!("git merge-base exited {code}"))),
        }
    }

    pub fn commit_facts(&self, commit: &str) -> Result<(Option<i64>, String), Failure> {
        let commit = commit_arg(commit)?;
        let out = self.run(&[
            "log",
            "-1",
            "--no-show-signature",
            "--format=%ct%x00%s",
            commit,
            "--",
        ])?;
        self.checked(&out)?;
        let line = out.output.lines().next().unwrap_or("");
        let (seconds, subject) = line.split_once('\0').unwrap_or((line, ""));
        let at = seconds
            .parse::<i64>()
            .ok()
            .and_then(|seconds| seconds.checked_mul(1000));
        Ok((at, plain_label(subject, SUBJECT_CHARS)))
    }

    pub fn remotes(&self) -> Result<Vec<(String, bool)>, Failure> {
        let out = self.run(&["config", "-z", "--get-regexp", "^remote\\..*\\.url$"])?;
        if out.code != 0 && out.code != 1 {
            return Err(Failure::Failed(format!("git config exited {}", out.code)));
        }
        Ok(remote_hosts(&out.output)
            .into_iter()
            .map(|(name, host)| (name, host.as_deref() == Some(GITHUB)))
            .collect())
    }

    pub fn default_rev(&self, local: &[String]) -> Option<String> {
        let remote = remote_default(self.launcher, &self.cwd, &self.drivers).filter(|name| {
            let tip = format!("{name}^{{commit}}");
            self.run(&["rev-parse", "--verify", "--quiet", &tip])
                .is_ok_and(|out| out.code == 0)
        });
        remote.or_else(|| {
            FALLBACK_BASES
                .iter()
                .find(|name| local.iter().any(|l| l == **name))
                .map(|name| (*name).to_owned())
        })
    }

    pub fn fetched_at(common_dir: &Path) -> Option<i64> {
        let meta = std::fs::symlink_metadata(common_dir.join("FETCH_HEAD")).ok()?;
        millis(meta.modified().ok()?)
    }
}
