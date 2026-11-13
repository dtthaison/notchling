use std::cmp::Ordering;
use std::time::Duration;

use notchling_engine::control::{Command, Launcher};

use super::xml::{self, Node};

pub const FEED_URL: &str =
    "https://github.com/dtthaison/notchling/releases/download/appcast/appcast.xml";
pub const KILL_SWITCH: &str = "NOTCHLING_RELEASES";
pub const OFF_MESSAGE: &str =
    "Updates are off. Notchling checks for updates only once the maintainer turns releases on.";

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Off,
    UpToDate(Version),
    Available(Release),
    Failed(String),
}

impl Outcome {
    pub fn line(&self) -> String {
        match self {
            Outcome::Off => OFF_MESSAGE.to_owned(),
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

pub fn releases_on(value: Option<&str>) -> bool {
    value == Some("on")
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

pub fn check(current: &str, switch: Option<&str>, fetcher: &dyn FeedFetcher) -> Outcome {
    if !releases_on(switch) {
        return Outcome::Off;
    }
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

    #[test]
    fn the_kill_switch_is_off_unless_exactly_on() {
        for off in [
            None,
            Some(""),
            Some("ON"),
            Some("true"),
            Some("1"),
            Some("on "),
        ] {
            assert!(!releases_on(off), "{off:?}");
        }
        assert!(releases_on(Some("on")));
    }

    #[test]
    fn while_off_nothing_is_requested() {
        let feed = Feed::of(FEED);
        for switch in [None, Some("off"), Some("")] {
            let outcome = check("1.0.0", switch, &feed);
            assert_eq!(outcome, Outcome::Off);
            assert_eq!(outcome.line(), OFF_MESSAGE);
        }
        assert!(feed.asked().is_empty(), "no request while off");
    }

    #[test]
    fn a_stable_app_is_offered_only_stable_releases() {
        let feed = Feed::of(FEED);
        let outcome = check("1.2.0", Some("on"), &feed);
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
        assert_eq!(
            check("1.2.1", Some("on"), &feed),
            Outcome::UpToDate(v("1.2.1"))
        );
    }

    #[test]
    fn a_beta_app_sees_betas_too_and_a_critical_one_is_flagged() {
        let feed = Feed::of(FEED);
        assert_eq!(
            check("1.2.1", Some("on"), &feed),
            Outcome::UpToDate(v("1.2.1"))
        );
        let outcome = check("1.3.0-beta.1", Some("on"), &feed);
        let Outcome::Available(release) = outcome else {
            panic!("beta.2 is newer");
        };
        assert_eq!(release.version.to_string(), "1.3.0-beta.2");
        assert!(release.critical);
        assert!(Outcome::Available(release).line().contains("critical"));
        assert_eq!(
            check("1.3.0-beta.2", Some("on"), &feed),
            Outcome::UpToDate(v("1.3.0-beta.2"))
        );
    }

    #[test]
    fn an_empty_feed_is_up_to_date_and_a_bad_one_fails() {
        assert_eq!(
            check("0.1.0", Some("on"), &Feed::of(EMPTY)),
            Outcome::UpToDate(v("0.1.0"))
        );
        for bad in ["", "not xml", "<rss><channel>", "<rss/>"] {
            let outcome = check("0.1.0", Some("on"), &Feed::of(bad));
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
            check("0.1.0", Some("on"), &failing),
            Outcome::Failed("no network".into())
        );
        assert!(matches!(
            check("dev", Some("on"), &failing),
            Outcome::Failed(_)
        ));
    }

    #[test]
    fn an_item_whose_channel_and_version_disagree_is_ignored() {
        let feed = r#"<rss xmlns:sparkle="s"><channel>
          <item><sparkle:shortVersionString>9.0.0</sparkle:shortVersionString><sparkle:channel>beta</sparkle:channel></item>
          <item><sparkle:shortVersionString>9.0.0-beta.1</sparkle:shortVersionString></item>
          <item><sparkle:shortVersionString>junk</sparkle:shortVersionString></item>
        </channel></rss>"#;
        assert_eq!(
            check("1.0.0-beta.1", Some("on"), &Feed::of(feed)),
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
