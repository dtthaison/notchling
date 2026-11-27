use notchling_engine::cost::check::{BUILDER_CAP, LONG_RUN};
use notchling_locale::{cost as w, fmt, units};
use serde_json::Value;

fn num(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn count(v: f64) -> String {
    fmt::count(v.round() as i64)
}

fn money(v: f64) -> String {
    format!("${}", fmt::decimal(v, if v.abs() >= 100.0 { 0 } else { 2 }))
}

fn tokens(v: f64) -> String {
    if v >= 1_000_000.0 {
        units::millions(&fmt::decimal(v / 1_000_000.0, 1))
    } else if v >= 1_000.0 {
        units::thousands(&fmt::decimal(v / 1_000.0, 1))
    } else {
        count(v)
    }
}

fn percent(share: f64) -> String {
    format!("{}%", fmt::decimal(share * 100.0, 0))
}

fn maybe(v: &Value, key: &str, show: impl Fn(f64) -> String) -> String {
    v.get(key)
        .and_then(Value::as_f64)
        .map_or_else(|| w::report_unknown().to_string(), show)
}

fn ev(finding: &Value, name: &str) -> f64 {
    finding
        .get("evidence")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|e| e.get("name").and_then(Value::as_str) == Some(name))
        .map_or(0.0, |e| num(e, "value"))
}

fn detail<'a>(finding: &'a Value, name: &str) -> &'a str {
    finding
        .get("detail")
        .and_then(|d| d.get(name))
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn name_of(id: &str) -> String {
    match id {
        "premium_subagents" => w::report_name_premium_subagents().to_string(),
        "repeated_reviews" => w::report_name_repeated_reviews().to_string(),
        "cold_resumes" => w::report_name_cold_resumes().to_string(),
        "model_switches" => w::report_name_model_switches().to_string(),
        "long_runs" => w::report_name_long_runs().to_string(),
        "many_builders" => w::report_name_many_builders().to_string(),
        "main_effort" => w::report_name_main_effort().to_string(),
        other => other.to_string(),
    }
}

fn evidence_of(f: &Value) -> Vec<String> {
    let id = f.get("id").and_then(Value::as_str).unwrap_or("");
    let mut out = Vec::new();
    match id {
        "premium_subagents" => out.push(w::report_ev_premium_subagents(
            &count(ev(f, "runs")),
            &count(ev(f, "calls")),
            detail(f, "models"),
            &money(ev(f, "cost_usd")),
            &percent(ev(f, "share")),
            detail(f, "target"),
        )),
        "repeated_reviews" => out.push(w::report_ev_repeated_reviews(
            &count(ev(f, "review_runs")),
            &count(ev(f, "repeat_runs")),
            &count(ev(f, "sessions_repeating")),
            &count(ev(f, "most_in_a_session")),
            &money(ev(f, "repeat_cost_usd")),
        )),
        "cold_resumes" => {
            let main = ev(f, "main_rebuilds");
            let sub = ev(f, "sub_rebuilds");
            out.push(w::report_ev_cold_resumes(
                &count(main + sub),
                &count(main),
                &count(sub),
                &money(ev(f, "main_extra_usd") + ev(f, "sub_extra_usd")),
                &fmt::decimal(ev(f, "avg_idle_min"), 0),
            ));
            if ev(f, "ttl1h_added_usd") + ev(f, "ttl1h_saved_usd") > 0.0 {
                let verdict = if ev(f, "ttl1h_pays") > 0.0 {
                    w::report_verdict_pays()
                } else {
                    w::report_verdict_not()
                };
                out.push(w::report_hour_cache(
                    &money(ev(f, "ttl1h_added_usd")),
                    &money(ev(f, "ttl1h_saved_usd")),
                    verdict,
                ));
            }
        }
        "model_switches" => out.push(w::report_ev_model_switches(
            &count(ev(f, "switches")),
            &tokens(ev(f, "rewritten_tokens")),
            &money(ev(f, "extra_usd")),
        )),
        "long_runs" => out.push(w::report_ev_long_runs(
            &count(ev(f, "runs_over_limit")),
            &count(LONG_RUN as f64),
            &count(ev(f, "max_calls")),
            &count(ev(f, "p90_calls")),
            &count(ev(f, "calls_over_150k")),
            &percent(ev(f, "top10_share")),
        )),
        "many_builders" => out.push(w::report_ev_many_builders(
            &count(ev(f, "peak_builders")),
            &count(ev(f, "buckets_over_cap")),
            &count(BUILDER_CAP as f64),
            &money(ev(f, "cost_over_cap_usd")),
            &percent(ev(f, "share_with_many_agents")),
            &count(BUILDER_CAP as f64 + 1.0),
        )),
        "main_effort" => out.push(w::report_ev_main_effort(
            &percent(ev(f, "low_or_medium_share")),
            &count(ev(f, "calls")),
        )),
        _ => {}
    }
    out
}

