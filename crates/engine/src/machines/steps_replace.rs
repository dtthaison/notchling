//! Replace key: the Deploy key step's own button (Swift `RepoSteps.replaceKeyPlan`). It lives only in
//! [`super::step::SetupStep::replace`], never in the step's Fix, so Set up and Set up all can't run it.
//!
//! The old key stays valid until the new one is proven: the new pair is made beside the old
//! (`<key>.new`), added read-only, and has to authenticate, be refused a push and be listed
//! read-only before it is moved over the old files. Only then is the old key deleted from GitHub,
//! found by its public key text (never by title). A failure before the move removes the `.new`
//! files and the new key from GitHub and leaves the old key and the ssh alias as they were.

use super::registry::Machine;
use super::repo_common::*;
use super::runner::CommandResult;
use super::shell;
use super::step::{run, FixContext, FixOutcome, FixPlan};
use super::steps_repo::write_pub_file;
use crate::lanes::quote;
use serde_json::Value;
use std::collections::BTreeSet;

/// The titles this machine gives its keys: `agents-<machine>` (Set up) or
/// `agents-<machine>-<8 digits>` (Replace key).
fn is_own_key_title(title: &str, machine: &str) -> bool {
    let base = format!("agents-{machine}");
    if title == base {
        return true;
    }
    match title.strip_prefix(&format!("{base}-")) {
        Some(tail) => tail.len() == 8 && tail.chars().all(|c| c.is_ascii_digit()),
        None => false,
    }
}

/// The scripts of one Replace key, built once from the machine's name and the repo.
struct Scripts {
    read_old: String,
    /// The new private and public key files, quoted for the shell.
    new_key: String,
    new_pub: String,
    ssh_check: String,
    push_check: String,
    swap: String,
    cleanup: String,
}

impl Scripts {
    fn new(github: &str) -> Scripts {
        let path = key_file(github);
        let key = shell::path(&path);
        let key_pub = shell::path(&format!("{path}.pub"));
        let new_key = shell::path(&format!("{path}.new"));
        let new_pub = shell::path(&format!("{path}.new.pub"));
        let read_old = format!("f={key_pub}; if [ -f \"$f\" ]; then head -c 1024 \"$f\"; fi");
        // -F /dev/null: the check uses the new key alone, not whatever the ssh config would offer
        let ssh_check = format!(
            "ssh -F /dev/null -i {new_key} -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes -o BatchMode=yes -o ConnectTimeout=8 -T git@github.com </dev/null 2>&1 || true"
        );
        // a throwaway repo, so no checkout is needed; hooks off and stdin closed, as the
        // Push-is-refused check
        let push_check = format!(
            "k={new_key}; t=$(mktemp -d) || {{ echo '@@push 99'; exit 0; }}\n\
             trap 'rm -rf \"$t\"' EXIT\n\
             git -c core.hooksPath=/dev/null init -q \"$t\" </dev/null >/dev/null 2>&1 && \
             git -c core.hooksPath=/dev/null -c commit.gpgsign=false -c user.name=notchling -c user.email=notchling@localhost \
             -C \"$t\" commit -q --allow-empty -m probe </dev/null >/dev/null 2>&1 || {{ echo '@@push 98'; exit 0; }}\n\
             GIT_TERMINAL_PROMPT=0 GIT_SSH_COMMAND=\"ssh -F /dev/null -i '$k' -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes -o BatchMode=yes -o ConnectTimeout=8\" \
             git -c core.hooksPath=/dev/null -C \"$t\" push --dry-run git@github.com:{github}.git HEAD:refs/heads/agents-push-test </dev/null 2>&1\n\
             echo \"@@push $?\""
        );
        // Exit 43: a file is missing and 41: the private key could not move, so nothing moved; 42:
        // only the .pub could not move (it is then rewritten from the private key, through a
        // temporary file). These are far from the codes a signal (1 to 15, as the runner reports
        // it), ssh (255) or a timeout (124) can give: any other code proves nothing about what was
        // moved.
        let tmp_pub = shell::path(&format!("{path}.pub.tmp"));
        let swap = format!(
            "[ -f {new_key} ] && [ -f {new_pub} ] || exit 43\n\
             mv -f {new_key} {key} || exit 41\n\
             mv -f {new_pub} {key_pub} || {{ ssh-keygen -y -f {key} > {tmp_pub} 2>/dev/null && mv -f {tmp_pub} {key_pub}; rm -f {tmp_pub}; exit 42; }}"
        );
        let cleanup = format!("rm -f {new_key} {new_pub}");
        Scripts {
            read_old,
            new_key,
            new_pub,
            ssh_check,
            push_check,
            swap,
            cleanup,
        }
    }

