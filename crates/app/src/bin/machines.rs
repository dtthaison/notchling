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

#[cfg(not(target_os = "macos"))]
fn run(_command: Command) -> i32 {
    println!("{}", notchling_app::NOT_SUPPORTED);
    0
}
