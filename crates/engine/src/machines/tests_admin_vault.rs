use super::admin_secret::{AdminSecrets, FromSource, NoAdminSecrets};
use super::engine_plan::{AdminAnswer, AdminNeed, PlanKind};
use super::runner::CommandResult;
use super::step::{run, FixOutcome, FixPlan, StepFix};
use super::tests_engine_world::*;
use crate::accounts::Secret;
use std::sync::{Arc, Mutex};

const ENTRY: &str = "m1-admin";
const VAULT_PW: &str = "test-vault-password";
const TYPED_PW: &str = "test-typed-password";

fn admin_step() -> super::step::SetupStep {
    StepBuilder::new("adm")
        .fix(StepFix::Run(FixPlan::new("echo installing", true, |ctx| {
            let r = run::admin(ctx, "echo installing", 900);
            if r.code == 0 {
                FixOutcome::ok(format!("said {}", r.out.trim()))
            } else {
                FixOutcome::fail(r.err)
            }
        })))
        .build()
}

fn with_entry() -> Setup {
    let mut s = Setup::new(|_, _| vec![admin_step()]);
    s.machines[0].set_admin_secret(Some(ENTRY));
    s.runner.on_call(
        |a| a.last().is_some_and(|x| x.contains("sudo -S")),
        |call| {
            let text = call
                .stdin
                .as_ref()
                .map(|d| String::from_utf8_lossy(d).into_owned())
                .unwrap_or_default();
            if let Some(sink) = &call.on_output {
                sink(super::runner::Channel::Stdout, &format!("echo {text}"));
            }
            CommandResult::ok("installing")
        },
    );
    s
}

fn sudo_calls(w: &World) -> Vec<super::runner::Recorded> {
    w.runner.calls().into_iter().filter(is_sudo).collect()
}

fn audit_file(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "notchling-admin-vault-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("audit.jsonl")
}

