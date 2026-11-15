const GIT_ARGS: [&str; 7] = [
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--exec-path",
    "--super-prefix",
];

const KUBE_ARGS: [&str; 10] = [
    "-n",
    "--namespace",
    "--context",
    "--cluster",
    "--kubeconfig",
    "-s",
    "--server",
    "--user",
    "--as",
    "--token",
];

const HELM_ARGS: [&str; 4] = ["-n", "--namespace", "--kube-context", "--kubeconfig"];

const PACKAGE_ARGS: [&str; 8] = [
    "--prefix",
    "-C",
    "--workspace",
    "-w",
    "--registry",
    "--filter",
    "-F",
    "--cwd",
];

const DOCKER_ARGS: [&str; 6] = ["-H", "--host", "--context", "-c", "--config", "--log-level"];

const RSYNC_ARGS: [&str; 24] = [
    "-e",
    "--rsh",
    "--exclude",
    "--include",
    "--exclude-from",
    "--include-from",
    "--filter",
    "-f",
    "--log-file",
    "--rsync-path",
    "--backup-dir",
    "--partial-dir",
    "--temp-dir",
    "-T",
    "--compare-dest",
    "--link-dest",
    "--copy-dest",
    "--files-from",
    "--port",
    "--bwlimit",
    "--max-size",
    "--min-size",
    "--timeout",
    "--chmod",
];

const CLOUD_ARGS: [&str; 19] = [
    "--profile",
    "--region",
    "--output",
    "--query",
    "--endpoint-url",
    "--project",
    "--zone",
    "--format",
    "--filter",
    "--account",
    "--subscription",
    "--configuration",
    "--organization",
    "--location",
    "--resource-group",
    "-g",
    "-o",
    "-n",
    "--name",
];

const CURL_ARG_FLAGS: &str = "oHuAebcmwxyYzKECDrQ";

pub fn outward(name: &str, args: &[String]) -> bool {
    match name {
        "git" => first_in(args, &GIT_ARGS, &["push", "send-email"]),
        "gh" => gh(args),
        "npm" | "pnpm" | "yarn" => package(args),
        "cargo" => positionals(args, &[])
            .into_iter()
            .find(|word| !word.starts_with('+'))
            .is_some_and(|word| word == "publish"),
        "docker" => docker(args),
        "kubectl" => first_in(args, &KUBE_ARGS, &["apply", "delete", "rollout"]),
        "helm" => first_in(args, &HELM_ARGS, &["install", "upgrade"]),
        "terraform" | "tofu" => first_in(args, &[], &["apply", "destroy"]),
        "fly" | "flyctl" => first_in(args, &[], &["deploy"]),
        "firebase" => first_in(args, &["-P", "--project", "--config"], &["deploy"]),
        "vercel" => args.iter().any(|a| a == "--prod" || a == "--production"),
        "curl" => curl(args),
        "wget" => wget(args),
        "ssh" | "scp" => true,
        "rsync" => rsync(args),
        "aws" | "gcloud" | "az" => cloud(args),
        _ => false,
    }
}

fn positionals<'a>(args: &'a [String], with_arg: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(word) = args.get(at) {
        at += 1;
        if word == "--" {
            out.extend(args[at..].iter().map(String::as_str));
            break;
        }
        if word.starts_with('-') && word.len() > 1 {
            if with_arg.contains(&word.as_str()) {
                at += 1;
            }
            continue;
        }
        out.push(word);
    }
    out
}

fn first_in(args: &[String], with_arg: &[&str], verbs: &[&str]) -> bool {
    positionals(args, with_arg)
        .first()
        .is_some_and(|word| verbs.contains(word))
}

fn safe_method(method: &str) -> bool {
    matches!(method.to_ascii_uppercase().as_str(), "GET" | "HEAD")
}

fn take(args: &[String], at: &mut usize) -> Option<String> {
    let value = args.get(*at).cloned();
    *at += 1;
    value
}

fn long_flag(word: &str) -> Option<(&str, Option<&str>)> {
    let long = word.strip_prefix("--")?;
    Some(
        long.split_once('=')
            .map_or((long, None), |(name, value)| (name, Some(value))),
    )
}

fn gh(args: &[String]) -> bool {
    let words = positionals(args, &["-R", "--repo", "--hostname"]);
    match words.as_slice() {
        ["pr", "create" | "merge" | "close" | "comment", ..] => true,
        ["release", ..] => true,
        ["api", ..] => gh_api(args),
        _ => false,
    }
}

