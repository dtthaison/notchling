use super::registry::Machine;
use super::shell;
use super::step::{
    run, Cadence, CheckResult, Detected, FixContext, FixOutcome, FixPlan, Probe, RemoteOutput,
    SetupStep, StepContext, StepFix, StepId,
};
use super::steps_node::{
    arch_word, first_text_line, machine_refusal, os_word, said, value_of, APP_DIR, MAC_BUILD,
};
use super::steps_node_mac::{
    binary_new, binary_path, copy_heading, mac_restart, mac_restart_text, place_script,
    prepare_script, source_for, where_word, Host,
};
use super::steps_node_service::{restart_command, stops_work};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
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
        Probe::remote(probe_script(m), parse_binary),
        StepFix::Run(FixPlan::new(display(m), !m.is_linux(), binary_fix)),
    )
}

fn probe_script(m: &Machine) -> String {
    let app = if m.is_linux() {
        String::new()
    } else {
        format!("\nif test -d {APP_DIR}; then echo app=yes; else echo app=no; fi")
    };
    format!(
        "p={}\n\"$p\" --version 2>&1 | head -1\nif command -v sha256sum >/dev/null 2>&1; then h=$(sha256sum \"$p\" 2>/dev/null); else h=$(shasum -a 256 \"$p\" 2>/dev/null); fi\necho \"sha256=${{h%% *}}\"{app}",
        shell::path(binary_path(m))
    )
}

const MAX_HASHED: u64 = 512 * 1024 * 1024;

fn hash_file(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = file.take(MAX_HASHED + 1);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_HASHED {
            return None;
        }
        hasher.update(&buf[..n]);
    }
    Some(format!("{:x}", hasher.finalize()))
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
    let digest = hash_file(path)?;
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(path.to_path_buf(), (stamp.0, stamp.1, digest.clone()));
    Some(digest)
}

fn parse_binary(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    let app = if ctx.machine.is_linux() {
        None
    } else {
        match value_of(&out.text, "app=") {
            Some("yes") => Some(true),
            Some("no") => Some(false),
            _ => None,
        }
    };
    let result = binary_state(out, ctx);
    match app {
        Some(_) => result.detected(Detected {
            notchling_app: app,
            ..Detected::default()
        }),
        None => result,
    }
}

fn binary_state(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    let line = first_text_line(&out.text);
    let have = value_of(&out.text, "sha256=").unwrap_or("");
    let place = where_word(&ctx.machine);
    if !line.starts_with("notchling ") {
        if !have.is_empty() {
            return CheckResult::fail(format!(
                "notchling is in {place} on the machine but doesn't run: {line}"
            ));
        }
        return CheckResult::fail(format!(
            "notchling isn't installed in {place} on the machine"
        ));
    }
    if (ctx.dist_dir.is_some() || ctx.mac_daemon.is_some())
        && (os_word(&ctx.machine).is_none() || arch_word(&ctx.machine).is_none())
    {
        return CheckResult::cant_tell("Checking the machine's architecture first");
    }
    let host = Host::this();
    let source = source_for(
        &ctx.machine,
        ctx.dist_dir.as_deref(),
        ctx.mac_daemon.as_ref(),
        host,
    );
    let want = match &source {
        Ok(found) => found
            .digest
            .clone()
            .or_else(|| file_hash(&found.path))
            .map(|digest| (digest, found.what.clone())),
        Err(why) if host.build.is_some() && ctx.machine.is_linux() => {
            return CheckResult::fail(why.clone());
        }
        Err(_) => None,
    };
    match want {
        Some((_, _)) if have.is_empty() => CheckResult::cant_tell(format!(
            "{line} is there, but its build can't be hashed on the machine (no sha256sum or shasum)"
        )),
        Some((want, what)) => {
            if have == want {
                CheckResult::pass(format!("{line}, the same build as {what}"))
            } else {
                CheckResult::fail(format!("{line} there, but not the build {what} has"))
            }
        }
        _ if line == MAC_BUILD => CheckResult::pass(line),
        _ => CheckResult::fail(format!("{line} there, this Mac has {MAC_BUILD}")),
    }
}

