const MODE_FLAG: &str = "--permission-mode";

pub fn mode_of(flags: &[String]) -> Option<String> {
    let mut found = None;
    let mut next = false;
    for flag in flags {
        if next {
            found = Some(flag.clone());
            next = false;
        } else if flag == MODE_FLAG {
            next = true;
        } else if let Some(mode) = flag.strip_prefix("--permission-mode=") {
            found = Some(mode.to_string());
        }
    }
    found
}
