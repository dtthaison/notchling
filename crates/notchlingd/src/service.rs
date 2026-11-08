//! The service file per OS, as pure functions, and writing it into a folder.
//!
//! - macOS: a LaunchAgent plist, label `dev.notchling.daemon`, `RunAtLoad`, `KeepAlive` on a crash,
//!   output to `notchlingd.log` in the data folder.
//! - Linux: a systemd user unit, restarted on failure.
//! - Windows: a Task Scheduler XML file, started at logon.
//!
//! `install` writes the file (0644) into a folder (the OS's real place unless `--dir` names one)
//! and prints the command that loads it. Nothing here runs `launchctl`, `systemctl` or `schtasks`.

use notchling_platform::paths::{home_in, Env, Os};
use std::io;
use std::path::{Path, PathBuf};

/// The LaunchAgent's label.
pub const LABEL: &str = "dev.notchling.daemon";
/// The Task Scheduler task's path.
pub const TASK_NAME: &str = r"\Notchling\notchlingd";

/// What a service file is made from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceSpec {
    /// The `notchling` binary; the service runs `<binary> daemon`.
    pub binary: PathBuf,
    /// Where its output goes (macOS and Linux).
    pub log: PathBuf,
}

/// The service file's name.
pub fn file_name(os: Os) -> &'static str {
    match os {
        Os::Mac => "dev.notchling.daemon.plist",
        Os::Linux => "notchlingd.service",
        Os::Windows => "notchlingd.xml",
    }
}

