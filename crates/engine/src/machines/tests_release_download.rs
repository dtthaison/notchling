use super::dist_manifest;
use super::fakes_b::*;
use super::node_updates::{offer, MemberBuild, UpdateState};
use super::registry::Machine;
use super::release_download::{release_url, release_version, CurlDownloads, ReleaseDownloads};
use super::runner::{Channel, CommandResult, ScriptedRunner};
use super::step::{CheckState, FixContext, RemoteOutput, StepId};
use super::steps_node_binary::{binary_fix_on, binary_state_on};
use super::steps_node_mac::{Host, BUILD_LINUX};
use super::tests_node_steps::{fix_of, hex, node_linux};
use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};

const BUILD: &str = "abc1234";
const VERSION: &str = "2.0.0";
const FILE: &str = "notchling-linux-x86_64";
const OTHER_FILE: &str = "notchling-linux-aarch64";
const BINARY: &[u8] = b"a linux notchling from the release";

fn host(build: Option<&'static str>, release: Option<&'static str>) -> Host<'static> {
    Host {
        arch: "aarch64",
        build,
        release,
    }
}

fn can() -> Host<'static> {
    host(Some(BUILD), Some(VERSION))
}

fn manifest_text(build: &str, files: &[(&str, &[u8])]) -> String {
    let list: Vec<String> = files
        .iter()
        .map(|(name, bytes)| {
            format!(
                "\"{name}\":{{\"sha256\":\"{}\",\"size\":{}}}",
                hex(bytes),
                bytes.len()
            )
        })
        .collect();
    format!("{{\"build\":\"{build}\",\"files\":{{{}}}}}", list.join(","))
}

fn released(downloads: &FakeDownloads, build: &str, bytes: &[u8]) {
    downloads.serve(
        &release_url(VERSION, "manifest.json"),
        manifest_text(build, &[(FILE, BINARY)]).as_bytes(),
    );
    downloads.serve(&release_url(VERSION, FILE), bytes);
}

struct Fixture {
    rig: Rig,
    dist: TempDir,
    downloads: Arc<FakeDownloads>,
}

impl Fixture {
    fn new() -> Fixture {
        let rig = Rig::new(node_linux());
        rig.runner.on(
            |a| a.last().is_some_and(|s| s.contains("sha256sum")),
            CommandResult::ok(&format!("{}  notchling.new\n", hex(BINARY))),
        );
        Fixture {
            rig,
            dist: TempDir::new("release-dist"),
            downloads: Arc::new(FakeDownloads::default()),
        }
    }

    fn dir(&self) -> std::path::PathBuf {
        self.dist.path().join("dist")
    }

    fn ctx(&self, log: &Arc<Mutex<String>>) -> FixContext {
        let mut ctx = fix_of(&self.rig, Some(&self.dir()), None);
        ctx.step.downloads = self.downloads.clone();
        let log = log.clone();
        ctx.log = Arc::new(move |_: Channel, text: &str| log.lock().unwrap().push_str(text));
        ctx
    }