fn headline(n: usize, f: &Value, subscription: bool) -> String {
    let id = f.get("id").and_then(Value::as_str).unwrap_or("");
    let title = name_of(id);
    let n = n.to_string();
    let Some(saving) = f.get("saving").and_then(Value::as_f64) else {
        return w::report_saving_unknown(&n, &title);
    };
    let share = maybe(f, "saving_share", percent);
    if subscription {
        w::report_saving_subscription(&n, &title, &share, &money(saving))
    } else {
        w::report_saving(&n, &title, &money(saving), &share)
    }
}

fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn guard_name(rule: &str) -> String {
    match rule {
        "model" => w::report_guard_name_model().to_string(),
        "review" => w::report_guard_name_review().to_string(),
        "cap" => w::report_guard_name_cap().to_string(),
        "read" => w::report_guard_name_read().to_string(),
        "tree" => w::report_guard_name_tree().to_string(),
        "poll" => w::report_guard_name_poll().to_string(),
        "size" => w::report_guard_name_size().to_string(),
        other => other.to_string(),
    }
}

fn guard_lines(report: &Value, out: &mut Vec<String>) {
    let Some(guard) = report.get("guard") else {
        return;
    };
    if num(guard, "total") <= 0.0 {
        return;
    }
    out.push(String::new());
    out.push(w::report_guard_heading(&count(num(guard, "days"))));
    out.push(format!(
        "  {}",
        w::report_guard_total(&count(num(guard, "total")), &count(num(guard, "sessions")))
    ));
    for rule in list(guard, "rules") {
        let name = rule.get("rule").and_then(Value::as_str).unwrap_or("");
        out.push(format!(
            "  {}",
            w::report_guard_rule(
                &guard_name(name),
                &count(num(rule, "count")),
                &count(num(rule, "sessions"))
            )
        ));
    }
}

pub(crate) fn signed_percent(change: f64) -> String {
    let rounded = (change * 100.0).round();
    if rounded == 0.0 {
        return "0%".to_string();
    }
    let sign = if rounded > 0.0 { "+" } else { "" };
    format!("{sign}{}%", fmt::decimal(rounded, 0))
}

fn side_line(label: &str, side: &Value) -> Vec<String> {
    let from = side.get("from").and_then(Value::as_str).unwrap_or("");
    let to = side.get("to").and_then(Value::as_str).unwrap_or("");
    let days = num(side, "days").max(1.0);
    if num(side, "calls") <= 0.0 {
        return vec![w::report_compare_nodata(label, from, to)];
    }
    let (spend, per_day) = match side.get("cost").and_then(Value::as_f64) {
        Some(cost) => (money(cost), money(cost / days)),
        None => (
            w::report_unknown().to_string(),
            w::report_unknown().to_string(),
        ),
    };
    let mut lines = vec![w::report_compare_side(
        label,
        from,
        to,
        &spend,
        &per_day,
        &count(num(side, "calls")),
        &tokens(num(side, "tokens")),
    )];
    if num(side, "unpriced_tokens") > 0.0 {
        lines.push(w::report_compare_unpriced(&tokens(num(
            side,
            "unpriced_tokens",
        ))));
    }
    lines
}

fn compare_lines(compare: &Value, out: &mut Vec<String>) {
    let before = compare.get("before").unwrap_or(&Value::Null);
    let after = compare.get("after").unwrap_or(&Value::Null);
    out.push(String::new());
    out.push(w::report_compare_heading(
        compare.get("applied").and_then(Value::as_str).unwrap_or(""),
    ));
    for (label, side) in [
        (w::report_compare_before(), before),
        (w::report_compare_after(), after),
    ] {
        for line in side_line(label, side) {
            out.push(format!("  {line}"));
        }
    }
    let per_day = |side: &Value| {
        side.get("cost")
            .and_then(Value::as_f64)
            .map(|c| c / num(side, "days").max(1.0))
    };
    if let Some((b, a)) = per_day(before)
        .zip(per_day(after))
        .filter(|(b, _)| *b > 0.0)
    {
        out.push(format!(
            "  {}",
            w::report_compare_change(&signed_percent((a - b) / b))
        ));
    }
}

