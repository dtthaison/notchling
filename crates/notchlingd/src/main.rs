use notchling_notchlingd::cli;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = match cli::parse(args.clone()) {
        Ok(command) => command,
        Err(_) if args.first().is_some_and(|a| a == "hook") => std::process::exit(0),
        Err(message) => {
            eprintln!("notchling: {message}\n\n{}", cli::usage());
            std::process::exit(2);
        }
    };
    std::process::exit(notchling_notchlingd::run(command));
}