    fn names(&self) -> Vec<String> {
        let Ok(read) = std::fs::read_dir(self.dir()) else {
            return Vec::new();
        };
        let mut names: Vec<String> = read
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

#[test]
fn a_download_needs_a_build_id_that_is_not_dirty_and_a_plain_version() {
    assert_eq!(release_version("2.0.0", Some("abc1234")), Some("2.0.0"));
    assert_eq!(
        release_version("0.1.0", Some("abc1234def56")),
        Some("0.1.0")
    );
    assert_eq!(release_version("2.0.0", None), None);
    assert_eq!(release_version("2.0.0", Some("abc1234-dirty")), None);
    assert_eq!(release_version("2.0.0", Some("not a build")), None);
    for version in [
        "2.0.0-beta.1",
        "2.0",
        "2.0.0.1",
        "2..0",
        "v2.0.0",
        "",
        "2.0.x",
        "2.0.0+1",
    ] {
        assert_eq!(release_version(version, Some("abc1234")), None, "{version}");
    }
}

#[test]
fn the_fix_downloads_when_the_dist_folder_is_empty_then_copies_as_before() {
    let f = Fixture::new();
    released(&f.downloads, BUILD, BINARY);
    let log = Arc::new(Mutex::new(String::new()));
    let out = binary_fix_on(&f.ctx(&log), can());
    assert!(out.ok, "{}", out.message);
    assert_eq!(
        f.downloads.urls(),
        [
            "https://github.com/dtthaison/notchling/releases/download/v2.0.0/manifest.json",
            "https://github.com/dtthaison/notchling/releases/download/v2.0.0/notchling-linux-x86_64"
        ]
    );
    let file = f.dir().join(FILE);
    assert_eq!(std::fs::read(&file).unwrap(), BINARY);
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o755
    );
    let written = dist_manifest::read(&f.dir()).unwrap();
    assert_eq!(written.build, BUILD);
    assert_eq!(written.files[FILE].sha256, hex(BINARY));
    assert_eq!(f.names(), ["manifest.json", FILE]);
    assert!(log
        .lock()
        .unwrap()
        .contains("Downloading notchling-linux-x86_64 from the 2.0.0 release"));
    let seen: Vec<String> = f.rig.argvs().iter().map(|a| a.join(" ")).collect();
    assert!(seen.iter().any(|t| t.contains("/usr/bin/scp")), "{seen:?}");
    assert!(seen.iter().any(|t| t.contains("mv -f")), "{seen:?}");
}

#[test]
fn a_release_built_from_another_commit_downloads_no_binary() {
    let f = Fixture::new();
    released(&f.downloads, "def5678", BINARY);
    let out = binary_fix_on(&f.ctx(&Arc::default()), can());
    assert!(!out.ok);
    assert!(out.message.ends_with(BUILD_LINUX), "{}", out.message);
    assert!(out.message.contains("another commit"), "{}", out.message);
    assert_eq!(f.downloads.urls().len(), 1);
    assert_eq!(f.names(), Vec::<String>::new());
    assert!(f.rig.calls().is_empty());
}

#[test]
fn a_release_without_the_file_or_a_manifest_fails_with_the_advice() {
    let f = Fixture::new();
    let out = binary_fix_on(&f.ctx(&Arc::default()), can());
    assert!(!out.ok);
    assert!(out.message.ends_with(BUILD_LINUX), "{}", out.message);
    assert!(out.message.contains("HTTP 404"), "{}", out.message);
    f.downloads.serve(
        &release_url(VERSION, "manifest.json"),
        manifest_text(BUILD, &[(OTHER_FILE, BINARY)]).as_bytes(),
    );
    let out = binary_fix_on(&f.ctx(&Arc::default()), can());
    assert!(
        out.message.contains("no notchling-linux-x86_64"),
        "{}",
        out.message
    );
    assert_eq!(f.names(), Vec::<String>::new());
}

#[test]
fn a_wrong_hash_or_size_fails_leaves_no_temp_file_and_keeps_the_old_file() {
    for served in [
        &b"a different file of other length"[..],
        &b"a linux notchling from the releasE"[..],
    ] {
        let f = Fixture::new();
        std::fs::create_dir_all(f.dir()).unwrap();
        std::fs::write(f.dir().join(FILE), b"old").unwrap();
        released(&f.downloads, BUILD, served);
        let out = binary_fix_on(&f.ctx(&Arc::default()), can());
        assert!(!out.ok, "{}", out.message);
        assert!(out.message.ends_with(BUILD_LINUX), "{}", out.message);
        assert_eq!(std::fs::read(f.dir().join(FILE)).unwrap(), b"old");
        assert_eq!(f.names(), [FILE], "no temp file and no manifest");
        assert!(f.rig.calls().is_empty());
    }
}

#[test]
fn a_dist_manifest_for_the_same_build_keeps_its_other_entries() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.dir()).unwrap();
    let other = b"the arm build";
    std::fs::write(f.dir().join(OTHER_FILE), other).unwrap();
    std::fs::write(
        f.dir().join("manifest.json"),
        manifest_text(BUILD, &[(OTHER_FILE, other)]),
    )
    .unwrap();
    released(&f.downloads, BUILD, BINARY);
    let out = binary_fix_on(&f.ctx(&Arc::default()), can());
    assert!(out.ok, "{}", out.message);
    let written = dist_manifest::read(&f.dir()).unwrap();
    assert_eq!(written.files.len(), 2);
    assert_eq!(written.files[OTHER_FILE].sha256, hex(other));
    assert_eq!(written.files[FILE].sha256, hex(BINARY));
}

#[test]
fn a_dist_manifest_for_another_build_is_replaced_by_the_releases() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.dir()).unwrap();
    std::fs::write(
        f.dir().join("manifest.json"),
        manifest_text("def5678", &[(OTHER_FILE, b"x")]),
    )
    .unwrap();
    released(&f.downloads, BUILD, BINARY);
    let out = binary_fix_on(&f.ctx(&Arc::default()), can());
    assert!(out.ok, "{}", out.message);
    let written = dist_manifest::read(&f.dir()).unwrap();
    assert_eq!(written.build, BUILD);
    assert_eq!(written.files.keys().collect::<Vec<_>>(), [FILE]);
}

