use super::registry::Machine;
use super::shell;
use super::step::{
    run, Cadence, CheckResult, FixContext, FixOutcome, FixPlan, Probe, RemoteOutput, SetupStep,
    StepContext, StepFix, StepId,
};
use super::steps_node::{
    arch_word, first_text_line, machine_refusal, os_word, said, value_of, BINARY, BINARY_NEW,
    MAC_BUILD,
};
use super::steps_node_service::{restart_command, STOPS_WORK};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

type Cached = (SystemTime, u64, String);

static HASHES: OnceLock<Mutex<BTreeMap<PathBuf, Cached>>> = OnceLock::new();

pub(super) fn binary(m: &Machine) -> SetupStep {
    SetupStep::new(
        StepId::NODE_BINARY,
        "Notchling installed",
        vec![StepId::REACHABLE],
        Cadence::EveryRefresh,
        Probe::remote(probe_script(), parse_binary),
        StepFix::Run(FixPlan::new(display(m), false, binary_fix)),
    )
}

fn probe_script() -> String {
    format!(
        "p={}\n\"$p\" --version 2>&1 | head -1\nif command -v sha256sum >/dev/null 2>&1; then h=$(sha256sum \"$p\" 2>/dev/null); else h=$(shasum -a 256 \"$p\" 2>/dev/null); fi\necho \"sha256=${{h%% *}}\"",
        shell::path(BINARY)
    )
}

fn hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn file_hash(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
    let stamp = (meta.modified().ok()?, meta.len());
    let cache = HASHES.get_or_init(Default::default);
    let known = cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(path)
        .filter(|(time, size, _)| (*time, *size) == stamp)
        .map(|(_, _, digest)| digest.clone());
    if known.is_some() {
        return known;
    }
    let digest = hex(&std::fs::read(path).ok()?);
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(path.to_path_buf(), (stamp.0, stamp.1, digest.clone()));
    Some(digest)
}

fn dist_path(m: &Machine, dist: Option<&PathBuf>) -> Option<PathBuf> {
    let name = format!("notchling-{}-{}", os_word(m)?, arch_word(m)?);
    Some(dist?.join(name))
}

fn parse_binary(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    let line = first_text_line(&out.text);
    let have = value_of(&out.text, "sha256=").unwrap_or("");
    if !line.starts_with("notchling ") {
        if !have.is_empty() {
            return CheckResult::fail(format!(
                "notchling is in ~/.local/bin on the machine but doesn't run: {line}"
            ));
        }
        return CheckResult::fail("notchling isn't installed in ~/.local/bin on the machine");
    }
    if ctx.dist_dir.is_some()
        && (os_word(&ctx.machine).is_none() || arch_word(&ctx.machine).is_none())
    {
        return CheckResult::cant_tell("Checking the machine's architecture first");
    }
    let want = dist_path(&ctx.machine, ctx.dist_dir.as_ref()).and_then(|p| file_hash(&p));
    match want {
        Some(_) if have.is_empty() => CheckResult::cant_tell(format!(
            "{line} is there, but its build can't be hashed on the machine (no sha256sum or shasum)"
        )),
        Some(want) => {
            if have == want {
                CheckResult::pass(format!("{line}, the same build as this Mac's"))
            } else {
                CheckResult::fail(format!("{line} there, but not the build this Mac has"))
            }
        }
        _ if line == MAC_BUILD => CheckResult::pass(line),
        _ => CheckResult::fail(format!("{line} there, this Mac has {MAC_BUILD}")),
    }
}

fn hash_script() -> String {
    let new = shell::path(BINARY_NEW);
    format!(
        "if command -v sha256sum >/dev/null 2>&1; then sha256sum {new}; else shasum -a 256 {new}; fi"
    )
}

fn scp_target(m: &Machine) -> String {
    let host = if m.host.contains(':') {
        format!("[{}]", m.host)
    } else {
        m.host.clone()
    };
    format!("{}@{host}:.local/bin/notchling.new", m.user)
}

