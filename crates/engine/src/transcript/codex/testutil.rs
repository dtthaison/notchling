use crate::sessions::testutil::TempDir;
use std::path::PathBuf;

pub(crate) struct Fixture {
    pub thread: &'static str,
    pub name: &'static str,
    pub text: &'static str,
}

macro_rules! rollout {
    ($stamp:literal, $suffix:literal) => {
        Fixture {
            thread: concat!("01a1d5da-0000-7000-8000-00000000000", $suffix),
            name: concat!(
                "rollout-2026-11-13T",
                $stamp,
                "-01a1d5da-0000-7000-8000-00000000000",
                $suffix,
                ".jsonl"
            ),
            text: include_str!(concat!(
                "../../../tests/fixtures/codex-children/sessions/2026/11/13/rollout-2026-11-13T",
                $stamp,
                "-01a1d5da-0000-7000-8000-00000000000",
                $suffix,
                ".jsonl"
            )),
        }
    };
}

pub(crate) const PARENT: Fixture = rollout!("09-00-00", "a");
pub(crate) const FORKED: Fixture = rollout!("09-00-20", "b");
pub(crate) const PLAIN: Fixture = rollout!("09-00-40", "c");
pub(crate) const OLD_FORK: Fixture = rollout!("09-01-00", "d");

pub(crate) const ALL: [&Fixture; 4] = [&PARENT, &FORKED, &PLAIN, &OLD_FORK];

impl Fixture {
    pub(crate) fn write_in(&self, dir: &TempDir, base: &str) -> PathBuf {
        dir.write(&format!("{base}/2026/11/13/{}", self.name), self.text)
    }

    pub(crate) fn path_in(&self, dir: &TempDir, base: &str) -> String {
        self.write_in(dir, base).to_string_lossy().into_owned()
    }

    pub(crate) fn offset_of_line(&self, index: usize) -> u64 {
        self.text
            .split_inclusive('\n')
            .take(index)
            .map(|line| line.len() as u64)
            .sum()
    }

    pub(crate) fn up_to_line(&self, index: usize) -> String {
        self.text.split_inclusive('\n').take(index).collect()
    }
}

pub(crate) fn tree(tag: &str) -> TempDir {
    let dir = TempDir::new(tag);
    for fixture in ALL {
        fixture.write_in(&dir, "sessions");
    }
    dir
}
