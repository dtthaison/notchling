pub const USAGE: &str = "Usage: notchling network list\n       notchling network switch <tailnet>\n       a switch takes [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--dry-run] [--json]; the user says yes on a card that lists the machines that lose reach";

pub fn run(args: &[String]) -> i32 {
    crate::machines_cli::run_words(args, USAGE, &[], Some("network"))
}