fn scp_args(from: &str, m: &Machine) -> Vec<String> {
    vec![
        "/usr/bin/scp".into(),
        "-O".into(),
        "-q".into(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ConnectTimeout=8".into(),
        "--".into(),
        from.into(),
        scp_target(m),
    ]
}

fn display(m: &Machine) -> String {
    let file = format!(
        "notchling-{}-{}",
        os_word(m).unwrap_or("<system>"),
        arch_word(m).as_deref().unwrap_or("<arch>")
    );
    let new = shell::path(BINARY_NEW);
    let old = shell::path(BINARY);
    format!(
        "{STOPS_WORK}\nCopy {file} from this Mac's cluster dist folder:\nmkdir -p ~/.local/bin\n{}\nCheck its SHA-256 there against this Mac's:\n{}\nWhen the copy fails or they differ, remove it:\nrm -f {new}\nOnly when they match, put it in place:\nchmod 755 {new} && mv -f {new} {old}\nThen restart the service if it runs:\n{}",
        scp_args(&file, m).join(" "),
        hash_script(),
        restart_command(m.is_linux())
    )
}

fn binary_fix(ctx: &FixContext) -> FixOutcome {
    let m = &ctx.step.machine;
    if let Some(why) = machine_refusal(m) {
        return FixOutcome::fail(why);
    }
    let (Some(os), Some(arch)) = (os_word(m), arch_word(m)) else {
        return FixOutcome::fail("the machine's system and chip aren't known yet; check it first");
    };
    let name = format!("notchling-{os}-{arch}");
    let Some(dist) = &ctx.step.dist_dir else {
        return FixOutcome::fail(format!(
            "Put a notchling build for {os}-{arch} in the cluster dist folder first."
        ));
    };
    let path = dist.join(&name);
    let Some(digest) = file_hash(&path) else {
        return FixOutcome::fail(format!(
            "Put a notchling build for {os}-{arch} in {} first.",
            path.display()
        ));
    };
    let made = run::script(
        ctx,
        &format!("mkdir -p {}", shell::path("~/.local/bin")),
        None,
        60,
    );
    if made.code != 0 {
        return FixOutcome::fail(format!(
            "couldn't make ~/.local/bin (exit {}): {}",
            made.code,
            said(ctx, &made)
        ));
    }
    let copied = run::local(ctx, scp_args(&path.to_string_lossy(), m), None, 900);
    if copied.code != 0 {
        let _ = run::script(ctx, &format!("rm -f {}", shell::path(BINARY_NEW)), None, 60);
        return FixOutcome::fail(format!(
            "the copy failed (exit {}): {}",
            copied.code,
            said(ctx, &copied)
        ));
    }
    let checked = run::script(ctx, &hash_script(), None, 120);
    let theirs = checked.out.split_whitespace().next().unwrap_or("");
    if checked.code != 0 || theirs != digest {
        let _ = run::script(ctx, &format!("rm -f {}", shell::path(BINARY_NEW)), None, 60);
        return FixOutcome::fail(
            "the copy on the machine doesn't match this Mac's build; it was removed",
        );
    }
    let new = shell::path(BINARY_NEW);
    let old = shell::path(BINARY);
    let moved = run::script(
        ctx,
        &format!("chmod 755 {new} && mv -f {new} {old}"),
        None,
        60,
    );
    if moved.code != 0 {
        return FixOutcome::fail(format!(
            "couldn't put the build in place (exit {}): {}",
            moved.code,
            said(ctx, &moved)
        ));
    }
    let restarted = run::script(ctx, &restart_command(m.is_linux()), None, 120);
    if restarted.code != 0 {
        return FixOutcome::fail(format!(
            "{name} is in place, but the service didn't restart (exit {}): {}",
            restarted.code,
            said(ctx, &restarted)
        ));
    }
    FixOutcome::ok(format!("{name} copied and its hash matches"))
}