    /// The script that makes the new pair and prints its public line.
    fn keygen(&self, comment: &str) -> String {
        let (new_key, new_pub) = (&self.new_key, &self.new_pub);
        format!(
            "set -e; mkdir -p \"$HOME/.ssh\"; chmod 700 \"$HOME/.ssh\"\n\
             rm -f {new_key} {new_pub}\n\
             ssh-keygen -q -t ed25519 -N '' -C {} -f {new_key}\n\
             cat {new_pub}",
            quote(comment)
        )
    }
}

/// The plan Replace key runs, shown first in full.
pub fn replace_key_plan(m: &Machine, github: &str) -> FixPlan {
    let name = m.name.clone();
    let scripts = Scripts::new(github);
    let list = list_argv(github).join(" ");
    let display = format!(
        "On {name}:\n{}\n(the old public key is read first)\n\n\
         On {name} (<yyyymmdd> is today's date in UTC):\n{}\n(the new key is made next to the old one; the old key stays valid)\n\n\
         On this Mac:\n{list}\n(the keys now on GitHub; a new key that is already there stops the run before anything is added)\n\n\
         On this Mac:\n{}\n(read-only: the key is added without write access)\n\n\
         On {name}:\n{}\n(the new key must authenticate)\n\n\
         On {name}:\n{}\n(GitHub must refuse the push)\n\n\
         On this Mac:\n{list}\n(GitHub must list the new key as read-only)\n\n\
         Only when every check above passed:\nOn {name}:\n{}\n\n\
         On this Mac:\n{}\n(the old key is found by its public key text, and only deleted when its title is this machine's own: agents-{name} or agents-{name}-<8 digits>)\n\n\
         If a check fails before the swap, these files are removed from {name}:\n{}\n\n\
         and the new key is deleted from GitHub, only a key that was not listed before the add and has the new key's title:\n\
         {list}\n{}",
        scripts.read_old,
        scripts.keygen(&format!("agents-{name}-<yyyymmdd>")),
        add_argv(
            "<the new public key file>",
            github,
            &format!("agents-{name}-<yyyymmdd>")
        )
        .join(" "),
        scripts.ssh_check,
        scripts.push_check,
        scripts.swap,
        delete_argv("<id of the old key>", github).join(" "),
        scripts.cleanup,
        delete_argv("<id of the new key>", github).join(" "),
    );
    let github = github.to_string();
    FixPlan::new(display, false, move |ctx| {
        run_replace(ctx, &name, &github, &scripts)
    })
}

/// One run: what it knows while it goes, and how it undoes itself.
struct Run<'a> {
    ctx: &'a FixContext,
    github: &'a str,
    name: &'a str,
    title: String,
    scripts: &'a Scripts,
    /// The ids on GitHub before the add: nothing in this list is ever deleted by this run.
    before: BTreeSet<i64>,
}

impl Run<'_> {
    fn mask(&self, s: &str) -> String {
        self.ctx.mask(s)
    }

    fn said(&self, r: &CommandResult) -> String {
        first_line(
            if r.err.is_empty() { &r.out } else { &r.err },
            &|s| self.mask(s),
            200,
        )
    }

    fn list(&self) -> CommandResult {
        run::local(self.ctx, list_argv(self.github), None, 120)
    }

    /// Deletes the new key from GitHub: `None` when it is gone (or was never there), else what to
    /// tell the user. Only an entry with the new key's text that this run added is deleted: its id
    /// was not in the listing before the add and its title is this run's own.
    fn remove_new_key_from_github(&self, pubkey: &str) -> Option<String> {
        let still = format!(
            "the new key ({}) is still on GitHub; remove it in Settings \u{2192} Deploy keys",
            self.title
        );
        let current = self.list();
        let keys = if current.code == 0 {
            parse_keys(&current.out)
        } else {
            None
        };
        let Some(keys) = keys else {
            return Some(still);
        };
        let entry = listed(pubkey, &keys)?;
        let own = key_id(entry).filter(|id| {
            !self.before.contains(id)
                && entry.get("title").and_then(Value::as_str) == Some(self.title.as_str())
        });
        let Some(id) = own else {
            return Some(format!(
                "a key with the new key's text is on GitHub, but this run did not add it under {}, so nothing was deleted; look in Settings \u{2192} Deploy keys",
                self.title
            ));
        };
        let gone = run::local(
            self.ctx,
            delete_argv(&id.to_string(), self.github),
            None,
            120,
        );
        (gone.code != 0).then_some(still)
    }

    /// A failure before the move: the `.new` files go, and so does the new key on GitHub if it
    /// got there.
    fn undo(&self, why: &str, new_key: Option<&str>, on_github: bool) -> FixOutcome {
        let mut notes: Vec<String> = Vec::new();
        let removed = run::script(self.ctx, &self.scripts.cleanup, None, 60);
        if removed.code != 0 {
            notes.push(format!(
                "the .new key files could not be removed from ~/.ssh on {}",
                self.name
            ));
        }
        if let (true, Some(pubkey)) = (on_github, new_key) {
            if let Some(note) = self.remove_new_key_from_github(pubkey) {
                notes.push(note);
            }
        }
        let mut parts = vec![why.to_string()];
        parts.extend(notes);
        parts.push("the old key and the ssh alias are untouched".into());
        FixOutcome::fail(parts.join("; "))
    }

    fn undo_new(&self, why: &str, pubkey: &str) -> FixOutcome {
        self.undo(why, Some(pubkey), true)
    }
}