pub fn render(reply: &Value) -> String {
    let report = reply.get("report").unwrap_or(&Value::Null);
    let totals = report.get("totals").unwrap_or(&Value::Null);
    let subscription = report.get("plan").and_then(Value::as_str) == Some("subscription");
    let mut out: Vec<String> = Vec::new();
    out.push(w::report_title(num(report, "days") as i64));
    let calls = count(num(totals, "calls"));
    let all_tokens = tokens(num(totals, "tokens"));
    match totals.get("cost").and_then(Value::as_f64) {
        Some(cost) => {
            let label = match report.get("money").and_then(Value::as_str) {
                Some("estimate") => w::report_label_estimate(),
                _ => w::report_label_api_equivalent(),
            };
            out.push(w::report_spend(&money(cost), &calls, &all_tokens, label));
        }
        None => out.push(w::report_spend_unknown(&calls, &all_tokens)),
    }
    let unpriced = num(totals, "unpriced_calls");
    if unpriced > 0.0 {
        out.push(w::report_unpriced(
            &count(unpriced),
            &tokens(num(totals, "unpriced_tokens")),
        ));
    }
    if reply.get("complete").and_then(Value::as_bool) == Some(false) {
        out.push(w::report_partial().to_string());
    }
    out.push(String::new());
    out.push(w::report_shares_heading().to_string());
    for share in list(report, "by_role") {
        let key = share.get("key").and_then(Value::as_str).unwrap_or("");
        let s = maybe(share, "cost_share", percent);
        let c = count(num(share, "calls"));
        out.push(format!(
            "  {}",
            if key == "sub" {
                w::report_share_sub(&s, &c)
            } else {
                w::report_share_main(&s, &c)
            }
        ));
    }
    for (i, share) in list(report, "by_model").iter().enumerate() {
        let key = share.get("key").and_then(Value::as_str).unwrap_or("");
        let unpriced =
            num(share, "unpriced_calls") > 0.0 && share.get("cost").is_none_or(Value::is_null);
        if i >= 6 && !unpriced {
            continue;
        }
        out.push(format!(
            "  {}",
            if unpriced {
                w::report_share_model_unpriced(
                    key,
                    &tokens(num(share, "tokens")),
                    &tokens(num(share, "input")),
                    &tokens(num(share, "output")),
                    &tokens(num(share, "cached")),
                    &count(num(share, "calls")),
                )
            } else {
                w::report_share_model(
                    key,
                    &maybe(share, "cost_share", percent),
                    &count(num(share, "calls")),
                )
            }
        ));
    }
    if let Some(cache) = report.get("cache") {
        for (who, key) in [(w::report_who_main(), "main"), (w::report_who_sub(), "sub")] {
            let c = cache.get(key).unwrap_or(&Value::Null);
            out.push(w::report_cache(
                who,
                &count(num(c, "one_hour")),
                &count(num(c, "five_min")),
                &count(num(c, "mixed")),
                &count(num(c, "unknown")),
            ));
        }
    }
    out.push(String::new());
    out.push(w::report_findings_heading().to_string());
    let findings = list(report, "findings");
    if findings.is_empty() {
        out.push(format!("  {}", w::report_none()));
    }
    for (i, f) in findings.iter().enumerate() {
        out.push(format!("  {}", headline(i + 1, f, subscription)));
        for line in evidence_of(f) {
            out.push(format!("     {line}"));
        }
        if let Some(t) = f.get("tokens_avoided").and_then(Value::as_f64) {
            out.push(format!("     {}", w::report_tokens(&tokens(t))));
        }
    }
    let worthless = list(report, "not_worth");
    if !worthless.is_empty() {
        out.push(String::new());
        out.push(w::report_notworth_heading().to_string());
        for f in worthless {
            let id = f.get("id").and_then(Value::as_str).unwrap_or("");
            let title = name_of(id);
            out.push(format!(
                "  {}",
                match (f.get("saving").and_then(Value::as_f64), id) {
                    (Some(s), _) => w::report_notworth_saving(&title, &money(s)),
                    (None, "many_builders") => {
                        w::report_notworth_builders(&title, &count(ev(f, "peak_builders")))
                    }
                    (None, "long_runs") => w::report_notworth_long(&title, &count(LONG_RUN as f64)),
                    (None, "repeated_reviews") => w::report_notworth_reviews(&title),
                    (None, _) => w::report_notworth_already(&title),
                }
            ));
        }
    }
    let unknown = list(report, "unknown");
    if !unknown.is_empty() {
        out.push(String::new());
        out.push(w::report_unknown_heading().to_string());
        for u in unknown {
            let n = count(num(u, "count"));
            let line = match u.get("id").and_then(Value::as_str) {
                Some("price") => w::report_unknown_price(&n),
                Some("cache_ttl") => w::report_unknown_cache_ttl(&n),
                Some("agent_type") => w::report_unknown_agent_type(&n),
                Some("effort") => w::report_unknown_effort(&n),
                _ => continue,
            };
            out.push(format!("  {line}"));
        }
    }
    guard_lines(report, &mut out);
    if let Some(compare) = reply.get("compare").filter(|c| c.is_object()) {
        compare_lines(compare, &mut out);
    }
    out.join("\n")
}
