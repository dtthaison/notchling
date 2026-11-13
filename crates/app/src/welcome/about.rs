use super::updates::Version;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    pub title: &'static str,
    pub url: &'static str,
}

pub const LINKS: [Link; 4] = [
    Link {
        title: "Source code",
        url: "https://github.com/dtthaison/notchling",
    },
    Link {
        title: "License",
        url: "https://github.com/dtthaison/notchling/blob/main/LICENSE",
    },
    Link {
        title: "Releases",
        url: "https://github.com/dtthaison/notchling/releases",
    },
    Link {
        title: "Report a problem",
        url: "https://github.com/dtthaison/notchling/issues",
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct About {
    pub name: &'static str,
    pub version: String,
    pub channel: &'static str,
    pub build: String,
    pub license: &'static str,
    pub tagline: &'static str,
    pub links: Vec<Link>,
}

pub fn about(version: &str, commit: Option<&str>, release_build: bool, system: &str) -> About {
    let beta = Version::parse(version).is_some_and(|v| v.is_beta());
    let commit = commit.filter(|c| !c.is_empty()).unwrap_or("dev");
    About {
        name: "Notchling",
        version: version.to_owned(),
        channel: if beta { "Beta" } else { "Stable" },
        build: format!(
            "{commit}, {}, {system}",
            if release_build { "release" } else { "debug" }
        ),
        license: "MIT",
        tagline: "Your agents, in the camera notch.",
        links: LINKS.to_vec(),
    }
}

pub fn current() -> About {
    let system = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    about(
        env!("CARGO_PKG_VERSION"),
        option_env!("NOTCHLING_BUILD"),
        !cfg!(debug_assertions),
        &system,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_version_build_and_license() {
        let a = about("1.3.0-beta.2", Some("0bdc0b0"), true, "macos aarch64");
        assert_eq!(a.version, "1.3.0-beta.2");
        assert_eq!(a.channel, "Beta");
        assert_eq!(a.build, "0bdc0b0, release, macos aarch64");
        assert_eq!(a.license, "MIT");
        let stable = about("1.2.1", None, false, "linux x86_64");
        assert_eq!(stable.channel, "Stable");
        assert_eq!(stable.build, "dev, debug, linux x86_64");
        assert_eq!(about("1.2.1", Some(""), true, "x").build, "dev, release, x");
    }

    #[test]
    fn links_are_https_github() {
        for link in LINKS {
            assert!(link
                .url
                .starts_with("https://github.com/dtthaison/notchling"));
            assert!(!link.title.is_empty());
        }
    }

    #[test]
    fn this_build_reports_the_crate_version_and_its_license() {
        let a = current();
        assert_eq!(a.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(a.license, env!("CARGO_PKG_LICENSE"));
    }
}
