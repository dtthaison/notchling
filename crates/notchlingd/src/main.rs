use notchling_notchlingd::cli;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = match cli::parse(args.clone()) {
        Ok(command) => command,
        Err(_) if args.first().is_some_and(|a| a == "hook" || a == "agy-hook") => {
            std::process::exit(0)
        }
        Err(message) => {
            let json = notchling_notchlingd::contract::wants_json(&args);
            let failure = notchling_notchlingd::contract::Failure::usage(format!(
                "notchling: {message}\n\n{}",
                cli::usage()
            ));
            std::process::exit(failure.report(json));
        }
    };
    std::process::exit(notchling_notchlingd::run(command));
}