fn gh_api(args: &[String]) -> bool {
    let mut method: Option<String> = None;
    let mut fields = false;
    let mut at = 0;
    while let Some(word) = args.get(at) {
        at += 1;
        if word == "-X" {
            method = take(args, &mut at);
        } else if let Some(value) = word.strip_prefix("-X") {
            method = Some(value.to_string());
        } else if let Some((name, value)) = long_flag(word) {
            match name {
                "method" => method = value.map(String::from).or_else(|| take(args, &mut at)),
                "field" | "raw-field" | "input" => fields = true,
                _ => {}
            }
        } else if word.starts_with("-f") || word.starts_with("-F") {
            fields = true;
        }
    }
    match method {
        Some(method) => !safe_method(&method),
        None => fields,
    }
}

fn package(args: &[String]) -> bool {
    let words = positionals(args, &PACKAGE_ARGS);
    matches!(words.as_slice(), ["publish", ..] | ["npm", "publish", ..])
}

fn docker(args: &[String]) -> bool {
    let words = positionals(args, &DOCKER_ARGS);
    matches!(
        words.as_slice(),
        ["push", ..] | ["image" | "manifest" | "compose" | "buildx", "push", ..]
    )
}

fn curl(args: &[String]) -> bool {
    let mut method: Option<String> = None;
    let (mut get, mut body, mut always) = (false, false, false);
    let mut at = 0;
    while let Some(word) = args.get(at) {
        at += 1;
        if let Some((name, value)) = long_flag(word) {
            match name {
                "request" => method = value.map(String::from).or_else(|| take(args, &mut at)),
                "get" => get = true,
                "data" | "data-raw" | "data-binary" | "data-ascii" | "data-urlencode" | "json" => {
                    body = true;
                    at += usize::from(value.is_none());
                }
                "form" | "form-string" | "upload-file" => {
                    always = true;
                    at += usize::from(value.is_none());
                }
                _ => {}
            }
        } else if let Some(cluster) = word.strip_prefix('-') {
            let chars: Vec<char> = cluster.chars().collect();
            for (k, c) in chars.iter().enumerate() {
                let rest: String = chars[k + 1..].iter().collect();
                match c {
                    'X' => {
                        method = Some(if rest.is_empty() {
                            take(args, &mut at).unwrap_or_default()
                        } else {
                            rest
                        });
                        break;
                    }
                    'd' => {
                        body = true;
                        at += usize::from(rest.is_empty());
                        break;
                    }
                    'F' | 'T' => {
                        always = true;
                        at += usize::from(rest.is_empty());
                        break;
                    }
                    'G' => get = true,
                    c if CURL_ARG_FLAGS.contains(*c) => {
                        at += usize::from(rest.is_empty());
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
    method.is_some_and(|m| !safe_method(&m)) || always || (body && !get)
}

fn wget(args: &[String]) -> bool {
    let mut at = 0;
    while let Some(word) = args.get(at) {
        at += 1;
        let Some((name, value)) = long_flag(word) else {
            continue;
        };
        match name {
            "post-data" | "post-file" | "body-data" | "body-file" => return true,
            "method" => {
                let method = value.map(String::from).or_else(|| take(args, &mut at));
                if method.is_some_and(|m| !safe_method(&m)) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn remote(path: &str) -> bool {
    path.starts_with("rsync://")
        || path
            .split_once(':')
            .is_some_and(|(host, _)| !host.is_empty() && !host.contains('/'))
}

fn rsync(args: &[String]) -> bool {
    let words = positionals(args, &RSYNC_ARGS);
    words.len() >= 2 && words.last().is_some_and(|dest| remote(dest))
}

fn is_read_verb(word: &str) -> bool {
    if !word
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return false;
    }
    matches!(word, "ls" | "show" | "help" | "version")
        || ["describe", "list", "get"].iter().any(|verb| {
            word == *verb
                || word
                    .strip_prefix(verb)
                    .is_some_and(|rest| rest.starts_with('-'))
        })
}

fn cloud(args: &[String]) -> bool {
    let words = positionals(args, &CLOUD_ARGS);
    !words.is_empty() && !words.iter().take(4).any(|word| is_read_verb(word))
}
