const GIT_ARGS: [&str; 7] = [
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--exec-path",
    "--super-prefix",
];

const GH_ARGS: [&str; 3] = ["-R", "--repo", "--hostname"];

const GH_API_ARGS: [&str; 19] = [
    "-X",
    "--method",
    "-H",
    "--header",
    "-q",
    "--jq",
    "-t",
    "--template",
    "-f",
    "-F",
    "--field",
    "--raw-field",
    "--input",
    "--hostname",
    "-p",
    "--preview",
    "--cache",
    "-R",
    "--repo",
];

const GH_READS: [&str; 13] = [
    "view", "list", "status", "diff", "checks", "checkout", "browse", "search", "download",
    "watch", "auth", "help", "version",
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

const KUBE_READS: [&str; 14] = [
    "get",
    "describe",
    "logs",
    "top",
    "explain",
    "api-resources",
    "api-versions",
    "version",
    "config",
    "diff",
    "auth",
    "wait",
    "cluster-info",
    "port-forward",
];

const HELM_ARGS: [&str; 4] = ["-n", "--namespace", "--kube-context", "--kubeconfig"];

const HELM_READS: [&str; 13] = [
    "list",
    "ls",
    "status",
    "get",
    "history",
    "template",
    "lint",
    "show",
    "search",
    "repo",
    "dependency",
    "version",
    "env",
];

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

const DOCKER_ARGS: [&str; 13] = [
    "-H",
    "--host",
    "--context",
    "-c",
    "--config",
    "--log-level",
    "-f",
    "--file",
    "-p",
    "--project-name",
    "--profile",
    "--env-file",
    "--project-directory",
];

const CARGO_ARGS: [&str; 5] = ["--manifest-path", "-C", "--target-dir", "--config", "-Z"];

const FLY_ARGS: [&str; 8] = [
    "-a",
    "--app",
    "-t",
    "--access-token",
    "-c",
    "--config",
    "-o",
    "--org",
];

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

const CLOUD_ARGS: [&str; 22] = [
    "--profile",
    "--region",
    "--output",
    "--query",
    "--endpoint-url",
    "--ca-bundle",
    "--cli-read-timeout",
    "--cli-connect-timeout",
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

const CLOUD_WRITES: [&str; 40] = [
    "delete",
    "create",
    "update",
    "put",
    "set",
    "deploy",
    "invoke",
    "terminate",
    "rm",
    "cp",
    "mv",
    "sync",
    "patch",
    "run",
    "start",
    "stop",
    "remove",
    "add",
    "import",
    "ssh",
    "scp",
    "exec",
    "apply",
    "build",
    "submit",
    "attach",
    "enable",
    "disable",
    "reset",
    "resize",
    "move",
    "copy",
    "upload",
    "publish",
    "grant",
    "revoke",
    "bind",
    "trigger",
    "rollback",
    "rename",
];

const GCLOUD_READS: [&str; 8] = [
    "list",
    "describe",
    "show",
    "get",
    "ls",
    "info",
    "get-iam-policy",
    "get-value",
];

const CURL_ARG_FLAGS: &str = "oHuAebcmwxyYzECDrQ";

pub fn outward(name: &str, args: &[String]) -> bool {
    match name {
        "git" => git(args),
        "gh" => gh(args),
        "npm" | "pnpm" | "yarn" | "bun" => package(args),
        "cargo" => positionals(args, &CARGO_ARGS)
            .into_iter()
            .find(|word| !word.starts_with('+'))
            .is_some_and(|word| word == "publish"),
        "docker" => docker(args, false),
        "docker-compose" => docker(args, true),
        "notchling" => args.iter().any(|a| {
            matches!(
                a.as_str(),
                "secret" | "--secret" | "delegate" | "--delegate"
            )
        }),
        "kubectl" => not_read(args, &KUBE_ARGS, &KUBE_READS),
        "helm" => not_read(args, &HELM_ARGS, &HELM_READS),
        "terraform" | "tofu" => terraform(args),
        "fly" | "flyctl" => fly(args),
        "firebase" | "firebase-tools" => positionals(args, &["-P", "--project", "--config"])
            .first()
            .is_some_and(|word| word.contains("deploy")),
        "wrangler" => positionals(args, &["-c", "--config", "-e", "--env"])
            .iter()
            .take(3)
            .any(|word| matches!(*word, "deploy" | "publish")),
        "vercel" => args.iter().any(|a| a == "--prod" || a == "--production"),
        "curl" => curl(args),
        "wget" => wget(args),
        "ssh" | "scp" | "sftp" => true,
        "rsync" => rsync(args),
        "aws" => aws(args),
        "gcloud" => gcloud(args, false),
        "az" => gcloud(args, true),
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

fn not_read(args: &[String], with_arg: &[&str], reads: &[&str]) -> bool {
    positionals(args, with_arg)
        .first()
        .is_some_and(|word| !reads.contains(word))
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

fn git(args: &[String]) -> bool {
    let words = positionals(args, &GIT_ARGS);
    matches!(
        words.as_slice(),
        ["push" | "send-email", ..] | ["subtree" | "lfs", "push", ..]
    )
}

fn gh(args: &[String]) -> bool {
    let words = positionals(args, &GH_ARGS);
    match words.as_slice() {
        [] => false,
        ["api", ..] => gh_api(args),
        [first, ..] if GH_READS.contains(first) => false,
        [_] => false,
        [_, verb, ..] => !GH_READS.contains(verb),
    }
}

struct ApiCall {
    method: Option<String>,
    fields: Vec<String>,
    input: bool,
}

fn api_call(args: &[String]) -> ApiCall {
    let mut call = ApiCall {
        method: None,
        fields: Vec::new(),
        input: false,
    };
    let mut at = 0;
    while let Some(word) = args.get(at) {
        at += 1;
        if word == "-X" {
            call.method = take(args, &mut at);
        } else if let Some(value) = word.strip_prefix("-X") {
            call.method = Some(value.to_string());
        } else if matches!(word.as_str(), "-f" | "-F") {
            call.fields.extend(take(args, &mut at));
        } else if let Some(value) = word
            .strip_prefix("-f")
            .or_else(|| word.strip_prefix("-F"))
            .filter(|v| !v.is_empty() && !word.starts_with("--"))
        {
            call.fields.push(value.to_string());
        } else if let Some((name, value)) = long_flag(word) {
            match name {
                "method" => {
                    call.method = value.map(String::from).or_else(|| take(args, &mut at));
                }
                "field" | "raw-field" => {
                    call.fields
                        .extend(value.map(String::from).or_else(|| take(args, &mut at)));
                }
                "input" => call.input = true,
                _ => {}
            }
        }
    }
    call
}

fn gh_api(args: &[String]) -> bool {
    let words = positionals(args, &GH_API_ARGS);
    let call = api_call(args);
    if words.get(1) == Some(&"graphql") {
        return call.input
            || call.fields.iter().any(|field| {
                field.strip_prefix("query=").is_some_and(|query| {
                    query.starts_with('@')
                        || query.contains('$')
                        || query.to_ascii_lowercase().contains("mutation")
                })
            });
    }
    match call.method {
        Some(method) => !safe_method(&method),
        None => call.input || !call.fields.is_empty(),
    }
}

fn package(args: &[String]) -> bool {
    let words = positionals(args, &PACKAGE_ARGS);
    words.contains(&"publish")
}

fn docker(args: &[String], compose: bool) -> bool {
    let mut words = positionals(args, &DOCKER_ARGS);
    if compose {
        words.insert(0, "compose");
    }
    match words.as_slice() {
        ["push", ..] | ["image" | "manifest" | "compose" | "buildx", "push", ..] => true,
        ["build" | "bake", ..]
        | ["buildx" | "builder" | "image" | "compose", "build" | "bake", ..] => pushes(args),
        _ => false,
    }
}

fn pushes(args: &[String]) -> bool {
    let mut at = 0;
    while let Some(word) = args.get(at) {
        at += 1;
        if word == "--push" || word.strip_prefix("--push=").is_some_and(|v| v != "false") {
            return true;
        }
        let output = if word == "--output" || word == "-o" {
            take(args, &mut at)
        } else {
            word.strip_prefix("--output=").map(String::from)
        };
        if output.is_some_and(|o| o.contains("push=true") || o.contains("type=registry")) {
            return true;
        }
    }
    false
}

fn terraform(args: &[String]) -> bool {
    let words = positionals(args, &[]);
    matches!(
        words.as_slice(),
        ["apply" | "destroy" | "import", ..] | ["state", "rm" | "mv" | "push", ..]
    )
}

fn fly(args: &[String]) -> bool {
    let words = positionals(args, &FLY_ARGS);
    matches!(
        words.as_slice(),
        ["deploy" | "scale" | "machine" | "machines", ..]
            | ["secrets", "set" | "unset" | "import", ..]
            | ["apps", "destroy" | "create", ..]
    )
}

fn curl(args: &[String]) -> bool {
    args.split(|a| a == "--next" || a == "-:").any(curl_request)
}

fn curl_request(args: &[String]) -> bool {
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
                "form" | "form-string" | "upload-file" | "config" => {
                    always = true;
                    at += usize::from(value.is_none());
                }
                _ => {}
            }
        } else if let Some(cluster) = word.strip_prefix('-') {
            for (k, c) in cluster.char_indices() {
                let rest = &cluster[k + c.len_utf8()..];
                match c {
                    'X' => {
                        method = Some(if rest.is_empty() {
                            take(args, &mut at).unwrap_or_default()
                        } else {
                            rest.to_string()
                        });
                        break;
                    }
                    'd' => {
                        body = true;
                        at += usize::from(rest.is_empty());
                        break;
                    }
                    'F' | 'T' | 'K' => {
                        always = true;
                        at += usize::from(rest.is_empty());
                        break;
                    }
                    'G' => get = true,
                    c if CURL_ARG_FLAGS.contains(c) => {
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

fn is_verb(word: &str, verbs: &[&str]) -> bool {
    verbs.iter().any(|verb| {
        word == *verb
            || word
                .strip_prefix(verb)
                .is_some_and(|rest| rest.starts_with('-'))
    })
}

fn aws(args: &[String]) -> bool {
    let words = positionals(args, &CLOUD_ARGS);
    match words.as_slice() {
        [] | ["help" | "version"] => false,
        [_, operation, ..] => {
            !(matches!(*operation, "ls" | "show" | "help" | "version")
                || is_verb(operation, &["describe", "list", "get"]))
        }
        [_] => true,
    }
}

fn gcloud(args: &[String], az: bool) -> bool {
    let mut at = 0;
    while let Some(word) = args.get(at) {
        if !word.starts_with('-') || word == "-" {
            break;
        }
        at += if CLOUD_ARGS.contains(&word.as_str()) {
            2
        } else {
            1
        };
    }
    let words: Vec<&str> = args[at.min(args.len())..]
        .iter()
        .map(String::as_str)
        .take_while(|word| !word.starts_with('-') || *word == "-")
        .collect();
    if words.is_empty() {
        return false;
    }
    let read = words.iter().any(|word| {
        GCLOUD_READS.contains(word)
            || (az && (word.starts_with("list-") || word.starts_with("show-")))
    });
    let write = words
        .iter()
        .skip(1)
        .any(|word| is_verb(word, &CLOUD_WRITES));
    !read || write
}
