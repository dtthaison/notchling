use notchling_embed::{bench, convert, granite, serve, tokens::Tokens};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

const USAGE: &str = "usage: notchling-embed bench <model dir> [--threads N] [--budget F] [--data DIR]\n       notchling-embed convert-tokens <tokenizer.json> <out file>\n       notchling-embed convert-static <model dir> <out file>\n       notchling-embed serve --model <model dir>";

struct BenchArgs {
    dir: PathBuf,
    threads: usize,
    budget: f64,
    data: PathBuf,
}

fn parse_bench(args: &[String]) -> Option<BenchArgs> {
    let mut out = BenchArgs {
        dir: PathBuf::new(),
        threads: 1,
        budget: 0.5,
        data: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/embed-eval/data"),
    };
    let mut dir = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--threads" => out.threads = it.next()?.parse().ok().filter(|n| *n > 0)?,
            "--budget" => {
                out.budget = it
                    .next()?
                    .parse()
                    .ok()
                    .filter(|b: &f64| *b > 0.0 && *b <= 1.0)?
            }
            "--data" => out.data = PathBuf::from(it.next()?),
            s if s.starts_with("--") => return None,
            s if dir.is_none() => dir = Some(PathBuf::from(s)),
            _ => return None,
        }
    }
    out.dir = dir?;
    Some(out)
}

fn convert_tokens(args: &[String]) -> ExitCode {
    let [from, to] = args else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let tokens = match Tokens::from_tokenizer_json(&PathBuf::from(from)) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("convert-tokens: {e}");
            return ExitCode::from(1);
        }
    };
    match tokens.write_native_file(&PathBuf::from(to)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("convert-tokens: {e}");
            ExitCode::from(1)
        }
    }
}

fn convert_static(args: &[String]) -> ExitCode {
    let [dir, out] = args else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match convert::convert(&PathBuf::from(dir), &PathBuf::from(out)) {
        Ok(hash) => {
            println!("{hash}  {out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("convert-static: {e}");
            ExitCode::from(1)
        }
    }
}

fn serve_lines(args: &[String]) -> ExitCode {
    let [flag, dir] = args else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if flag != "--model" {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let model = match serve::load(&PathBuf::from(dir)) {
        Ok(m) => Arc::new(m),
        Err(e) => {
            eprintln!("serve: {e}");
            return ExitCode::from(1);
        }
    };
    let input = std::io::BufReader::new(std::io::stdin());
    match serve::serve(model, input, std::io::stdout(), &Instant::now) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("serve: {e}");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((command, rest)) = args.split_first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if command == "convert-tokens" {
        return convert_tokens(rest);
    }
    if command == "serve" {
        return serve_lines(rest);
    }
    if command == "convert-static" {
        return convert_static(rest);
    }
    if command != "bench" {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let Some(b) = parse_bench(rest) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    bench::limit_threads(b.threads);
    match bench::run(&b.dir, &b.data, b.budget) {
        Ok(r) => {
            println!(
                "model {} threads {} budget {}",
                granite::MODEL,
                b.threads,
                b.budget
            );
            println!("load {:.0} ms", r.load_ms);
            println!(
                "query ms: p50 {:.2} p95 {:.2}",
                r.query_p50_ms, r.query_p95_ms
            );
            println!(
                "chunks/s: {:.1}, at the budget {:.1}",
                r.chunks_per_s, r.chunks_per_s_at_budget
            );
            println!("rss {:.0} MB, peak {:.0} MB", r.rss_mb, r.peak_rss_mb);
            match serde_json::to_string(&r) {
                Ok(json) => println!("bench-json: {json}"),
                Err(e) => eprintln!("{e}"),
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("bench: {e}");
            ExitCode::from(1)
        }
    }
}