fn tail_of(line: &str) -> String {
    if line.is_empty() {
        String::new()
    } else {
        format!(": {line}")
    }
}

fn run_replace(ctx: &FixContext, name: &str, github: &str, scripts: &Scripts) -> FixOutcome {
    if !is_valid_machine_name(name) {
        return FixOutcome::fail(format!("the machine name {name} is not a plain name"));
    }
    if ends_in_replaced_key_date(name) {
        return FixOutcome::fail(format!(
            "a machine name can't end in -<8 digits>; that's how replaced keys are titled ({name})"
        ));
    }
    let title = format!("agents-{name}-{}", date_stamp(ctx.step.now));
    let mut r = Run {
        ctx,
        github,
        name,
        title: title.clone(),
        scripts,
        before: BTreeSet::new(),
    };

    // 1. the old public key, read before anything changes: it is how the old key is found on GitHub
    let reading = run::script(ctx, &scripts.read_old, None, 60);
    let old_pub = if reading.code == 0 {
        single_ed25519_line(&reading.out)
    } else {
        None
    };
    let Some(old_pub) = old_pub else {
        return FixOutcome::fail(format!(
            "unexpected public key on {name}; nothing was changed"
        ));
    };

    // 2. the new pair, next to the old one; only its public line comes back
    let made = run::script(ctx, &scripts.keygen(&title), None, 600);
    if made.code != 0 {
        return r.undo(
            &format!("could not make the new key on {name}: {}", r.said(&made)),
            None,
            false,
        );
    }
    let Some(pubkey) = single_ed25519_line(&made.out) else {
        return r.undo(
            &format!("unexpected public key from {name}; nothing was added"),
            None,
            false,
        );
    };
    if normalised_key(&pubkey) == normalised_key(&old_pub) {
        return r.undo("the new key is the old key; nothing was added", None, false);
    }

    // 2b. what GitHub lists now. The new key must not be on the repo already (a machine could hand
    // back another key's text); and nothing in this list is ever deleted by this run.
    let prior = r.list();
    let prior_keys = if prior.code == 0 {
        parse_keys(&prior.out)
    } else {
        None
    };
    let Some(prior_keys) = prior_keys else {
        return r.undo(
            &format!(
                "gh could not list the deploy keys of {github}: {}; nothing was added",
                r.said(&prior)
            ),
            None,
            false,
        );
    };
    if listed(&pubkey, &prior_keys).is_some() {
        return r.undo(
            "the new key is already on GitHub; nothing changed",
            None,
            false,
        );
    }
    r.before = prior_keys.iter().filter_map(key_id).collect();

    // 3. read-only, through the user's gh. A failed add is looked for on GitHub too: gh may have
    // been cut short.
    let file_name = format!("{name}-{}-new.pub", slug(github));
    let Some(file) = write_pub_file(&ctx.step.scratch, &file_name, &pubkey) else {
        return r.undo(
            "could not write the public key file on this Mac",
            None,
            false,
        );
    };
    let added = run::local(ctx, add_argv(&file.path(), github, &title), None, 120);
    if added.code != 0 {
        return r.undo_new(
            &format!(
                "gh could not add the new deploy key to {github}: {}",
                r.said(&added)
            ),
            &pubkey,
        );
    }

    // 4. the new key has to work and still be read-only, from three sides
    let auth = run::script(ctx, &scripts.ssh_check, None, 60);
    if !auth.out.contains("successfully authenticated") {
        let line = first_line(
            if auth.out.is_empty() {
                &auth.err
            } else {
                &auth.out
            },
            &|s| ctx.mask(s),
            200,
        );
        return r.undo_new(
            &format!(
                "the new key could not authenticate to GitHub over ssh{}",
                tail_of(&line)
            ),
            &pubkey,
        );
    }
    let pushed = run::script(ctx, &scripts.push_check, None, 90);
    let push_lines: Vec<&str> = pushed.out.split('\n').collect();
    let marker = push_lines.iter().rposition(|l| l.starts_with("@@push "));
    let push_code = marker.and_then(|i| push_lines[i][7..].parse::<i64>().ok());
    let (Some(marker), Some(push_code)) = (marker, push_code) else {
        let line = first_line(
            if pushed.out.is_empty() {
                &pushed.err
            } else {
                &pushed.out
            },
            &|s| ctx.mask(s),
            200,
        );
        return r.undo_new(
            &format!("the push check with the new key failed{}", tail_of(&line)),
            &pubkey,
        );
    };
    let git = push_lines[..marker].join("\n");
    if push_code == 0 {
        return r.undo_new(
            &format!("the new key can write to {github}: a dry-run push was accepted"),
            &pubkey,
        );
    }
    if !git.contains("marked as read only") {
        let line = first_line(&git, &|s| ctx.mask(s), 200);
        let tail = if line.is_empty() {
            format!(" (code {push_code})")
        } else {
            format!(": {line}")
        };
        return r.undo_new(
            &format!("the push check with the new key failed{tail}"),
            &pubkey,
        );
    }
    let current = r.list();
    let keys = if current.code == 0 {
        parse_keys(&current.out)
    } else {
        None
    };
    let Some(keys) = keys else {
        return r.undo_new(
            &format!(
                "gh could not list the deploy keys of {github}: {}",
                r.said(&current)
            ),
            &pubkey,
        );
    };
    let Some(entry) = listed(&pubkey, &keys) else {
        return r.undo_new(
            &format!("GitHub does not list the new key on {github}"),
            &pubkey,
        );
    };
    if !read_only(entry) {
        return r.undo_new(
            &format!("the new key can write to {github} according to GitHub"),
            &pubkey,
        );
    }
    // the old key's id comes from GitHub's JSON and goes into an argv: it has to be an integer,
    // and it is found before the move so that a bad one stops everything while the old key is
    // still in place
    let mut old_id: Option<i64> = None;
    let mut foreign_title: Option<String> = None;
    if let Some(old) = listed(&old_pub, &keys) {
        let Some(id) = key_id(old) else {
            return r.undo_new(
                "the old key's id on GitHub is not a number; no key was deleted and nothing was replaced",
                &pubkey,
            );
        };
        // the text matches; it is deleted only when the title is this machine's own as well
        let found = old.get("title").and_then(Value::as_str).unwrap_or("");
        if is_own_key_title(found, name) {
            old_id = Some(id);
        } else {
            foreign_title = Some(found.to_string());
        }
    }

    // 5. every check passed: the new pair takes the old one's file names, so ~/.ssh/config stays
    // as it is
    let moved = run::script(ctx, &scripts.swap, None, 60);
    match moved.code {
        0 => {}
        42 => {
            return FixOutcome::fail(format!(
                "new key in use, but the public key file on {name} may be stale; Check again. The old key is still on GitHub: remove it in Settings \u{2192} Deploy keys once the check passes"
            ));
        }
        41 | 43 => {
            // the script's own failures: nothing was moved
            return r.undo_new(
                &format!(
                    "could not move the new key over the old one on {name}: {}",
                    r.said(&moved)
                ),
                &pubkey,
            );
        }
        code => {
            // 255 (the connection dropped), a timeout, a kill or a bare 1, 2, 3 (a signal): the
            // move may have happened. Nothing is deleted anywhere.
            return FixOutcome::fail(format!(
                "can't tell whether the key was replaced on {name} (code {code}); the swap may have happened; Check again before anything else. Nothing was removed from GitHub or from {name}"
            ));
        }
    }
    if let Some(found) = foreign_title {
        // the title came from GitHub: masked, then cut, like any text from outside
        let shown = first_line(&found, &|s| ctx.mask(s), 80);
        return FixOutcome::fail(format!(
            "replaced the key on {name}; the old key on GitHub is titled '{shown}', not agents-{name}, so nothing was deleted; remove it in Settings \u{2192} Deploy keys if it's {name}'s"
        ));
    }
    let Some(old_id) = old_id else {
        return FixOutcome::ok(format!(
            "replaced the deploy key of {github} on {name}; the old key was not listed on GitHub, so nothing was deleted"
        ));
    };
    let deleted = run::local(ctx, delete_argv(&old_id.to_string(), github), None, 120);
    if deleted.code != 0 {
        let why = r.said(&deleted);
        let tail = if why.is_empty() {
            String::new()
        } else {
            format!(" (gh said: {why})")
        };
        return FixOutcome::fail(format!(
            "new key in use; the old key is still on GitHub, remove it in Settings \u{2192} Deploy keys{tail}"
        ));
    }
    FixOutcome::ok(format!(
        "replaced the deploy key of {github} on {name}; the old key was deleted from GitHub"
    ))
}
