use notchling_platform::paths::{data_dir_in, process_env};
use std::path::PathBuf;

pub use notchling_platform::paths::Os;

#[derive(Clone, Debug, Default)]
pub struct DirEnv {
    pub os: Option<Os>,
    pub home: Option<PathBuf>,
    pub xdg_data_home: Option<PathBuf>,
    pub appdata: Option<PathBuf>,
}

impl DirEnv {
    pub fn from_process() -> DirEnv {
        let var = |name: &str| process_env(name).map(PathBuf::from);
        let os = Os::current();
        DirEnv {
            os: Some(os),
            home: var(if os == Os::Windows {
                "USERPROFILE"
            } else {
                "HOME"
            }),
            xdg_data_home: var("XDG_DATA_HOME"),
            appdata: var("APPDATA"),
        }
    }
}

pub fn data_dir(env: &DirEnv) -> Option<PathBuf> {
    let lookup = |name: &str| -> Option<String> {
        let value = match name {
            "HOME" | "USERPROFILE" => &env.home,
            "XDG_DATA_HOME" => &env.xdg_data_home,
            "APPDATA" => &env.appdata,
            _ => &None,
        };
        value.as_ref().map(|p| p.to_string_lossy().into_owned())
    };
    data_dir_in(env.os?, &lookup)
}

pub fn settings_file(env: &DirEnv) -> Option<PathBuf> {
    data_dir(env).map(|d| d.join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(os: Os) -> DirEnv {
        DirEnv {
            os: Some(os),
            home: Some("/home/me".into()),
            ..DirEnv::default()
        }
    }

    #[test]
    fn macos_uses_application_support() {
        assert_eq!(
            data_dir(&env(Os::Mac)),
            Some(PathBuf::from(
                "/home/me/Library/Application Support/Notchling"
            ))
        );
    }

    #[test]
    fn linux_prefers_xdg_then_local_share() {
        assert_eq!(
            data_dir(&env(Os::Linux)),
            Some(PathBuf::from("/home/me/.local/share/notchling"))
        );
        let mut with_xdg = env(Os::Linux);
        with_xdg.xdg_data_home = Some("/data".into());
        assert_eq!(data_dir(&with_xdg), Some(PathBuf::from("/data/notchling")));
        with_xdg.xdg_data_home = Some("relative/dir".into());
        assert_eq!(
            data_dir(&with_xdg),
            Some(PathBuf::from("/home/me/.local/share/notchling"))
        );
    }

    #[test]
    fn windows_uses_appdata() {
        let mut e = env(Os::Windows);
        e.appdata = Some(PathBuf::from("/Users/me/AppData/Roaming"));
        assert_eq!(
            data_dir(&e),
            Some(PathBuf::from("/Users/me/AppData/Roaming/Notchling"))
        );
        e.appdata = None;
        assert_eq!(data_dir(&e), None);
    }

    #[test]
    fn a_relative_home_is_not_a_home() {
        for os in [Os::Mac, Os::Linux, Os::Windows] {
            let mut e = env(os);
            e.home = Some("relative/home".into());
            e.appdata = None;
            assert_eq!(data_dir(&e), None, "{os:?}");
        }
    }

    #[test]
    fn windows_without_appdata_has_no_data_folder() {
        assert_eq!(data_dir(&env(Os::Windows)), None);
    }

    #[test]
    fn it_is_the_platform_rule() {
        let values: [Option<&str>; 4] = [None, Some(""), Some("rel/x"), Some("/abs/x")];
        for os in [Os::Mac, Os::Linux, Os::Windows] {
            for home in values {
                for xdg in values {
                    for appdata in values {
                        let e = DirEnv {
                            os: Some(os),
                            home: home.map(PathBuf::from),
                            xdg_data_home: xdg.map(PathBuf::from),
                            appdata: appdata.map(PathBuf::from),
                        };
                        let lookup = |name: &str| {
                            match name {
                                "HOME" | "USERPROFILE" => home,
                                "XDG_DATA_HOME" => xdg,
                                "APPDATA" => appdata,
                                _ => None,
                            }
                            .map(String::from)
                        };
                        assert_eq!(
                            data_dir(&e),
                            notchling_platform::paths::data_dir_in(os, &lookup),
                            "{os:?} {home:?} {xdg:?} {appdata:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn missing_environment_gives_none() {
        assert_eq!(data_dir(&DirEnv::default()), None);
        let no_home = DirEnv {
            os: Some(Os::Mac),
            ..DirEnv::default()
        };
        assert_eq!(data_dir(&no_home), None);
        assert_eq!(
            settings_file(&env(Os::Linux)),
            Some(PathBuf::from(
                "/home/me/.local/share/notchling/settings.json"
            ))
        );
    }
}
