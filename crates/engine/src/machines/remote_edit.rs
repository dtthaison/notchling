use super::runner::{Call, CommandResult};
use super::shell;
use super::step::{run, FixContext, FixOutcome, RemoteOutput};
use crate::versions::knob::{sha256_hex, KnobPlan};

pub const MAX_READ: usize = 1024 * 1024;
pub const KEEP_BACKUPS: usize = 5;
pub const CHANGED_SINCE_PLAN: &str = "changed_since_plan";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Read {
    Text { text: String, sha256: String },
    Missing,
    TooBig,
    NotUtf8,
    Unreadable,
}

impl Read {
    pub fn why(&self) -> Option<&'static str> {
        match self {
            Read::Text { .. } | Read::Missing => None,
            Read::TooBig => Some("too_big"),
            Read::NotUtf8 => Some("not_utf8"),
            Read::Unreadable => Some("unreadable"),
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Read::Text { text, .. } => Some(text),
            Read::Missing => Some(""),
            _ => None,
        }
    }
}

const HASH: &str = "h() { if [ \"$(uname)\" = Darwin ]; then shasum -a 256 \"$1\" | cut -d' ' -f1; else sha256sum \"$1\" | cut -d' ' -f1; fi; }";

pub fn read_script_expr(path_word: &str, nonce: &str, max: usize) -> String {
    format!(
        "{HASH}\nf={path_word}\necho path=$f\nif [ ! -e \"$f\" ]; then echo missing=1; exit 0; fi\nif [ ! -f \"$f\" ] || [ ! -r \"$f\" ]; then echo unreadable=1; exit 0; fi\nsize=$(wc -c < \"$f\" | tr -d ' ')\necho size=$size\necho sha256=$(h \"$f\")\nif [ \"$size\" -le {max} ]; then echo '@@begin {nonce}'; cat \"$f\"; echo; echo '@@end {nonce}'; fi"
    )
}

pub fn read_script_max(path: &str, nonce: &str, max: usize) -> String {
    read_script_expr(&shell::path(path), nonce, max)
}

pub fn resolved_path(text: &str) -> Option<String> {
    let head = text.split("@@begin ").next().unwrap_or(text);
    let path = head
        .lines()
        .take(16)
        .find_map(|l| l.strip_prefix("path="))?
        .trim();
    shell::is_safe_machine_path(path).then(|| path.to_string())
}

pub fn read_script(path: &str, nonce: &str) -> String {
    read_script_max(path, nonce, MAX_READ)
}

pub fn parse_read(out: &RemoteOutput, nonce: &str) -> Read {
    let begin = format!("@@begin {nonce}\n");
    let end = format!("\n@@end {nonce}");
    let (head, body) = match out.text.find(&begin) {
        Some(at) => (&out.text[..at], Some(at + begin.len())),
        None => (out.text.as_str(), None),
    };
    let (mut size, mut sha) = (None, None);
    let (mut missing, mut unreadable) = (false, false);
    for line in head.lines().take(16) {
        if let Some(n) = line.strip_prefix("size=") {
            size = n.trim().parse::<usize>().ok();
        } else if let Some(s) = line.strip_prefix("sha256=") {
            let s = s.trim();
            if s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
                sha = Some(s.to_ascii_lowercase());
            }
        } else if line.trim() == "missing=1" {
            missing = true;
        } else if line.trim() == "unreadable=1" {
            unreadable = true;
        }
    }
    if missing {
        return Read::Missing;
    }
    if unreadable {
        return Read::Unreadable;
    }
    let (Some(size), Some(sha)) = (size, sha) else {
        return Read::Unreadable;
    };
    let Some(start) = body else {
        return if size > 0 {
            Read::TooBig
        } else {
            Read::Unreadable
        };
    };
    let rest = &out.text[start..];
    let Some(stop) = rest.rfind(&end) else {
        return Read::Unreadable;
    };
    let text = &rest[..stop];
    if text.contains('\u{fffd}') {
        return Read::NotUtf8;
    }
    if text.len() != size || sha256_hex(text) != sha {
        return Read::Unreadable;
    }
    Read::Text {
        text: text.to_string(),
        sha256: sha,
    }
}

