use notchling_app::cli::{self, Command};

fn main() {
    let command = match cli::parse(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("{message}\n\n{}", cli::usage());
            std::process::exit(2);
        }
    };
    if matches!(
        command,
        Command::Run {
            data_root: None,
            ..
        }
    ) {
        if let Some(dir) = notchling_platform::paths::data_dir() {
            notchling_app::crash_log::install(dir);
        }
    }
    match command {
        Command::Version => println!("{}", cli::version_line()),
        Command::Help => println!("{}", cli::usage()),
        other => std::process::exit(run(other)),
    }
}

#[cfg(target_os = "macos")]
fn run(command: Command) -> i32 {
    notchling_app::mac::run(command)
}

#[cfg(not(target_os = "macos"))]
fn run(_command: Command) -> i32 {
    println!("{}", notchling_app::NOT_SUPPORTED);
    0
}
