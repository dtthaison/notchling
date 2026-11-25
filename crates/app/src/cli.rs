use std::path::PathBuf;

use crate::views;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Run {
        data_root: Option<PathBuf>,
        open_chat: bool,
    },
    Render {
        view: String,
        out: PathBuf,
        data_root: Option<PathBuf>,
    },
    Version,
    Help,
}

pub fn parse<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let mut view: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut data_root: Option<PathBuf> = None;
    let mut open_chat = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--open-chat" => open_chat = true,
            "--render" => view = Some(value(&mut args, "--render", "a view name")?),
            "--out" => out = Some(PathBuf::from(value(&mut args, "--out", "a file path")?)),
            "--data-root" => {
                data_root = Some(PathBuf::from(value(&mut args, "--data-root", "a folder")?))
            }
            "--version" | "-V" => return Ok(Command::Version),
            "--help" | "-h" => return Ok(Command::Help),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    match (view, out) {
        (None, None) => Ok(Command::Run {
            data_root,
            open_chat,
        }),
        (Some(_), Some(_)) if open_chat => {
            Err("--open-chat runs the app; a render opens no Chat".to_owned())
        }
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

pub fn version_line() -> String {
    format!("notchling-app {}", env!("CARGO_PKG_VERSION"))
}

pub fn usage() -> String {
    let mut text = String::from(
        "usage: notchling-app [--data-root <dir>] [--open-chat]       the menu bar app (--open-chat: open Chat at launch)\n       \
         notchling-app --render <view> --out <file.png> [--data-root <dir>]\n       \
         notchling-app --version | --help\n\n--data-root <dir> (or NOTCHLING_DATA_ROOT) reads \
         sessions from a fixture root laid out as <dir>/claude, <dir>/notchling and <dir>/codex, \
         never the real folders. A render without one reads the fixture this build carries. \
         NOTCHLING_RENDER_APPEARANCE=light|dark renders in that appearance instead of the \
         system's. NOTCHLING_RENDER_SCROLLBARS=1 draws the overlay scroll bars, which a still \
         frame would otherwise hide.\n\nviews:\n",
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
        assert_eq!(
            parse_str(""),
            Ok(Command::Run {
                data_root: None,
                open_chat: false
            })
        );
        assert_eq!(
            parse_str("--data-root /f"),
            Ok(Command::Run {
                data_root: Some(PathBuf::from("/f")),
                open_chat: false
            })
        );
    }

    #[test]
    fn open_chat_opens_chat_at_launch_and_never_with_a_render() {
        assert_eq!(
            parse_str("--open-chat"),
            Ok(Command::Run {
                data_root: None,
                open_chat: true
            })
        );
        assert!(parse_str("--open-chat --render chat --out /tmp/c.png").is_err());
        assert!(usage().contains("--open-chat"));
    }

    #[test]
    fn render_takes_a_view_and_a_png() {
        assert_eq!(
            parse_str("--render kit-gallery --out /tmp/g.png"),
            Ok(Command::Render {
                view: "kit-gallery".into(),
                out: PathBuf::from("/tmp/g.png"),
                data_root: None
            })
        );
        assert_eq!(
            parse_str("--out /tmp/c.png --render chat --data-root /f"),
            Ok(Command::Render {
                view: "chat".into(),
                out: PathBuf::from("/tmp/c.png"),
                data_root: Some(PathBuf::from("/f"))
            })
        );
    }

    #[test]
    fn render_refuses_half_a_request() {
        assert!(parse_str("--render kit-gallery").is_err());
        assert!(parse_str("--out /tmp/g.png").is_err());
        assert!(parse_str("--render --out /tmp/g.png").is_err());
        assert!(parse_str("--render kit-gallery --out").is_err());
        assert!(parse_str("--data-root").is_err());
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
        assert!(usage().contains("kit-gallery") && usage().contains("NOTCHLING_DATA_ROOT"));
    }
}