pub fn write_script(path: &str) -> String {
    let f = shell::path(path);
    let empty = sha256_hex("");
    format!(
        "{HASH}\numask 077\nf={f}\nd=$(dirname \"$f\")\nms=$(( $(date +%s) * 1000 + $$ % 1000 ))\ntmp=\"$f.notchling-tmp-$ms\"\nbak=\"$f.notchling-backup-$ms\"\nIFS= read -r want || {{ echo why=no_plan; exit 2; }}\nmkdir -p \"$d\" || {{ echo why=no_folder; exit 4; }}\ncat > \"$tmp\" || {{ rm -f \"$tmp\"; echo why=write_failed; exit 5; }}\nif [ -e \"$f\" ]; then\n  have=$(h \"$f\")\n  if [ \"$have\" != \"$want\" ]; then rm -f \"$tmp\"; echo why={CHANGED_SINCE_PLAN}; exit 3; fi\n  cp \"$f\" \"$bak\" && chmod 600 \"$bak\" || {{ rm -f \"$tmp\" \"$bak\"; echo why=backup_failed; exit 4; }}\n  ls -1t \"$f\".notchling-backup-* 2>/dev/null | tail -n +{} | while IFS= read -r old; do rm -f -- \"$old\"; done\n  mode=$(stat -c %a \"$f\" 2>/dev/null || stat -f %Lp \"$f\" 2>/dev/null)\n  chmod \"${{mode:-600}}\" \"$tmp\"\nelse\n  if [ \"$want\" != {empty} ]; then rm -f \"$tmp\"; echo why={CHANGED_SINCE_PLAN}; exit 3; fi\n  chmod 600 \"$tmp\"\nfi\nmv -f \"$tmp\" \"$f\" || {{ rm -f \"$tmp\"; echo why=write_failed; exit 5; }}\necho ok=1",
        KEEP_BACKUPS + 1
    )
}

pub fn stdin_of(plan: &KnobPlan) -> Vec<u8> {
    format!("{}\n{}", plan.from_sha256, plan.new_text).into_bytes()
}

pub fn friendly(path: &str, home: Option<&str>) -> String {
    match home {
        Some(home) if home.len() > 1 => match path.strip_prefix(home) {
            Some(rest) if rest.starts_with('/') => format!("~{rest}"),
            _ => path.to_string(),
        },
        _ => path.to_string(),
    }
}

fn home_of(ctx: &FixContext) -> Option<String> {
    ctx.step
        .machine
        .abilities
        .as_ref()
        .and_then(|a| a.home.clone())
}

pub fn read_quiet(ctx: &FixContext, path: &str, nonce: &str) -> Read {
    let script = read_script(path, nonce);
    let call = Call::secs(shell::ssh_script(&ctx.step.machine, &script), 30);
    let CommandResult { code, out, .. } = ctx.step.runner.run(&call);
    if code != 0 {
        return Read::Unreadable;
    }
    parse_read(&RemoteOutput { text: out, code }, nonce)
}

fn reason(out: &str) -> Option<String> {
    let why = out.lines().find_map(|l| l.trim().strip_prefix("why="))?;
    (!why.is_empty() && why.len() <= 32 && why.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
        .then(|| why.to_string())
}

pub fn apply(ctx: &FixContext, plan: &KnobPlan) -> FixOutcome {
    let home = home_of(ctx);
    let place = friendly(&plan.place.file(), home.as_deref());
    let r = run::script(
        ctx,
        &write_script(&plan.place.file()),
        Some(stdin_of(plan)),
        30,
    );
    if r.code == 0 && r.out.lines().any(|l| l.trim() == "ok=1") {
        return FixOutcome::ok(format!("Changed {place}"));
    }
    let why = reason(&r.out).unwrap_or_else(|| match r.code {
        255 | 124 => "no_connection".to_string(),
        _ => "write_failed".to_string(),
    });
    FixOutcome::fail(format!("Didn't change {place}: {why}"))
}
