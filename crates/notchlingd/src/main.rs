//! `notchling`: the daemon (`notchling daemon`) and its command line. See the library's docs.

use notchling_notchlingd::cli;

fn main() {
    let command = match cli::parse(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("notchling: {message}\n\n{}", cli::usage());
            std::process::exit(2);
        }
    };
    std::process::exit(notchling_notchlingd::run(command));
}