/// The service file's text.
pub fn render(os: Os, spec: &ServiceSpec) -> String {
    match os {
        Os::Mac => launchd_plist(spec),
        Os::Linux => systemd_unit(spec),
        Os::Windows => task_xml(spec),
    }
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn launchd_plist(spec: &ServiceSpec) -> String {
    let binary = xml(&spec.binary.to_string_lossy());
    let log = xml(&spec.log.to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{binary}</string>
		<string>daemon</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<dict>
		<key>Crashed</key>
		<true/>
	</dict>
	<key>ProcessType</key>
	<string>Background</string>
	<key>StandardOutPath</key>
	<string>{log}</string>
	<key>StandardErrorPath</key>
	<string>{log}</string>
</dict>
</plist>
"#
    )
}

/// systemd's quoting: the path in double quotes, with `\`, `"` and `%` escaped.
fn systemd_quote(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    format!("\"{escaped}\"")
}

fn systemd_unit(spec: &ServiceSpec) -> String {
    let binary = systemd_quote(&spec.binary.to_string_lossy());
    let log = spec.log.to_string_lossy().replace('%', "%%");
    format!(
        "[Unit]\n\
         Description=Notchling daemon\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={binary} daemon\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         StandardOutput=append:{log}\n\
         StandardError=append:{log}\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

fn task_xml(spec: &ServiceSpec) -> String {
    let binary = xml(&spec.binary.to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Notchling daemon</Description>
    <URI>{TASK_NAME}</URI>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>3</Count>
    </RestartOnFailure>
    <Hidden>true</Hidden>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{binary}</Command>
      <Arguments>daemon</Arguments>
    </Exec>
  </Actions>
</Task>
"#
    )
}

/// The OS's real place for the file: `~/Library/LaunchAgents`, `$XDG_CONFIG_HOME/systemd/user`
/// (else `~/.config/systemd/user`), `%APPDATA%\Notchling`.
pub fn default_dir(os: Os, env: Env) -> Option<PathBuf> {
    match os {
        Os::Mac => Some(home_in(os, env)?.join("Library/LaunchAgents")),
        Os::Linux => match env("XDG_CONFIG_HOME").filter(|v| v.starts_with('/')) {
            Some(base) => Some(PathBuf::from(base).join("systemd/user")),
            None => Some(home_in(os, env)?.join(".config/systemd/user")),
        },
        Os::Windows => notchling_platform::paths::data_dir_in(os, env),
    }
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

/// The command a person runs to load an installed file.
pub fn load_command(os: Os, file: &Path, uid: u32) -> String {
    match os {
        Os::Mac => format!("launchctl bootstrap gui/{uid} {}", shell_quote(file)),
        Os::Linux => {
            "systemctl --user daemon-reload && systemctl --user enable --now notchlingd.service"
                .into()
        }
        Os::Windows => format!(
            r#"schtasks /Create /TN "{TASK_NAME}" /XML "{}""#,
            file.display()
        ),
    }
}

/// The command a person runs to unload it.
pub fn unload_command(os: Os, file: &Path, uid: u32) -> String {
    match os {
        Os::Mac => format!("launchctl bootout gui/{uid} {}", shell_quote(file)),
        Os::Linux => "systemctl --user disable --now notchlingd.service".into(),
        Os::Windows => {
            let _ = file;
            format!(r#"schtasks /Delete /TN "{TASK_NAME}" /F"#)
        }
    }
}

/// Writes the service file into `dir` (made if missing), mode 0644, replacing an old one.
pub fn install(os: Os, spec: &ServiceSpec, dir: &Path) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(file_name(os));
    let temp = dir.join(format!(".{}.{}.tmp", file_name(os), std::process::id()));
    let result = (|| {
        std::fs::write(&temp, render(os, spec))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o644))?;
        }
        std::fs::rename(&temp, &path)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    Ok(path)
}

/// Removes the service file from `dir`; `None` when there was none.
pub fn uninstall(os: Os, dir: &Path) -> io::Result<Option<PathBuf>> {
    let path = dir.join(file_name(os));
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(Some(path)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| map.get(k).cloned()
    }

    #[test]
    fn the_real_places() {
        let e = env(&[("HOME", "/Users/me")]);
        assert_eq!(
            default_dir(Os::Mac, &e),
            Some(PathBuf::from("/Users/me/Library/LaunchAgents"))
        );
        assert_eq!(
            default_dir(Os::Linux, &env(&[("HOME", "/home/me")])),
            Some(PathBuf::from("/home/me/.config/systemd/user"))
        );
        assert_eq!(
            default_dir(
                Os::Linux,
                &env(&[("HOME", "/home/me"), ("XDG_CONFIG_HOME", "/c")])
            ),
            Some(PathBuf::from("/c/systemd/user"))
        );
        assert_eq!(
            default_dir(Os::Windows, &env(&[("APPDATA", "/w/roaming")])),
            Some(PathBuf::from("/w/roaming").join("Notchling"))
        );
    }

    #[test]
    fn load_commands_name_the_file() {
        let f = Path::new("/d/dev.notchling.daemon.plist");
        assert_eq!(
            load_command(Os::Mac, f, 501),
            "launchctl bootstrap gui/501 '/d/dev.notchling.daemon.plist'"
        );
        assert!(unload_command(Os::Mac, f, 501).starts_with("launchctl bootout gui/501"));
        assert!(load_command(Os::Linux, f, 0).contains("enable --now notchlingd.service"));
        assert!(load_command(Os::Windows, Path::new(r"C:\x.xml"), 0).contains("/XML"));
    }

    #[test]
    fn paths_are_escaped() {
        let spec = ServiceSpec {
            binary: "/a&b/<n>/notchling".into(),
            log: "/l/100%/log".into(),
        };
        assert!(render(Os::Mac, &spec).contains("/a&amp;b/&lt;n&gt;/notchling"));
        assert!(render(Os::Windows, &spec).contains("/a&amp;b/&lt;n&gt;/notchling"));
        let unit = render(Os::Linux, &spec);
        assert!(unit.contains("StandardOutput=append:/l/100%%/log"));
        assert_eq!(systemd_quote(r#"/x "y"/z"#), r#""/x \"y\"/z""#);
    }
}
