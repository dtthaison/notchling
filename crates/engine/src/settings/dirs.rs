//! Data folders per OS, as a pure function of the environment.

use std::path::PathBuf;

/// The operating systems the app runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Linux,
    Windows,
}

impl Os {
    /// The OS this build runs on (anything that is neither macOS nor Windows is treated as Linux).
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// The environment a data folder is computed from. Build it with [`DirEnv::from_process`] or fill it
/// in for a test.
#[derive(Clone, Debug, Default)]
pub struct DirEnv {
    pub os: Option<Os>,
    /// `$HOME` (`%USERPROFILE%` on Windows).
    pub home: Option<PathBuf>,
    /// `$XDG_DATA_HOME`.
    pub xdg_data_home: Option<PathBuf>,
    /// `%APPDATA%`.
    pub appdata: Option<PathBuf>,
}

impl DirEnv {
    /// Reads the real environment. The only impure function in this module.
    pub fn from_process() -> DirEnv {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        let os = Os::current();
        DirEnv {
            os: Some(os),
            home: if os == Os::Windows { var("USERPROFILE").or_else(|| var("HOME")) } else { var("HOME") },
            xdg_data_home: var("XDG_DATA_HOME"),
            appdata: var("APPDATA"),
        }
    }
}

/// Where Notchling keeps its files (`settings.json`, `lanes.json`, `pages.json`, `vault.json`):
///
/// | OS | Folder |
/// | --- | --- |
/// | macOS | `~/Library/Application Support/Notchling` |
/// | Linux | `$XDG_DATA_HOME/notchling`, else `~/.local/share/notchling` |
/// | Windows | `%APPDATA%\Notchling` |
///
/// `None` when the environment lacks what the OS needs. A relative `XDG_DATA_HOME` is ignored, as the
/// XDG spec says.
pub fn data_dir(env: &DirEnv) -> Option<PathBuf> {
    match env.os? {
        Os::MacOs => Some(env.home.as_ref()?.join("Library/Application Support/Notchling")),
        Os::Linux => match env.xdg_data_home.as_ref().filter(|p| p.is_absolute()) {
            Some(xdg) => Some(xdg.join("notchling")),
            None => Some(env.home.as_ref()?.join(".local/share/notchling")),
        },
        Os::Windows => match env.appdata.as_ref() {
            Some(appdata) => Some(appdata.join("Notchling")),
            None => Some(env.home.as_ref()?.join("AppData/Roaming/Notchling")),
        },
    }
}

/// `settings.json` inside [`data_dir`].
pub fn settings_file(env: &DirEnv) -> Option<PathBuf> {
    data_dir(env).map(|d| d.join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(os: Os) -> DirEnv {
        DirEnv { os: Some(os), home: Some("/home/me".into()), ..DirEnv::default() }
    }

    #[test]
    fn macos_uses_application_support() {
        assert_eq!(
            data_dir(&env(Os::MacOs)),
            Some(PathBuf::from("/home/me/Library/Application Support/Notchling"))
        );
    }

    #[test]
    fn linux_prefers_xdg_then_local_share() {
        assert_eq!(data_dir(&env(Os::Linux)), Some(PathBuf::from("/home/me/.local/share/notchling")));
        let mut with_xdg = env(Os::Linux);
        with_xdg.xdg_data_home = Some("/data".into());
        assert_eq!(data_dir(&with_xdg), Some(PathBuf::from("/data/notchling")));
        with_xdg.xdg_data_home = Some("relative/dir".into());
        assert_eq!(data_dir(&with_xdg), Some(PathBuf::from("/home/me/.local/share/notchling")));
    }

    #[test]
    fn windows_uses_appdata() {
        let mut e = env(Os::Windows);
        e.appdata = Some(PathBuf::from("/Users/me/AppData/Roaming"));
        assert_eq!(data_dir(&e), Some(PathBuf::from("/Users/me/AppData/Roaming/Notchling")));
        e.appdata = None;
        assert_eq!(data_dir(&e), Some(PathBuf::from("/home/me/AppData/Roaming/Notchling")));
    }

    #[test]
    fn missing_environment_gives_none() {
        assert_eq!(data_dir(&DirEnv::default()), None);
        let no_home = DirEnv { os: Some(Os::MacOs), ..DirEnv::default() };
        assert_eq!(data_dir(&no_home), None);
        assert_eq!(
            settings_file(&env(Os::Linux)),
            Some(PathBuf::from("/home/me/.local/share/notchling/settings.json"))
        );
    }
}
