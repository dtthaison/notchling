use std::io::Write;

use notchling_engine::contract::{matrix, Registry};

pub fn run(json: bool, out: &mut dyn Write) -> i32 {
    let registry = Registry::standard();
    let text = if json {
        match serde_json::to_string_pretty(&matrix::json(&registry)) {
            Ok(t) => format!("{t}\n"),
            Err(e) => {
                eprintln!("notchling: {e}");
                return 1;
            }
        }
    } else {
        matrix::markdown(&registry)
    };
    match out.write_all(text.as_bytes()) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("notchling: {e}");
            1
        }
    }
}
