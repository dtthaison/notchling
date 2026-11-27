const CONTEXT: usize = 2;
const LIMIT: usize = 4_000_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Keep,
    Add,
    Del,
}

fn script<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(Op, &'a str)> {
    if old.len().saturating_mul(new.len()) > LIMIT {
        let mut out: Vec<(Op, &str)> = old.iter().map(|l| (Op::Del, *l)).collect();
        out.extend(new.iter().map(|l| (Op::Add, *l)));
        return out;
    }
    let (n, m) = (old.len(), new.len());
    let mut table = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if old[i] == new[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < n && j < m {
        if old[i] == new[j] {
            out.push((Op::Keep, old[i]));
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            out.push((Op::Del, old[i]));
            i += 1;
        } else {
            out.push((Op::Add, new[j]));
            j += 1;
        }
    }
    out.extend(old[i..].iter().map(|l| (Op::Del, *l)));
    out.extend(new[j..].iter().map(|l| (Op::Add, *l)));
    out
}

pub fn counts(old: &str, new: &str) -> (usize, usize) {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let ops = script(&old_lines, &new_lines);
    let added = ops.iter().filter(|o| o.0 == Op::Add).count();
    let removed = ops.iter().filter(|o| o.0 == Op::Del).count();
    (added, removed)
}

pub fn unified(path: &str, old: &str, new: &str) -> String {
    if old == new {
        return String::new();
    }
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let ops = script(&old_lines, &new_lines);
    let mut shown = vec![false; ops.len()];
    for (at, (op, _)) in ops.iter().enumerate() {
        if *op != Op::Keep {
            let from = at.saturating_sub(CONTEXT);
            let to = (at + CONTEXT + 1).min(ops.len());
            shown[from..to].iter_mut().for_each(|s| *s = true);
        }
    }
    let mut out = format!("--- {path}\n+++ {path}\n");
    let (mut old_at, mut new_at) = (1usize, 1usize);
    let mut at = 0;
    while at < ops.len() {
        if !shown[at] {
            match ops[at].0 {
                Op::Keep => {
                    old_at += 1;
                    new_at += 1;
                }
                Op::Del => old_at += 1,
                Op::Add => new_at += 1,
            }
            at += 1;
            continue;
        }
        let end = (at..ops.len()).find(|k| !shown[*k]).unwrap_or(ops.len());
        let old_count = ops[at..end].iter().filter(|o| o.0 != Op::Add).count();
        let new_count = ops[at..end].iter().filter(|o| o.0 != Op::Del).count();
        out.push_str(&format!(
            "@@ -{old_at},{old_count} +{new_at},{new_count} @@\n"
        ));
        for (op, line) in &ops[at..end] {
            let mark = match op {
                Op::Keep => ' ',
                Op::Add => '+',
                Op::Del => '-',
            };
            out.push(mark);
            out.push_str(line);
            out.push('\n');
            match op {
                Op::Keep => {
                    old_at += 1;
                    new_at += 1;
                }
                Op::Del => old_at += 1,
                Op::Add => new_at += 1,
            }
        }
        at = end;
    }
    out
}
