use crate::contract::{self, Failure};
use std::io::Write;

pub fn not_built_to(what: &str, args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    Failure::new(
        contract::REFUSED,
        "not_yet",
        format!("notchling {what} is not built yet."),
    )
    .emit(contract::wants_json(args), out, err)
}

pub fn not_built(what: &str, args: &[String]) -> i32 {
    not_built_to(what, args, &mut std::io::stdout(), &mut std::io::stderr())
}

#[cfg(test)]
#[path = "phase8_cli_tests.rs"]
mod tests;
