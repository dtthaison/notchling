use std::cmp::Ordering;
use std::time::Duration;

use notchling_engine::control::{Command, Launcher};

use super::xml::{self, Node};

pub const FEED_URL: &str =
    "https://github.com/dtthaison/notchling/releases/download/appcast/appcast.xml";
pub const RELEASES_URL: &str = "https://github.com/dtthaison/notchling/releases";
pub const KILL_SWITCH: &str = "NOTCHLING_RELEASES";
pub const SETTING_KEY: &str = "checkForUpdates";
pub const INTERVAL_SECS: u64 = 24 * 60 * 60;
pub const RETRY_SECS: u64 = 60 * 60;
pub const TICK_SECS: u64 = 5 * 60;

const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_FEED: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub beta: Option<u64>,
}

impl Version {
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.strip_prefix('v').unwrap_or(text);
        let (core, beta) = match text.split_once("-beta.") {
            Some((core, n)) => (core, Some(n.parse::<u64>().ok()?)),
            None => (text, None),
        };
        let mut parts = core.split('.');
        let mut next = || parts.next()?.parse::<u64>().ok();
        let version = Version {
            major: next()?,
            minor: next()?,
            patch: next()?,
            beta,
        };
        parts.next().is_none().then_some(version)
    }

    pub fn is_beta(&self) -> bool {
        self.beta.is_some()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let core = |v: &Version| (v.major, v.minor, v.patch);
        core(self)
            .cmp(&core(other))
            .then_with(|| match (self.beta, other.beta) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(&b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        match self.beta {
            Some(n) => write!(f, "-beta.{n}"),
            None => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub critical: bool,
    pub notes: Option<String>,
}

impl Release {
    pub fn page(&self) -> String {
        self.notes
            .clone()
            .unwrap_or_else(|| RELEASES_URL.to_owned())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skip {
    Environment,
    Setting,
    Development,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Automatic,
    Manual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Skipped(Skip),
    UpToDate(Version),
    Available(Release),
    Failed(String),
}

impl Outcome {
    pub fn line(&self) -> String {
        match self {
            Outcome::Skipped(Skip::Environment) => {
                format!("Update checks are off ({KILL_SWITCH}=off).")
            }
            Outcome::Skipped(Skip::Setting) => "Update checks are off in Settings.".to_owned(),
            Outcome::Skipped(Skip::Development) => {
                "This is a development build, so it doesn't check for updates.".to_owned()
            }
            Outcome::UpToDate(v) => format!("Notchling {v} is up to date."),
            Outcome::Available(r) if r.critical => {
                format!("Notchling {} is available, a critical update.", r.version)
            }
            Outcome::Available(r) => format!("Notchling {} is available.", r.version),
            Outcome::Failed(why) => format!("Couldn't check for updates: {why}"),
        }
    }
}

pub trait FeedFetcher: Send + Sync {
    fn fetch(&self, url: &str) -> Result<String, String>;
}

pub struct CurlFetcher<L: Launcher> {
    launcher: L,
}

impl<L: Launcher> CurlFetcher<L> {
    pub fn new(launcher: L) -> Self {
        CurlFetcher { launcher }
    }

    pub fn command(url: &str) -> Command {
        Command::new(&[
            "/usr/bin/curl",
            "-q",
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "20",
            "--max-filesize",
            &MAX_FEED.to_string(),
            url,
        ])
    }
}

impl<L: Launcher> FeedFetcher for CurlFetcher<L> {
    fn fetch(&self, url: &str) -> Result<String, String> {
        if !url.starts_with("https://") {
            return Err("the feed isn't an https address".into());
        }
        let out = self
            .launcher
            .run(&Self::command(url), FETCH_TIMEOUT)
            .map_err(|e| e.to_string())?;
        if out.code != 0 {
            return Err("the feed couldn't be fetched".into());
        }
        Ok(out.output)
    }
}

pub struct NoFeed;

impl FeedFetcher for NoFeed {
    fn fetch(&self, _url: &str) -> Result<String, String> {
        Err("there is no network here".into())
    }
}

pub fn environment_off(value: Option<&str>) -> bool {
    value.is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no"
        )
    })
}

pub fn is_development(version: &str, build: Option<&str>) -> bool {
    let Some(parsed) = Version::parse(version) else {
        return true;
    };
    parsed.major == 0 || build.is_none_or(|b| b.is_empty() || b.ends_with("-dirty"))
}

pub fn skip(
    trigger: Trigger,
    switch: Option<&str>,
    setting: bool,
    version: &str,
    build: Option<&str>,
) -> Option<Skip> {
    if environment_off(switch) {
        return Some(Skip::Environment);
    }
    if trigger == Trigger::Manual {
        return None;
    }
    if !setting {
        return Some(Skip::Setting);
    }
    is_development(version, build).then_some(Skip::Development)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attempt {
    pub at: u64,
    pub failed: bool,
}

pub fn due(last: Option<Attempt>, now: u64) -> bool {
    let Some(last) = last else {
        return true;
    };
    let wait = if last.failed {
        RETRY_SECS
    } else {
        INTERVAL_SECS
    };
    now < last.at || now - last.at >= wait
}

fn safe_link(link: &str) -> Option<String> {
    let rest = link.strip_prefix("https://github.com/")?;
    let clean = !rest.is_empty()
        && !link
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\\'));
    clean.then(|| link.to_owned())
}

fn release_of(item: &Node) -> Option<(Release, bool)> {
    let short = item
        .child("sparkle:shortVersionString")
        .map(|n| n.text.as_str())
        .or_else(|| {
            item.child("enclosure")
                .and_then(|e| e.attr("sparkle:shortVersionString"))
        })?;
    let version = Version::parse(short)?;
    let beta_channel = item
        .child("sparkle:channel")
        .is_some_and(|c| c.text == "beta");
    if beta_channel != version.is_beta() {
        return None;
    }
    Some((
        Release {
            version,
            critical: item.child("sparkle:criticalUpdate").is_some(),
            notes: item
                .child("sparkle:releaseNotesLink")
                .and_then(|n| safe_link(&n.text)),
        },
        beta_channel,
    ))
}

pub struct Inputs<'a> {
    pub version: &'a str,
    pub build: Option<&'a str>,
    pub switch: Option<&'a str>,
    pub setting: bool,
}

pub fn run(trigger: Trigger, inputs: &Inputs, fetcher: &dyn FeedFetcher) -> Outcome {
    match skip(
        trigger,
        inputs.switch,
        inputs.setting,
        inputs.version,
        inputs.build,
    ) {
        Some(why) => Outcome::Skipped(why),
        None => check(inputs.version, fetcher),
    }
}

pub fn tick(
    last: &mut Option<Attempt>,
    now: u64,
    inputs: &Inputs,
    fetcher: &dyn FeedFetcher,
) -> Option<Outcome> {
    if let Some(why) = skip(
        Trigger::Automatic,
        inputs.switch,
        inputs.setting,
        inputs.version,
        inputs.build,
    ) {
        *last = None;
        return Some(Outcome::Skipped(why));
    }
    if !due(*last, now) {
        return None;
    }
    let outcome = check(inputs.version, fetcher);
    *last = Some(Attempt {
        at: now,
        failed: matches!(outcome, Outcome::Failed(_)),
    });
    Some(outcome)
}

pub fn newest(feed: &str, current: &Version) -> Result<Option<Release>, String> {
    if feed.len() > MAX_FEED {
        return Err("the feed is too large".into());
    }
    let root = xml::parse(feed).map_err(|e| format!("the feed can't be read ({e})"))?;
    let channel = root.child("channel").ok_or("the feed has no channel")?;
    let mut best: Option<Release> = None;
    for item in channel.children_named("item") {
        let Some((release, beta)) = release_of(item) else {
            continue;
        };
        if beta && !current.is_beta() {
            continue;
        }
        if best.as_ref().is_none_or(|b| release.version > b.version) {
            best = Some(release);
        }
    }
    Ok(best.filter(|r| r.version > *current))
}

pub fn check(current: &str, fetcher: &dyn FeedFetcher) -> Outcome {
    let Some(current) = Version::parse(current) else {
        return Outcome::Failed("this build's version isn't a release number".into());
    };
    let feed = match fetcher.fetch(FEED_URL) {
        Ok(feed) => feed,
        Err(why) => return Outcome::Failed(why),
    };
    match newest(&feed, &current) {
        Ok(Some(release)) => Outcome::Available(release),
        Ok(None) => Outcome::UpToDate(current),
        Err(why) => Outcome::Failed(why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::control::{FakeLauncher, LaunchOutput};
    use std::sync::Mutex;

    const FEED: &str = include_str!("../../fixtures/welcome/appcast.xml");
    const EMPTY: &str = include_str!("../../fixtures/welcome/appcast-empty.xml");

    struct Feed {
        text: Result<String, String>,
        asked: Mutex<Vec<String>>,
    }

    impl Feed {
        fn of(text: &str) -> Feed {
            Feed {
                text: Ok(text.to_owned()),
                asked: Mutex::new(Vec::new()),
            }
        }
        fn asked(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }
    }

    impl FeedFetcher for Feed {
        fn fetch(&self, url: &str) -> Result<String, String> {
            self.asked.lock().unwrap().push(url.to_owned());
            self.text.clone()
        }
    }

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn versions_parse_and_order() {
        assert_eq!(v("1.2.3").to_string(), "1.2.3");
        assert_eq!(v("v1.3.0-beta.2").to_string(), "1.3.0-beta.2");
        for bad in [
            "",
            "1.2",
            "1.2.3.4",
            "1.2.x",
            "1.2.3-beta",
            "1.2.3-rc.1",
            "-1.0.0",
        ] {
            assert!(Version::parse(bad).is_none(), "{bad}");
        }
        assert!(v("1.3.0-beta.2") > v("1.3.0-beta.1"));
        assert!(v("1.3.0") > v("1.3.0-beta.9"));
        assert!(v("1.3.0-beta.1") > v("1.2.9"));
        assert!(v("0.10.0") > v("0.9.9"));
    }

    fn inputs<'a>(
        version: &'a str,
        build: Option<&'a str>,
        switch: Option<&'a str>,
        setting: bool,
    ) -> Inputs<'a> {
        Inputs {
            version,
            build,
            switch,
            setting,
        }
    }

    #[test]
    fn only_the_off_words_turn_the_environment_off() {
        for off in [
            Some("off"),
            Some("OFF"),
            Some(" off "),
            Some("0"),
            Some("false"),
            Some("no"),
        ] {
            assert!(environment_off(off), "{off:?}");
        }
        for on in [
            None,
            Some(""),
            Some("on"),
            Some("1"),
            Some("yes"),
            Some("offline"),
        ] {
            assert!(!environment_off(on), "{on:?}");
        }
    }

    #[test]
    fn dev_builds_are_told_by_version_and_build_id() {
        assert!(!is_development("2.0.0", Some("0bdc0b0abcde")));
        assert!(!is_development("2.0.0-beta.1", Some("0bdc0b0abcde")));
        assert!(is_development("0.1.0", Some("0bdc0b0abcde")));
        assert!(is_development("2.0.0", Some("0bdc0b0abcde-dirty")));
        assert!(is_development("2.0.0", None));
        assert!(is_development("2.0.0", Some("")));
        assert!(is_development("dev", Some("0bdc0b0abcde")));
    }

    #[test]
    fn a_release_build_checks_by_default() {
        let release = inputs("2.0.0", Some("0bdc0b0abcde"), None, true);
        assert_eq!(
            skip(
                Trigger::Automatic,
                None,
                true,
                "2.0.0",
                Some("0bdc0b0abcde")
            ),
            None
        );
        let feed = Feed::of(FEED);
        let outcome = run(Trigger::Automatic, &release, &feed);
        assert_eq!(outcome, Outcome::UpToDate(v("2.0.0")));
        assert_eq!(feed.asked(), vec![FEED_URL.to_owned()]);
    }

    #[test]
    fn the_environment_setting_dev_builds_and_manual_checks_gate_the_request() {
        let feed = Feed::of(FEED);
        let cases = [
            (
                Trigger::Automatic,
                inputs("1.2.0", Some("abc"), Some("off"), true),
                Skip::Environment,
            ),
            (
                Trigger::Manual,
                inputs("1.2.0", Some("abc"), Some("off"), true),
                Skip::Environment,
            ),
            (
                Trigger::Automatic,
                inputs("1.2.0", Some("abc"), None, false),
                Skip::Setting,
            ),
            (
                Trigger::Automatic,
                inputs("1.2.0", Some("abc-dirty"), None, true),
                Skip::Development,
            ),
            (
                Trigger::Automatic,
                inputs("0.1.0", Some("abc"), None, true),
                Skip::Development,
            ),
        ];
        for (trigger, input, why) in cases {
            assert_eq!(run(trigger, &input, &feed), Outcome::Skipped(why));
        }
        assert!(feed.asked().is_empty(), "no request while skipped");
        let manual = inputs("1.2.0", Some("abc-dirty"), None, false);
        let outcome = run(Trigger::Manual, &manual, &feed);
        assert!(matches!(outcome, Outcome::Available(_)), "{outcome:?}");
        assert_eq!(feed.asked().len(), 1);
    }

    #[test]
    fn skipped_outcomes_say_why() {
        assert!(Outcome::Skipped(Skip::Environment)
            .line()
            .contains("NOTCHLING_RELEASES=off"));
        assert!(Outcome::Skipped(Skip::Setting).line().contains("Settings"));
        assert!(Outcome::Skipped(Skip::Development)
            .line()
            .contains("development"));
    }

    #[test]
    fn a_check_is_due_at_launch_then_daily_and_a_failure_retries_sooner() {
        assert!(due(None, 0));
        let ok = Some(Attempt {
            at: 100,
            failed: false,
        });
        assert!(!due(ok, 100));
        assert!(!due(ok, 100 + INTERVAL_SECS - 1));
        assert!(due(ok, 100 + INTERVAL_SECS));
        let failed = Some(Attempt {
            at: 100,
            failed: true,
        });
        assert!(!due(failed, 100 + RETRY_SECS - 1));
        assert!(due(failed, 100 + RETRY_SECS));
        assert!(due(ok, 50), "a clock that went back");
    }

    #[test]
    fn ticks_check_at_launch_then_daily_and_never_while_skipped() {
        let feed = Feed::of(FEED);
        let on = inputs("1.2.0", Some("abc"), None, true);
        let mut last = None;
        let first = tick(&mut last, 10, &on, &feed);
        assert!(matches!(first, Some(Outcome::Available(_))));
        assert_eq!(tick(&mut last, 10 + TICK_SECS, &on, &feed), None);
        assert_eq!(feed.asked().len(), 1, "no second request within a day");
        assert!(tick(&mut last, 10 + INTERVAL_SECS, &on, &feed).is_some());
        assert_eq!(feed.asked().len(), 2);

        let off = inputs("1.2.0", Some("abc"), None, false);
        assert_eq!(
            tick(&mut last, 20 + INTERVAL_SECS, &off, &feed),
            Some(Outcome::Skipped(Skip::Setting))
        );
        assert_eq!(last, None);
        assert_eq!(feed.asked().len(), 2, "no request while off");
        assert!(
            matches!(
                tick(&mut last, 30 + INTERVAL_SECS, &on, &feed),
                Some(Outcome::Available(_))
            ),
            "turning it back on checks at once"
        );
    }

    #[test]
    fn a_failed_tick_retries_within_the_hour() {
        let failing = Feed {
            text: Err("no network".into()),
            asked: Mutex::new(Vec::new()),
        };
        let on = inputs("1.2.0", Some("abc"), None, true);
        let mut last = None;
        assert!(matches!(
            tick(&mut last, 0, &on, &failing),
            Some(Outcome::Failed(_))
        ));
        assert_eq!(tick(&mut last, RETRY_SECS - 1, &on, &failing), None);
        assert!(tick(&mut last, RETRY_SECS, &on, &failing).is_some());
        assert_eq!(failing.asked().len(), 2);
    }

    #[test]
    fn a_release_links_to_its_notes_or_the_releases_page() {
        let feed = Feed::of(FEED);
        let Outcome::Available(release) = check("1.2.0", &feed) else {
            panic!("1.2.1 is newer");
        };
        assert_eq!(
            release.page(),
            "https://github.com/dtthaison/notchling/releases/tag/v1.2.1"
        );
        let bare = Release {
            version: v("2.0.0"),
            critical: false,
            notes: None,
        };
        assert_eq!(bare.page(), RELEASES_URL);
    }

    #[test]
    fn a_stable_app_is_offered_only_stable_releases() {
        let feed = Feed::of(FEED);
        let outcome = check("1.2.0", &feed);
        let Outcome::Available(release) = outcome else {
            panic!("1.2.1 is newer");
        };
        assert_eq!(release.version.to_string(), "1.2.1");
        assert!(!release.critical);
        assert_eq!(
            release.notes.as_deref(),
            Some("https://github.com/dtthaison/notchling/releases/tag/v1.2.1")
        );
        assert_eq!(feed.asked(), vec![FEED_URL.to_owned()]);
        assert_eq!(check("1.2.1", &feed), Outcome::UpToDate(v("1.2.1")));
    }

    #[test]
    fn a_beta_app_sees_betas_too_and_a_critical_one_is_flagged() {
        let feed = Feed::of(FEED);
        assert_eq!(check("1.2.1", &feed), Outcome::UpToDate(v("1.2.1")));
        let outcome = check("1.3.0-beta.1", &feed);
        let Outcome::Available(release) = outcome else {
            panic!("beta.2 is newer");
        };
        assert_eq!(release.version.to_string(), "1.3.0-beta.2");
        assert!(release.critical);
        assert!(Outcome::Available(release).line().contains("critical"));
        assert_eq!(
            check("1.3.0-beta.2", &feed),
            Outcome::UpToDate(v("1.3.0-beta.2"))
        );
    }

    #[test]
    fn an_empty_feed_is_up_to_date_and_a_bad_one_fails() {
        assert_eq!(
            check("0.1.0", &Feed::of(EMPTY)),
            Outcome::UpToDate(v("0.1.0"))
        );
        for bad in ["", "not xml", "<rss><channel>", "<rss/>"] {
            let outcome = check("0.1.0", &Feed::of(bad));
            assert!(
                matches!(outcome, Outcome::Failed(_)),
                "{bad:?}: {outcome:?}"
            );
        }
        let failing = Feed {
            text: Err("no network".into()),
            asked: Mutex::new(Vec::new()),
        };
        assert_eq!(
            check("0.1.0", &failing),
            Outcome::Failed("no network".into())
        );
        assert!(matches!(check("dev", &failing), Outcome::Failed(_)));
    }

    #[test]
    fn an_item_whose_channel_and_version_disagree_is_ignored() {
        let feed = r#"<rss xmlns:sparkle="s"><channel>
          <item><sparkle:shortVersionString>9.0.0</sparkle:shortVersionString><sparkle:channel>beta</sparkle:channel></item>
          <item><sparkle:shortVersionString>9.0.0-beta.1</sparkle:shortVersionString></item>
          <item><sparkle:shortVersionString>junk</sparkle:shortVersionString></item>
        </channel></rss>"#;
        assert_eq!(
            check("1.0.0-beta.1", &Feed::of(feed)),
            Outcome::UpToDate(v("1.0.0-beta.1"))
        );
    }

    #[test]
    fn only_github_https_links_are_kept() {
        assert!(safe_link("https://github.com/dtthaison/notchling/releases/tag/v1.0.0").is_some());
        for bad in [
            "http://github.com/x",
            "https://evil.example/github.com/x",
            "https://github.com.evil.example/x",
            "javascript:alert(1)",
            "https://github.com/",
            "https://github.com/a b",
            "https://github.com/a\"b",
        ] {
            assert!(safe_link(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn the_real_fetcher_is_curl_over_https_and_nothing_else() {
        let launcher = FakeLauncher::new();
        launcher.push_output(Ok(LaunchOutput::ok(FEED)));
        launcher.push_output(Ok(LaunchOutput::failed(22, "")));
        let fetcher = CurlFetcher::new(launcher);
        assert_eq!(fetcher.fetch(FEED_URL).unwrap(), FEED);
        assert!(fetcher.fetch(FEED_URL).is_err());
        assert!(fetcher.fetch("http://example.com/appcast.xml").is_err());
        let commands = fetcher.launcher.commands();
        assert_eq!(commands.len(), 2, "the http address was refused unsent");
        let argv = &commands[0].argv;
        assert_eq!(argv[0], "/usr/bin/curl");
        assert_eq!(argv[1], "-q", "first, so no ~/.curlrc is read");
        assert!(argv.windows(2).any(|w| w == ["--proto", "=https"]));
        assert_eq!(argv.last().map(String::as_str), Some(FEED_URL));
        assert!(!argv
            .iter()
            .any(|a| a == "-o" || a == "--output" || a == "-O"));
    }
}
