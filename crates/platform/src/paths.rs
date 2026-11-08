//! Data, cache and home folders per OS, worked out from the environment.
//!
//! The functions take the OS and a lookup so they can be tested for every OS on any machine.
//! Data folder rule: macOS `~/Library/Application Support/Notchling`, Linux `$XDG_DATA_HOME/notchling`
//! or `~/.local/share/notchling`, Windows `%APPDATA%\Notchling`.

use std::path::PathBuf;

/// The operating systems with a folder rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Linux,
    Windows,
}

impl Os {
    /// The OS this build runs on. Anything that is not macOS or Windows follows the Linux rule.
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// A variable lookup: the real environment, or a fake in tests.
pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The real process environment.
pub fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// A value that is set, non-empty and absolute. The XDG rules ignore relative paths. A leading `/`
/// counts on every host, so a Mac or Linux rule can be tested on Windows.
fn absolute(env: Env, name: &str) -> Option<PathBuf> {
    let value = env(name)?;
    let path = PathBuf::from(&value);
    (!value.is_empty() && (path.is_absolute() || value.starts_with('/'))).then_some(path)
}

/// The user's home folder, or `None` when the environment does not say.
pub fn home_in(os: Os, env: Env) -> Option<PathBuf> {
    let name = if os == Os::Windows {
        "USERPROFILE"
    } else {
        "HOME"
    };
    absolute(env, name)
}

/// Where Notchling keeps its data.
pub fn data_dir_in(os: Os, env: Env) -> Option<PathBuf> {
    match os {
        Os::Mac => Some(home_in(os, env)?.join("Library/Application Support/Notchling")),
        Os::Linux => match absolute(env, "XDG_DATA_HOME") {
            Some(base) => Some(base.join("notchling")),
            None => Some(home_in(os, env)?.join(".local/share/notchling")),
        },
        Os::Windows => Some(absolute(env, "APPDATA")?.join("Notchling")),
    }
}

/// Where Notchling keeps what it can rebuild.
pub fn cache_dir_in(os: Os, env: Env) -> Option<PathBuf> {
    match os {
        Os::Mac => Some(home_in(os, env)?.join("Library/Caches/Notchling")),
        Os::Linux => match absolute(env, "XDG_CACHE_HOME") {
            Some(base) => Some(base.join("notchling")),
            None => Some(home_in(os, env)?.join(".cache/notchling")),
        },
        Os::Windows => Some(
            absolute(env, "LOCALAPPDATA")?
                .join("Notchling")
                .join("Cache"),
        ),
    }
}

/// The home folder of the user running this process.
pub fn home() -> Option<PathBuf> {
    home_in(Os::current(), &process_env)
}

/// This OS's data folder for the real environment.
pub fn data_dir() -> Option<PathBuf> {
    data_dir_in(Os::current(), &process_env)
}

/// This OS's cache folder for the real environment.
pub fn cache_dir() -> Option<PathBuf> {
    cache_dir_in(Os::current(), &process_env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn fake(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| map.get(name).cloned()
    }

    fn join(base: &str, rest: &str) -> PathBuf {
        PathBuf::from(base).join(rest)
    }

    #[test]
    fn mac_data_and_cache_live_under_library() {
        let env = fake(&[("HOME", "/Users/a")]);
        assert_eq!(
            data_dir_in(Os::Mac, &env),
            Some(join("/Users/a", "Library/Application Support/Notchling"))
        );
        assert_eq!(
            cache_dir_in(Os::Mac, &env),
            Some(join("/Users/a", "Library/Caches/Notchling"))
        );
    }

    #[test]
    fn linux_prefers_xdg_then_home() {
        let both = fake(&[
            ("HOME", "/home/a"),
            ("XDG_DATA_HOME", "/x/data"),
            ("XDG_CACHE_HOME", "/x/cache"),
        ]);
        assert_eq!(
            data_dir_in(Os::Linux, &both),
            Some(join("/x/data", "notchling"))
        );
        assert_eq!(
            cache_dir_in(Os::Linux, &both),
            Some(join("/x/cache", "notchling"))
        );
        let home_only = fake(&[("HOME", "/home/a")]);
        assert_eq!(
            data_dir_in(Os::Linux, &home_only),
            Some(join("/home/a", ".local/share/notchling"))
        );
        assert_eq!(
            cache_dir_in(Os::Linux, &home_only),
            Some(join("/home/a", ".cache/notchling"))
        );
    }

    #[test]
    fn linux_ignores_empty_or_relative_xdg() {
        let env = fake(&[
            ("HOME", "/home/a"),
            ("XDG_DATA_HOME", ""),
            ("XDG_CACHE_HOME", "rel/cache"),
        ]);
        assert_eq!(
            data_dir_in(Os::Linux, &env),
            Some(join("/home/a", ".local/share/notchling"))
        );
        assert_eq!(
            cache_dir_in(Os::Linux, &env),
            Some(join("/home/a", ".cache/notchling"))
        );
    }

    #[test]
    fn windows_uses_appdata_and_localappdata() {
        // Bases start with `/` so they count as absolute on every host; joins use the host's rules.
        let env = fake(&[
            ("APPDATA", "/w/roaming"),
            ("LOCALAPPDATA", "/w/local"),
            ("USERPROFILE", "/w/a"),
        ]);
        assert_eq!(
            data_dir_in(Os::Windows, &env),
            Some(join("/w/roaming", "Notchling"))
        );
        assert_eq!(
            cache_dir_in(Os::Windows, &env),
            Some(join("/w/local", "Notchling").join("Cache"))
        );
        assert_eq!(home_in(Os::Windows, &env), Some(PathBuf::from("/w/a")));
    }

    #[test]
    fn missing_environment_gives_none() {
        let env = fake(&[]);
        for os in [Os::Mac, Os::Linux, Os::Windows] {
            assert_eq!(data_dir_in(os, &env), None);
            assert_eq!(cache_dir_in(os, &env), None);
            assert_eq!(home_in(os, &env), None);
        }
    }

    #[test]
    fn home_variable_differs_by_os() {
        let env = fake(&[("HOME", "/home/a")]);
        assert_eq!(home_in(Os::Linux, &env), Some(PathBuf::from("/home/a")));
        assert_eq!(home_in(Os::Windows, &env), None);
    }
}