#[test]
fn a_matching_dist_file_downloads_nothing() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.dir()).unwrap();
    std::fs::write(f.dir().join(FILE), BINARY).unwrap();
    std::fs::write(
        f.dir().join("manifest.json"),
        manifest_text(BUILD, &[(FILE, BINARY)]),
    )
    .unwrap();
    let out = binary_fix_on(&f.ctx(&Arc::default()), can());
    assert!(out.ok, "{}", out.message);
    assert!(f.downloads.urls().is_empty());
}

#[test]
fn without_a_build_id_or_a_release_the_messages_are_today_s() {
    let f = Fixture::new();
    released(&f.downloads, BUILD, BINARY);
    let log = Arc::new(Mutex::new(String::new()));
    let none = binary_fix_on(&f.ctx(&log), host(None, None));
    assert!(!none.ok);
    assert!(
        none.message
            .starts_with("Put a notchling build for linux-x86_64 in "),
        "{}",
        none.message
    );
    let dirty = binary_fix_on(&f.ctx(&log), host(Some(BUILD), None));
    assert_eq!(dirty.message, BUILD_LINUX);
    assert!(f.downloads.urls().is_empty());
    assert!(log.lock().unwrap().is_empty());
    assert_eq!(f.names(), Vec::<String>::new());
}

fn state(f: &Fixture, host: Host) -> (CheckState, String) {
    let mut ctx = f.rig.step();
    ctx.dist_dir = Some(f.dir());
    let out = RemoteOutput {
        text: format!("notchling 0.1.0\nsha256={}\n", hex(b"installed")),
        code: 0,
    };
    let got = binary_state_on(&out, &ctx, host);
    (got.state.clone(), format!("{:?}", got.state))
}

#[test]
fn the_probe_says_fix_downloads_it_when_a_download_is_possible_and_never_fetches() {
    let f = Fixture::new();
    released(&f.downloads, BUILD, BINARY);
    let (got, text) = state(&f, can());
    assert!(matches!(got, CheckState::Fail(_)), "{text}");
    assert!(
        text.contains("No Linux build of Notchling 2.0.0 in this Mac's dist folder yet; Fix downloads it from the 2.0.0 release."),
        "{text}"
    );
    assert!(!text.contains("build-node"), "{text}");
    let (got, text) = state(&f, host(Some(BUILD), None));
    assert!(matches!(got, CheckState::Fail(_)), "{text}");
    assert!(text.contains(BUILD_LINUX), "{text}");
    assert!(f.downloads.urls().is_empty());
    let _ = StepId::NODE_BINARY;
}

fn member() -> MemberBuild {
    MemberBuild {
        node: "node-vps".into(),
        name: "vps".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        build: Some("def5678".into()),
        running_shell_jobs: 0,
        moving: false,
    }
}

fn machines() -> Vec<Machine> {
    vec![linux("vps")]
}

#[test]
fn an_update_row_for_a_downloadable_file_is_ready_not_needs_build() {
    let none = BTreeSet::new();
    let members = [member()];
    let ready = offer(Some(BUILD), &members, &machines(), None, &none, true);
    assert_eq!(ready[0].state, UpdateState::Ready);
    let stuck = offer(Some(BUILD), &members, &machines(), None, &none, false);
    assert!(matches!(stuck[0].state, UpdateState::NeedsBuild(_)));
}

#[test]
fn curl_runs_quiet_https_only_with_caps_and_refuses_other_addresses() {
    let runner = Arc::new(ScriptedRunner::new());
    let curl = CurlDownloads::new(runner.clone());
    let dest = Path::new("/tmp/x.download");
    assert!(curl.fetch("http://example.com/a", dest, 10).is_err());
    assert!(runner.calls().is_empty());
    curl.fetch(&release_url(VERSION, FILE), dest, 1234).unwrap();
    let argv = runner.calls()[0].argv.clone();
    assert_eq!(&argv[..2], ["/usr/bin/curl", "-q"]);
    assert!(argv.windows(2).any(|w| w == ["--proto", "=https"]));
    assert!(argv.windows(2).any(|w| w == ["--proto-redir", "=https"]));
    assert!(argv.windows(2).any(|w| w == ["--max-filesize", "1234"]));
    assert!(argv.windows(2).any(|w| w == ["--max-time", "600"]));
    assert!(argv.windows(2).any(|w| w == ["-o", "/tmp/x.download"]));
    assert!(argv.contains(&"--fail".to_string()));
    assert_eq!(argv.last().unwrap(), &release_url(VERSION, FILE));
    runner.on(
        |_| true,
        CommandResult::new(22, "", "curl: (22) The requested URL returned error: 404\n"),
    );
    let why = curl
        .fetch(&release_url(VERSION, FILE), dest, 1)
        .unwrap_err();
    assert!(why.contains("404"), "{why}");
}
