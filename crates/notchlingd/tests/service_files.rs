use notchling_notchlingd::service::{file_name, render, ServiceSpec};
use notchling_platform::paths::Os;
use std::path::{Path, PathBuf};
use std::process::Command;

fn golden(os: Os, spec: &ServiceSpec) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(file_name(os));
    let text = render(os, spec);
    if std::env::var_os("NOTCHLING_UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let want = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        text,
        want,
        "{} differs from its golden copy",
        path.display()
    );
}

#[test]
fn launch_agent_plist() {
    golden(
        Os::Mac,
        &ServiceSpec {
            binary: "/Applications/Notchling.app/Contents/MacOS/notchling".into(),
            log: "/Users/me/Library/Application Support/Notchling/notchlingd.log".into(),
        },
    );
}

#[test]
fn systemd_user_unit() {
    golden(
        Os::Linux,
        &ServiceSpec {
            binary: "/home/me/.local/bin/notchling".into(),
            log: "/home/me/.local/share/notchling/notchlingd.log".into(),
        },
    );
}

#[test]
fn task_scheduler_xml() {
    golden(
        Os::Windows,
        &ServiceSpec {
            binary: r"C:\Program Files\Notchling\notchling.exe".into(),
            log: r"C:\Users\me\AppData\Roaming\Notchling\notchlingd.log".into(),
        },
    );
}

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Dir {
    let dir = std::env::temp_dir().join(format!("nl-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Dir(dir)
}

fn notchling(home: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_notchling"));
    c.env("HOME", home)
        .env("NOTCHLING_DATA_ROOT", home.join("root"))
        .env_remove("XDG_CONFIG_HOME");
    c
}

#[test]
fn install_dir_writes_0644_and_prints_the_load_command() {
    let home = scratch("svc");
    let target = home.0.join("agents");
    let out = notchling(&home.0)
        .args(["service", "install", "--dir"])
        .arg(&target)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let file = target.join(file_name(Os::current()));
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("daemon"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(&format!("Wrote {}", file.display())),
        "{stdout}"
    );
    assert!(stdout.contains("Load it with: "), "{stdout}");
    #[cfg(target_os = "macos")]
    assert!(stdout.contains("launchctl bootstrap gui/"), "{stdout}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o644);
    }
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 1);

    let out = notchling(&home.0)
        .args(["service", "uninstall", "--dir"])
        .arg(&target)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!file.exists());
}

#[test]
fn print_writes_nothing() {
    let home = scratch("svc-print");
    let out = notchling(&home.0)
        .args(["service", "print"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("daemon"));
    assert_eq!(std::fs::read_dir(&home.0).unwrap().count(), 0);
}
