//! The command line: nothing (run the app), `--render <view> --out <file.png>`, `--version`, `--help`.

use std::path::PathBuf;

use crate::views;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// The menu bar app.
    Run,
    /// Open `view` off to the side, write it to `out` as a PNG, exit (part U10).
    Render {
        view: String,
        out: PathBuf,
    },
    Version,
    Help,
}

/// Parses the arguments after the program name.
pub fn parse<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let mut view: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--render" => view = Some(value(&mut args, "--render", "a view name")?),
            "--out" => out = Some(PathBuf::from(value(&mut args, "--out", "a file path")?)),
            "--version" | "-V" => return Ok(Command::Version),
            "--help" | "-h" => return Ok(Command::Help),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    match (view, out) {
        (None, None) => Ok(Command::Run),
        (Some(view), Some(out)) => {
            if views::spec(&view).is_none() {
                return Err(format!(
                    "unknown view {view}; the views are {}",
                    views::every()
                        .map(|v| v.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if out.extension().and_then(|e| e.to_str()) != Some("png") {
                return Err("--out must name a .png file".to_owned());
            }
            Ok(Command::Render { view, out })
        }
        (Some(_), None) => Err("--render needs --out <file.png>".to_owned()),
        (None, Some(_)) => Err("--out needs --render <view>".to_owned()),
    }
}

fn value(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
    what: &str,
) -> Result<String, String> {
    match args.next() {
        Some(v) if !v.starts_with("--") => Ok(v),
        _ => Err(format!("{flag} needs {what}")),
    }
}

/// `notchling-app <version>`.
pub fn version_line() -> String {
    format!("notchling-app {}", env!("CARGO_PKG_VERSION"))
}

pub fn usage() -> String {
    let mut text = String::from(
        "usage: notchling-app                              the menu bar app\n       \
         notchling-app --render <view> --out <file.png> a picture of a view, then exit\n       \
         notchling-app --version | --help\n\nviews:\n",
    );
    for spec in views::every() {
        text.push_str(&format!("  {:<20} {}\n", spec.name, spec.about));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(line: &str) -> Result<Command, String> {
        parse(line.split_whitespace().map(str::to_owned))
    }

    #[test]
    fn nothing_runs_the_app() {
        assert_eq!(parse_str(""), Ok(Command::Run));
    }

    #[test]
    fn render_takes_a_view_and_a_png() {
        assert_eq!(
            parse_str("--render kit-gallery --out /tmp/g.png"),
            Ok(Command::Render {
                view: "kit-gallery".into(),
                out: PathBuf::from("/tmp/g.png")
            })
        );
        assert_eq!(
            parse_str("--out /tmp/g.png --render kit-gallery-dark"),
            Ok(Command::Render {
                view: "kit-gallery-dark".into(),
                out: PathBuf::from("/tmp/g.png")
            })
        );
    }

    #[test]
    fn render_refuses_half_a_request() {
        assert!(parse_str("--render kit-gallery").is_err());
        assert!(parse_str("--out /tmp/g.png").is_err());
        assert!(parse_str("--render --out /tmp/g.png").is_err());
        assert!(parse_str("--render kit-gallery --out").is_err());
    }

    #[test]
    fn render_refuses_unknown_views_and_non_png() {
        let err = parse_str("--render nope --out /tmp/g.png").unwrap_err();
        assert!(err.contains("unknown view nope") && err.contains("kit-gallery"));
        assert!(parse_str("--render kit-gallery --out /tmp/g.jpg").is_err());
    }

    #[test]
    fn version_help_and_unknown() {
        assert_eq!(parse_str("--version"), Ok(Command::Version));
        assert_eq!(parse_str("-h"), Ok(Command::Help));
        assert_eq!(
            parse_str("--frobnicate"),
            Err("unknown argument: --frobnicate".to_owned())
        );
        assert!(version_line().starts_with("notchling-app "));
        assert!(usage().contains("kit-gallery"));
    }
}
