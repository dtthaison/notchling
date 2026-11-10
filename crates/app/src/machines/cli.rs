//! `notchling-machines`' command line: nothing (the window, off), `--live` (ssh and `gh` run),
//! `--vault` (the Secret Vault's values are read), `--data-root <dir>` (a fixture root instead of
//! the real folders), `--render <view> --out <file.png>`, `--version`, `--help`.

use std::path::PathBuf;

use super::renders;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// The Machines window. A fixture root never runs a process or opens the vault, whatever
    /// the flags say.
    Run {
        data_root: Option<PathBuf>,
        live: bool,
        vault: bool,
    },
    /// Open `view` off to the side, write it to `out` as a PNG, exit (part U10).
    Render {
        view: String,
        out: PathBuf,
        data_root: Option<PathBuf>,
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
    let mut data_root: Option<PathBuf> = None;
    let mut live = false;
    let mut vault = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--render" => view = Some(value(&mut args, "--render", "a view name")?),
            "--out" => out = Some(PathBuf::from(value(&mut args, "--out", "a file path")?)),
            "--data-root" => {
                data_root = Some(PathBuf::from(value(&mut args, "--data-root", "a folder")?))
            }
            "--live" => live = true,
            "--vault" => vault = true,
            "--version" | "-V" => return Ok(Command::Version),
            "--help" | "-h" => return Ok(Command::Help),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    match (view, out) {
        (None, None) => Ok(Command::Run {
            data_root,
            live,
            vault,
        }),
        (Some(view), Some(out)) => {
            if live || vault {
                return Err("--render takes neither --live nor --vault".to_owned());
            }
            if renders::shown(&view).is_none() {
                return Err(format!(
                    "unknown view {view}; the views are {}",
                    renders::VIEWS
                        .iter()
                        .map(|v| v.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if out.extension().and_then(|e| e.to_str()) != Some("png") {
                return Err("--out must name a .png file".to_owned());
            }
            Ok(Command::Render {
                view,
                out,
                data_root,
            })
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

/// `notchling-machines <version>`.
pub fn version_line() -> String {
    format!("notchling-machines {}", env!("CARGO_PKG_VERSION"))
}

pub fn usage() -> String {
    let mut text = String::from(
        "usage: notchling-machines [--data-root <dir>] [--live] [--vault]   the Machines window\n       \
         notchling-machines --render <view> --out <file.png> [--data-root <dir>]\n       \
         notchling-machines --version | --help\n\n\
         Off until the maintainer hands Machines over: without --live no ssh or gh runs (every \
         check says so), and without --vault no secret value is read. It reads the Swift app's \
         machines.json and writes only its own machines and update offers in <data>/notchling.db, \
         plus its audit log and scratch files under <data>/daemon/machines/.\n\n\
         --data-root <dir> (or NOTCHLING_DATA_ROOT) reads <dir>/notchling/machines.json instead of \
         the real folder, and never runs a process or opens the vault. A render without one reads \
         the fixture this build carries.\n\n\
         views:\n",
    );
    for spec in renders::VIEWS {
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
    fn nothing_runs_the_window_off() {
        assert_eq!(
            parse_str(""),
            Ok(Command::Run {
                data_root: None,
                live: false,
                vault: false
            })
        );
        assert_eq!(
            parse_str("--live --vault --data-root /f"),
            Ok(Command::Run {
                data_root: Some(PathBuf::from("/f")),
                live: true,
                vault: true
            })
        );
    }

    #[test]
    fn render_takes_a_view_and_a_png_and_no_switches() {
        assert_eq!(
            parse_str("--render machines --out /tmp/m.png"),
            Ok(Command::Render {
                view: "machines".into(),
                out: PathBuf::from("/tmp/m.png"),
                data_root: None
            })
        );
        assert!(parse_str("--render machines --out /tmp/m.png --live").is_err());
        assert!(parse_str("--render machines").is_err());
        assert!(parse_str("--out /tmp/m.png").is_err());
        assert!(parse_str("--render nope --out /tmp/m.png")
            .unwrap_err()
            .contains("machines-fix"));
        assert!(parse_str("--render machines --out /tmp/m.jpg").is_err());
        assert!(parse_str("--data-root").is_err());
    }

    #[test]
    fn version_help_and_unknown() {
        assert_eq!(parse_str("--version"), Ok(Command::Version));
        assert_eq!(parse_str("-h"), Ok(Command::Help));
        assert_eq!(
            parse_str("--frobnicate"),
            Err("unknown argument: --frobnicate".to_owned())
        );
        assert!(version_line().starts_with("notchling-machines "));
        assert!(usage().contains("--live") && usage().contains("machines-jobs"));
    }
}
