//! `notchling-machines`: the Machines window (part E10), a GPUI window over the engine's
//! `machines` module and apart from the daemon; or `--render <view> --out <file.png>` for a
//! picture of one of its views. Off until the maintainer hands Machines over: no ssh or `gh`
//! without `--live`, no vault value without `--vault`.

use notchling_app::machines::cli::{self, Command};

fn main() {
    let command = match cli::parse(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("{message}\n\n{}", cli::usage());
            std::process::exit(2);
        }
    };
    match command {
        Command::Version => println!("{}", cli::version_line()),
        Command::Help => println!("{}", cli::usage()),
        other => std::process::exit(run(other)),
    }
}

#[cfg(target_os = "macos")]
fn run(command: Command) -> i32 {
    notchling_app::machines::window::run_command(command)
}

/// Linux and Windows: the GPUI window isn't built there yet, so say so and exit 0.
#[cfg(not(target_os = "macos"))]
fn run(_command: Command) -> i32 {
    println!("{}", notchling_app::NOT_SUPPORTED);
    0
}
