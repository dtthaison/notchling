use crate::contract::{self, Failure};
use crate::layout::Layout;
use notchling_engine::delegate::command::{self, Context, RecordTo};
use notchling_engine::delegate::options::DEPTH_VARIABLE;
use notchling_engine::delegate::real::{self, Folders};
use notchling_engine::delegate::record;
use notchling_engine::settings::Settings;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

static SIGNALLED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn on_signal(_: libc::c_int) {
    SIGNALLED.store(true, Ordering::SeqCst);
}

#[cfg(unix)]
fn stop_on_signals(stop: Arc<AtomicBool>) {
    let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
    unsafe {
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGHUP, handler);
    }
    std::thread::spawn(move || loop {
        if SIGNALLED.load(Ordering::SeqCst) {
            stop.store(true, Ordering::SeqCst);
        }
        std::thread::sleep(Duration::from_millis(50));
    });
}

#[cfg(not(unix))]
fn stop_on_signals(_stop: Arc<AtomicBool>) {
    let _ = &SIGNALLED;
}

pub fn folders(
    layout: &Layout,
    real_home: &Path,
    path: String,
    codex_home: Option<&std::ffi::OsStr>,
) -> Folders {
    let network = Settings::open(&layout.data_dir.join("settings.json"))
        .ok()
        .and_then(|s| s.get_bool("codexNetwork"))
        .unwrap_or(true);
    Folders {
        home: real_home.to_path_buf(),
        codex_home: layout.codex_dir_with(codex_home),
        antigravity: layout.antigravity_dir(),
        claude_json: layout.home().join(".claude.json"),
        path,
        network,
        catalog_file: layout.state_dir().join("catalog.json"),
        quota_dir: Some(layout.data_dir.join("daemon").join("accounts")),
    }
}

fn setup(json: bool) -> Option<(Layout, Folders)> {
    let Some(layout) = Layout::from_env(None) else {
        Failure::failed("No home folder in the environment, so nothing to find the tools from.")
            .report(json);
        return None;
    };
    let real_home = notchling_platform::paths::home().unwrap_or_else(|| layout.home());
    let path = std::env::var("PATH").unwrap_or_default();
    let folders = folders(
        &layout,
        &real_home,
        path,
        std::env::var_os("CODEX_HOME").as_deref(),
    );
    Some((layout, folders))
}

pub fn run(args: &[String]) -> i32 {
    if let Some(code) = crate::cluster_dispatch_delegate::run_machine(args) {
        return code;
    }
    let json = contract::wants_json(args);
    let Some((layout, folders)) = setup(json) else {
        return contract::FAILED;
    };
    let Ok(cwd) = std::env::current_dir() else {
        return Failure::failed("Can't tell which folder this runs in.").report(json);
    };
    let raw_depth = std::env::var(DEPTH_VARIABLE).ok();
    if let Ok(deeper) = notchling_engine::delegate::options::enter_depth(raw_depth.as_deref()) {
        std::env::set_var(DEPTH_VARIABLE, deeper.to_string());
    }
    let stop = Arc::new(AtomicBool::new(false));
    stop_on_signals(stop.clone());
    let deps = real::deps(&folders, stop);
    let home: PathBuf = folders.home.clone();
    let parent = std::env::var(record::PARENT_VARIABLE)
        .ok()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty());
    let records = layout.data_dir.join(record::FOLDER);
    let parent_cwd = cwd.to_string_lossy();
    let ctx = Context {
        cwd: &cwd,
        home: Some(&home),
        depth: raw_depth.as_deref(),
        record: parent.as_deref().map(|parent_session| RecordTo {
            dir: &records,
            parent_session,
            parent_cwd: &parent_cwd,
            pid: std::process::id(),
        }),
    };
    let recorded = parent
        .as_ref()
        .map(|_| crate::jobs_cli::delegate_begin(args, &parent_cwd));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = command::run(args, &ctx, deps, &mut out, &mut err);
    if let Some(recorded) = recorded {
        crate::jobs_cli::delegate_end(recorded, code);
    }
    conclude(
        code,
        (&text_of(out), &text_of(err)),
        json,
        (&mut std::io::stdout(), &mut std::io::stderr()),
    )
}