#[test]
fn the_vault_password_reaches_sudo_stdin_and_nothing_else() {
    let file = audit_file("stdin");
    let mut s = with_entry();
    s.secrets = Secrets::with(&[(ENTRY, VAULT_PW)]);
    s.audit = Some(file.clone());
    let w = s.build();
    let plan = w.engine.plan("m1", PlanKind::Fix(sid("adm"))).unwrap();
    assert_eq!(plan.admin, AdminNeed::Password);
    assert_eq!(
        lock(&w.secrets.reads).get(ENTRY),
        None,
        "planning never reads the Vault"
    );
    let out = w
        .engine
        .run(w.engine.confirm(plan.id, AdminAnswer::Vault).unwrap());
    assert!(out.ok, "{}", out.message);
    assert_eq!(lock(&w.secrets.reads).get(ENTRY), Some(&1));
    let sudo = sudo_calls(&w);
    assert_eq!(sudo.len(), 1);
    assert_eq!(sudo[0].stdin_text(), format!("{VAULT_PW}\n"));
    for call in w.runner.calls() {
        assert!(!call.text().contains(VAULT_PW));
        assert!(!call.env.iter().any(|(_, v)| v.contains(VAULT_PW)));
    }
    let log = w.log("m1");
    assert!(
        !log.contains(VAULT_PW) && log.contains("[admin password]"),
        "{log}"
    );
    assert!(!out.message.contains(VAULT_PW));
    assert!(!format!("{:?}", w.engine.results("m1")).contains(VAULT_PW));
    let audit = std::fs::read_to_string(&file).unwrap();
    assert!(
        audit.contains(&format!("\"vault\":[\"{ENTRY}\"]")),
        "{audit}"
    );
    assert!(!audit.contains(VAULT_PW));
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn a_missing_entry_fails_the_step_by_name_and_the_typed_password_still_works() {
    let file = audit_file("missing");
    let mut s = with_entry();
    s.audit = Some(file.clone());
    let w = s.build();
    let out = w.fix("m1", "adm", AdminAnswer::Vault);
    assert!(!out.ok);
    assert!(
        out.message.starts_with(&format!(
            "Admin password from Vault entry {ENTRY}: {ENTRY} isn't in the Vault"
        )),
        "{}",
        out.message
    );
    assert!(sudo_calls(&w).is_empty(), "no command ran");
    let typed = w.fix("m1", "adm", pw(TYPED_PW));
    assert!(typed.ok, "{}", typed.message);
    let sudo = sudo_calls(&w);
    assert_eq!(sudo.len(), 1);
    assert_eq!(sudo[0].stdin_text(), format!("{TYPED_PW}\n"));
    let audit = std::fs::read_to_string(&file).unwrap();
    assert!(
        !audit.contains("vault"),
        "a typed password reads no entry: {audit}"
    );
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn an_entry_with_an_empty_value_fails_like_a_missing_one() {
    let mut s = with_entry();
    s.secrets = Secrets::with(&[(ENTRY, "")]);
    let w = s.build();
    let out = w.fix("m1", "adm", AdminAnswer::Vault);
    assert!(!out.ok);
    assert!(out.message.contains(ENTRY), "{}", out.message);
    assert!(sudo_calls(&w).is_empty());
}

#[test]
fn a_vault_answer_needs_an_entry_saved_for_the_machine() {
    let mut s = Setup::new(|_, _| vec![admin_step()]);
    s.secrets = Secrets::with(&[(ENTRY, VAULT_PW)]);
    let w = s.build();
    let plan = w.engine.plan("m1", PlanKind::Fix(sid("adm"))).unwrap();
    let err = w.engine.confirm(plan.id, AdminAnswer::Vault).err().unwrap();
    assert!(err.contains("no Vault entry"), "{err}");
    assert!(w.runner.calls().is_empty());
    assert_eq!(lock(&w.secrets.reads).get(ENTRY), None);
}

#[test]
fn a_typed_password_wins_over_the_entry_and_reads_nothing() {
    let mut s = with_entry();
    s.secrets = Secrets::with(&[(ENTRY, VAULT_PW)]);
    let w = s.build();
    let out = w.fix("m1", "adm", pw(TYPED_PW));
    assert!(out.ok, "{}", out.message);
    assert_eq!(sudo_calls(&w)[0].stdin_text(), format!("{TYPED_PW}\n"));
    assert_eq!(lock(&w.secrets.reads).get(ENTRY), None);
}

#[test]
fn set_up_all_uses_the_entry_without_asking_and_asks_when_there_is_none() {
    let asked = Arc::new(Mutex::new(0));
    let mut s = with_entry();
    s.secrets = Secrets::with(&[(ENTRY, VAULT_PW)]);
    let w = s.build();
    w.answer("echo probe-adm", "no", 1);
    w.refresh("m1", false);
    let plan = w.engine.plan("m1", PlanKind::SetUpAll).unwrap();
    let counter = asked.clone();
    let out = w.engine.run_with(
        w.engine.confirm(plan.id, AdminAnswer::None).unwrap(),
        &move |_| {
            *lock(&counter) += 1;
            AdminAnswer::None
        },
    );
    assert!(sudo_calls(&w).len() == 1, "{}", out.message);
    assert_eq!(sudo_calls(&w)[0].stdin_text(), format!("{VAULT_PW}\n"));
    assert_eq!(*lock(&asked), 0);

    let mut plain = Setup::new(|_, _| vec![admin_step()]);
    plain.secrets = Secrets::with(&[(ENTRY, VAULT_PW)]);
    let w = plain.build();
    w.answer("echo probe-adm", "no", 1);
    w.refresh("m1", false);
    let plan = w.engine.plan("m1", PlanKind::SetUpAll).unwrap();
    let counter = asked.clone();
    let out = w.engine.run_with(
        w.engine.confirm(plan.id, AdminAnswer::None).unwrap(),
        &move |_| {
            *lock(&counter) += 1;
            AdminAnswer::None
        },
    );
    assert!(!out.ok);
    assert_eq!(*lock(&asked), 1);
    assert_eq!(lock(&w.secrets.reads).get(ENTRY), None);
}

#[test]
fn the_engine_reads_through_the_admin_source_it_was_given() {
    struct Fixed(Mutex<Vec<(String, String)>>);
    impl AdminSecrets for Fixed {
        fn read(&self, entry: &str, machine: &str) -> Result<Secret, String> {
            lock(&self.0).push((entry.into(), machine.into()));
            Ok(Secret::new(VAULT_PW))
        }
    }
    let source = Arc::new(Fixed(Mutex::new(Vec::new())));
    let mut s = with_entry();
    s.secrets = Secrets::default();
    let w = s.build();
    let engine = w.engine.with_admin_secrets(source.clone());
    let plan = engine.plan("m1", PlanKind::Fix(sid("adm"))).unwrap();
    let out = engine.run(engine.confirm(plan.id, AdminAnswer::Vault).unwrap());
    assert!(out.ok, "{}", out.message);
    assert_eq!(
        lock(&source.0).clone(),
        vec![(ENTRY.to_string(), "m1".to_string())]
    );
    let sudo: Vec<_> = w.runner.calls().into_iter().filter(is_sudo).collect();
    assert_eq!(sudo[0].stdin_text(), format!("{VAULT_PW}\n"));
}

#[test]
fn a_source_that_refuses_names_the_entry_and_never_leaks() {
    struct Refuses;
    impl AdminSecrets for Refuses {
        fn read(&self, _: &str, _: &str) -> Result<Secret, String> {
            Err("the Vault is off".into())
        }
    }
    let w = with_entry().build();
    let engine = w.engine.with_admin_secrets(Arc::new(Refuses));
    let plan = engine.plan("m1", PlanKind::Fix(sid("adm"))).unwrap();
    let out = engine.run(engine.confirm(plan.id, AdminAnswer::Vault).unwrap());
    assert_eq!(
        out,
        FixOutcome::fail(format!(
            "Admin password from Vault entry {ENTRY}: the Vault is off"
        ))
    );
    assert!(NoAdminSecrets.read(ENTRY, "m1").is_err());
    let source = Secrets::with(&[(ENTRY, VAULT_PW)]);
    assert_eq!(
        FromSource(Arc::new(source))
            .read(ENTRY, "m1")
            .unwrap()
            .expose(),
        VAULT_PW
    );
}

#[test]
fn readiness_is_a_typed_password_or_an_entry() {
    assert!(!AdminAnswer::ready(0, None));
    assert!(!AdminAnswer::ready(0, Some("")));
    assert!(AdminAnswer::ready(3, None));
    assert!(AdminAnswer::ready(0, Some(ENTRY)));
    assert!(matches!(AdminAnswer::pick(None, None), AdminAnswer::None));
    assert!(matches!(
        AdminAnswer::pick(Some(Secret::new("")), Some(ENTRY)),
        AdminAnswer::Vault
    ));
    assert!(matches!(
        AdminAnswer::pick(Some(Secret::new(TYPED_PW)), Some(ENTRY)),
        AdminAnswer::Password(_)
    ));
}

#[test]
fn the_entry_is_saved_per_machine_and_cleared_without_touching_others() {
    let mut s = Setup::new(|_, _| vec![admin_step()]);
    s.machines = vec![mac("m1"), mac("m2")];
    let w = s.build();
    assert!(w.engine.set_admin_secret("m1", Some(ENTRY)).is_ok());
    assert_eq!(lock(&w.store.list)[0].admin_secret(), Some(ENTRY));
    assert_eq!(lock(&w.store.list)[1].admin_secret(), None);
    let saves = lock(&w.store.saves).len();
    assert!(w.engine.set_admin_secret("m1", Some(ENTRY)).is_ok());
    assert_eq!(lock(&w.store.saves).len(), saves, "no change, no write");
    assert!(w.engine.set_admin_secret("m1", None).is_ok());
    assert_eq!(lock(&w.store.list)[0].admin_secret(), None);
    assert!(w.engine.set_admin_secret("nope", Some(ENTRY)).is_err());
    *lock(&w.store.refuse) = true;
    assert!(w.engine.set_admin_secret("m2", Some(ENTRY)).is_err());
}