fn hash_script(new: &str) -> String {
    let new = shell::path(new);
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
    let relative = binary_new(m).strip_prefix("~/").unwrap_or_default();
    format!("{}@{host}:{relative}", m.user)
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
    let linux = m.is_linux();
    let file =
        if linux || os_word(m).is_none() || arch_word(m).as_deref() != Some(std::env::consts::ARCH)
        {
            format!(
                "notchling-{}-{}",
                os_word(m).unwrap_or("<system>"),
                arch_word(m).as_deref().unwrap_or("<arch>")
            )
        } else {
            "notchling".to_string()
        };
    let new = shell::path(binary_new(m));
    let restart = if linux {
        restart_command()
    } else {
        format!("As admin:\n{}", mac_restart_text())
    };
    let put = if linux {
        "Only when they match, put it in place:"
    } else {
        "Only when they match, put it in place and point ~/.local/bin/notchling at it:"
    };
    format!(
        "{}\n{}\n{}\n{}\nCheck its SHA-256 there against this Mac's:\n{}\nWhen the copy fails or they differ, remove it:\nrm -f {new}\n{put}\n{}\nThen restart the service if it runs:\n{restart}",
        stops_work(linux),
        copy_heading(m),
        prepare_script(m),
        scp_args(&file, m).join(" "),
        hash_script(binary_new(m)),
        place_script(m),
    )
}

fn binary_fix(ctx: &FixContext) -> FixOutcome {
    let m = &ctx.step.machine;
    if let Some(why) = machine_refusal(m) {
        return FixOutcome::fail(why);
    }
    let source = match source_for(
        m,
        ctx.step.dist_dir.as_deref(),
        ctx.step.mac_daemon.as_ref(),
        Host::this(),
    ) {
        Ok(found) => found,
        Err(why) => return FixOutcome::fail(why),
    };
    let name = format!(
        "notchling-{}-{}",
        os_word(m).unwrap_or_default(),
        arch_word(m).unwrap_or_default()
    );
    let Some(digest) = source.digest.clone().or_else(|| file_hash(&source.path)) else {
        return FixOutcome::fail(format!(
            "Put a notchling build for {}-{} in {} first.",
            os_word(m).unwrap_or_default(),
            arch_word(m).unwrap_or_default(),
            source.path.display()
        ));
    };
    let rm_new = format!("rm -f {}", shell::path(binary_new(m)));
    let made = run::script(ctx, &prepare_script(m), None, 60);
    if made.code != 0 {
        return FixOutcome::fail(format!(
            "couldn't make the folders there (exit {}): {}",
            made.code,
            said(ctx, &made)
        ));
    }
    let copied = run::local(ctx, scp_args(&source.path.to_string_lossy(), m), None, 900);
    if copied.code != 0 {
        let _ = run::script(ctx, &rm_new, None, 60);
        return FixOutcome::fail(format!(
            "the copy failed (exit {}): {}",
            copied.code,
            said(ctx, &copied)
        ));
    }
    let checked = run::script(ctx, &hash_script(binary_new(m)), None, 120);
    let theirs = checked.out.split_whitespace().next().unwrap_or("");
    if checked.code != 0 || theirs != digest {
        let _ = run::script(ctx, &rm_new, None, 60);
        return FixOutcome::fail(
            "the copy on the machine doesn't match this Mac's build; it was removed",
        );
    }
    let moved = run::script(ctx, &place_script(m), None, 60);
    if moved.code != 0 {
        return FixOutcome::fail(format!(
            "couldn't put the build in place (exit {}): {}",
            moved.code,
            said(ctx, &moved)
        ));
    }
    let restarted = if m.is_linux() {
        run::script(ctx, &restart_command(), None, 120)
    } else {
        mac_restart(ctx)
    };
    if restarted.code != 0 {
        return FixOutcome::fail(format!(
            "{name} is in place, but the service didn't restart (exit {}): {}",
            restarted.code,
            said(ctx, &restarted)
        ));
    }
    FixOutcome::ok(format!(
        "{name} copied and its hash matches ({})",
        source.label
    ))
}