fn text_of(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

fn contract_object(text: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(text.trim()) else {
        return false;
    };
    value.get("error").is_some_and(|error| {
        error.get("exit").is_some_and(Value::is_i64)
            && error.get("code").is_some_and(Value::is_string)
    })
}

fn names_the_code(err: &str, code: i32) -> bool {
    err.trim_end()
        .lines()
        .next_back()
        .is_some_and(|line| line.ends_with(&format!(" ({})", contract::name_of(code))))
}

pub fn conclude(
    code: i32,
    (out, err): (&str, &str),
    json: bool,
    (to_out, to_err): (&mut dyn Write, &mut dyn Write),
) -> i32 {
    if code != contract::OK {
        if json && contract_object(out) {
            let _ = writeln!(to_out, "{}", out.trim());
            return code;
        }
        if !json && names_the_code(err, code) {
            let _ = write!(to_out, "{out}");
            let _ = write!(to_err, "{err}");
            return code;
        }
    }
    match code {
        contract::OK => {
            let _ = write!(to_out, "{out}");
            if !json {
                let _ = write!(to_err, "{err}");
            }
            code
        }
        contract::BAD_REQUEST => {
            let human = [err, out]
                .iter()
                .map(|text| text.trim())
                .find(|text| !text.is_empty())
                .unwrap_or_default();
            Failure::usage(human).emit(json, to_out, to_err)
        }
        contract::FAILED | contract::EXPIRED => {
            let failure = if code == contract::EXPIRED {
                Failure::new(contract::EXPIRED, "expired", "The job ran out of time.")
            } else {
                Failure::failed("The job failed.")
            };
            if json {
                let value = match serde_json::from_str::<Value>(out.trim()) {
                    Ok(outcome) => failure.to_value_with(outcome),
                    Err(_) => failure.to_value(),
                };
                let _ = writeln!(to_out, "{value}");
                return code;
            }
            let _ = write!(to_out, "{out}");
            let _ = write!(to_err, "{err}");
            failure.emit(false, to_out, to_err)
        }
        _ => Failure::failed("The job ended in a way Notchling doesn't know.")
            .emit(json, to_out, to_err),
    }
}

fn listing_failure(args: &[String]) -> Failure {
    if notchling_engine::delegate::role_models::wants_for(args) {
        Failure::failed("No model is picked for that role.")
    } else {
        Failure::failed("The models couldn't be listed.")
    }
}

fn listing(args: &[String], json: bool, deps: &notchling_engine::delegate::deps::Deps) -> i32 {
    let args: Vec<String> = args.iter().filter(|a| *a != "--json").cloned().collect();
    let mut out = Vec::new();
    let code = if json {
        command::models_listing_json(&args, deps, &mut out)
    } else {
        command::models_listing(&args, deps, &mut out)
    };
    let text = String::from_utf8_lossy(&out).trim().to_string();
    match code {
        contract::OK => {
            println!("{text}");
            code
        }
        contract::BAD_REQUEST => Failure::usage(text).report(json),
        _ => {
            let failure = listing_failure(&args).with_human(text.clone());
            if json {
                let value = match serde_json::from_str::<Value>(&text) {
                    Ok(listing) => failure.to_value_with(listing),
                    Err(_) => failure.to_value(),
                };
                println!("{value}");
                return failure.exit;
            }
            failure.report(false)
        }
    }
}

pub fn models(args: &[String]) -> i32 {
    let json = contract::wants_json(args);
    let Some((_, folders)) = setup(json) else {
        return contract::FAILED;
    };
    let deps = real::deps(&folders, Arc::new(AtomicBool::new(false)));
    listing(args, json, &deps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn layout(fixture_root: Option<&str>) -> Layout {
        Layout::resolve(None, fixture_root.map(OsStr::new), || {
            Some((PathBuf::from("/home/me"), PathBuf::from("/data/Notchling")))
        })
        .unwrap()
    }

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_failed_role_lookup_says_no_model_is_picked() {
        for args in [
            words(&["--for", "review-final"]),
            words(&["--for=review-final"]),
        ] {
            assert_eq!(
                listing_failure(&args).message,
                "No model is picked for that role."
            );
        }
    }

    #[test]
    fn any_other_listing_failure_does_not_blame_a_role() {
        for args in [words(&[]), words(&["codex"]), words(&["agy"])] {
            let failure = listing_failure(&args);
            assert_eq!(failure.exit, contract::FAILED);
            assert_eq!(failure.message, "The models couldn't be listed.");
        }
    }

    #[test]
    fn the_real_folders_are_the_users_own() {
        let f = folders(&layout(None), Path::new("/home/me"), "/bin".into(), None);
        assert_eq!(f.home, Path::new("/home/me"));
        assert_eq!(f.codex_home, Path::new("/home/me/.codex"));
        assert_eq!(f.antigravity, Path::new("/home/me/.gemini/antigravity-cli"));
        assert_eq!(f.claude_json, Path::new("/home/me/.claude.json"));
        assert!(f.network, "on unless the settings say off");
        let custom = folders(
            &layout(None),
            Path::new("/home/me"),
            String::new(),
            Some(OsStr::new("/elsewhere/codex")),
        );
        assert_eq!(custom.codex_home, Path::new("/elsewhere/codex"));
    }

    #[test]
    fn a_fixture_root_stands_in_for_the_data_but_not_for_the_tools() {
        let f = folders(
            &layout(Some("/tmp/nl-root")),
            Path::new("/home/me"),
            "/bin".into(),
            None,
        );
        assert_eq!(
            f.home,
            Path::new("/home/me"),
            "the tools are found in the real home"
        );
        assert_eq!(f.codex_home, Path::new("/tmp/nl-root/codex"));
        assert_eq!(
            f.antigravity,
            Path::new("/tmp/nl-root/gemini/antigravity-cli")
        );
        assert_eq!(
            f.claude_json,
            Path::new("/tmp/nl-root/.claude.json"),
            "a fixture never reads the real Claude state"
        );
    }

    #[test]
    fn the_network_follows_the_codex_network_setting() {
        let dir = std::env::temp_dir().join(format!("nl-delegate-cli-{}", std::process::id()));
        let data = dir.join("notchling");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(
            data.join("settings.json"),
            r#"{"version":1,"values":{"codexNetwork":false}}"#,
        )
        .unwrap();
        let root = dir.to_string_lossy().into_owned();
        let f = folders(&layout(Some(&root)), Path::new("/h"), String::new(), None);
        assert!(!f.network);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
