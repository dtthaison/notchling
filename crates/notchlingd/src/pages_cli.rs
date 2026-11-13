use crate::layout::Layout;
use crate::pages_ops::find_value;
use notchling_engine::pages::PageLibrary;
use notchling_local_api::{ops, Client, ClientError, ClientKind, ErrorCode};
use serde_json::{json, Value};

pub const SHOWN: usize = 10;

pub fn report(reply: &Value, query: &str, list: bool) -> Result<String, String> {
    let pages: &[Value] = reply
        .get("pages")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if pages.is_empty() {
        return Err(format!("No saved page matches \"{query}\"."));
    }
    let text = |page: &Value, key: &str| {
        page.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    if list {
        let lines: Vec<String> = pages
            .iter()
            .map(|p| {
                let or_dash = |s: String| if s.is_empty() { "-".to_string() } else { s };
                let id = p
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("-")
                    .to_string();
                [id, text(p, "title"), or_dash(text(p, "account"))].join("\t")
            })
            .collect();
        return Ok(lines.join("\n"));
    }
    let shown: Vec<String> = pages
        .iter()
        .take(SHOWN)
        .map(|p| text(p, "describe"))
        .collect();
    let mut out = shown.join("\n\n");
    if pages.len() > SHOWN {
        out.push_str(&format!(
            "\n\n…and {} more. Narrow the search, or use `notchling pages`.",
            pages.len() - SHOWN
        ));
    }
    Ok(out)
}

fn lookup(layout: &Layout, query: &str) -> Result<Value, String> {
    let direct = || {
        Ok(find_value(
            &PageLibrary::read_only(layout.data_dir.join("pages")),
            query,
        ))
    };
    let mut client =
        match Client::connect(&layout.socket, ClientKind::Cli, env!("CARGO_PKG_VERSION")) {
            Ok(c) => c,
            Err(ClientError::NotRunning(_) | ClientError::Unsupported) => return direct(),
            Err(e) => return Err(format!("notchling: {e}")),
        };
    match client.call(ops::PAGES_FIND, json!({ "query": query })) {
        Ok(v) => Ok(v),
        Err(ClientError::Api(e)) if e.code == ErrorCode::UnknownOp => direct(),
        Err(e) => Err(format!("notchling: {e}")),
    }
}

pub fn run(layout: &Layout, query: &str, list: bool, json: bool) -> i32 {
    run_to(
        layout,
        query,
        list,
        json,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

fn run_to(
    layout: &Layout,
    query: &str,
    list: bool,
    json: bool,
    out: &mut dyn std::io::Write,
    err: &mut dyn std::io::Write,
) -> i32 {
    let reply = match lookup(layout, query) {
        Ok(v) => v,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 1;
        }
    };
    if json {
        let text = serde_json::to_string_pretty(&reply).unwrap_or_else(|_| reply.to_string());
        let _ = writeln!(out, "{text}");
        return match report(&reply, query, list) {
            Ok(_) => 0,
            Err(message) => {
                let _ = writeln!(err, "{message}");
                1
            }
        };
    }
    match report(&reply, query, list) {
        Ok(text) => {
            let _ = writeln!(out, "{text}");
            0
        }
        Err(message) => {
            let _ = writeln!(err, "{message}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::pages::RecordContext;
    use serde_json::json;

    fn temp(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("nl-pages-cli-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn root_with_pages(tag: &str) -> (std::path::PathBuf, Layout) {
        let root = temp(tag);
        std::fs::create_dir_all(root.join("work")).unwrap();
        std::fs::write(root.join("work/a.html"), "<title>Cost Report</title>").unwrap();
        std::fs::write(root.join("work/b.html"), "<title>Release Plan</title>").unwrap();
        let layout = Layout::resolve(None, Some(root.as_os_str()), || None).unwrap();
        let lib = PageLibrary::writable(layout.data_dir.join("pages"));
        for (file, url, account) in [
            (
                "a.html",
                "https://claude.ai/artifact/AbCdEfGhIjKlMnOpQrStUv",
                "me@example.com",
            ),
            (
                "b.html",
                "https://claude.ai/code/artifact/ZZZZZZZZZZZZZZZZZZZZZZ",
                "",
            ),
        ] {
            let event = json!({
                "tool_name": "Artifact",
                "cwd": root.join("work"),
                "tool_input": {"file_path": file},
                "tool_response": format!("Published {url}"),
            });
            lib.record(
                &event,
                &RecordContext {
                    account,
                    now: 1000.0,
                    home: &root,
                },
            )
            .unwrap()
            .unwrap();
        }
        (root, layout)
    }

    #[test]
    fn page_prints_the_describe_blocks_and_pages_prints_a_table() {
        let (root, layout) = root_with_pages("print");
        assert!(layout.data_dir.join("pages/pages.json").exists());
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(run_to(&layout, "cost", false, false, &mut out, &mut err), 0);
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.starts_with("Title: Cost Report\nLink: https://claude.ai/artifact/AbCd"),
            "{text}"
        );
        assert!(text.contains("Published by: me@example.com\nProject: work\nLast published: "));
        assert!(text.contains("Local copy: "));
        assert!(err.is_empty());

        let mut out = Vec::new();
        assert_eq!(run_to(&layout, "", true, false, &mut out, &mut err), 0);
        let lines: Vec<String> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        assert_eq!(lines.len(), 2);
        assert!(lines.contains(&"AbCdEfGhIjKlMnOpQrStUv\tCost Report\tme@example.com".to_string()));
        assert!(lines.contains(&"ZZZZZZZZZZZZZZZZZZZZZZ\tRelease Plan\t-".to_string()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_link_finds_its_page_and_nothing_found_is_exit_one() {
        let (root, layout) = root_with_pages("find");
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_to(
            &layout,
            "https://claude.ai/code/artifact/ZZZZZZZZZZZZZZZZZZZZZZ",
            false,
            false,
            &mut out,
            &mut err,
        );
        assert_eq!(code, 0);
        assert!(String::from_utf8(out)
            .unwrap()
            .starts_with("Title: Release Plan"));
        let mut out = Vec::new();
        assert_eq!(
            run_to(&layout, "nothing here", false, false, &mut out, &mut err),
            1
        );
        assert!(out.is_empty());
        assert_eq!(
            String::from_utf8(err).unwrap(),
            "No saved page matches \"nothing here\".\n"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn json_prints_the_whole_reply_and_keeps_the_exit_codes() {
        let (root, layout) = root_with_pages("json");
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(run_to(&layout, "", true, true, &mut out, &mut err), 0);
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["pages"].as_array().unwrap().len(), 2, "{v}");
        let cost = v["pages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["title"] == "Cost Report")
            .unwrap();
        assert_eq!(cost["id"], "AbCdEfGhIjKlMnOpQrStUv");
        assert_eq!(cost["account"], "me@example.com");
        assert!(cost["describe"]
            .as_str()
            .unwrap()
            .starts_with("Title: Cost Report"));
        assert!(err.is_empty());

        let mut out = Vec::new();
        assert_eq!(run_to(&layout, "cost", false, true, &mut out, &mut err), 0);
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["pages"].as_array().unwrap().len(), 1);

        let mut out = Vec::new();
        assert_eq!(
            run_to(&layout, "nothing here", false, true, &mut out, &mut err),
            1
        );
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert!(v["pages"].as_array().unwrap().is_empty());
        assert_eq!(
            String::from_utf8(err).unwrap(),
            "No saved page matches \"nothing here\".\n"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_library_that_does_not_exist_is_nothing_found() {
        let root = temp("none");
        let layout = Layout::resolve(None, Some(root.as_os_str()), || None).unwrap();
        let mut err = Vec::new();
        assert_eq!(
            run_to(&layout, "", true, false, &mut Vec::new(), &mut err),
            1
        );
        assert_eq!(
            String::from_utf8(err).unwrap(),
            "No saved page matches \"\".\n"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn more_than_ten_pages_end_with_a_count() {
        let pages: Vec<Value> = (0..13)
            .map(|n| json!({"describe": format!("Title: P{n}"), "id": format!("id{n}"), "title": format!("P{n}"), "account": ""}))
            .collect();
        let text = report(&json!({"pages": pages}), "p", false).unwrap();
        assert_eq!(text.matches("Title: P").count(), 10);
        assert!(text.ends_with("\n\n…and 3 more. Narrow the search, or use `notchling pages`."));
        assert_eq!(
            report(&json!({"pages": pages}), "p", true)
                .unwrap()
                .lines()
                .count(),
            13
        );
        assert!(report(&json!({}), "q", true).is_err());
    }
}
