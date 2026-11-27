use serde_json::{json, Value};

use super::{Capability, Cell, Group, Kind, Registry, Support, Tool};

fn escape(text: &str) -> String {
    text.replace('|', "\\|")
}

fn cell_text(cell: &Cell) -> String {
    let mut out = match cell.support {
        Support::Native { how } => format!("N: {how}"),
        Support::Fallback { flow, why } => format!("F ({}): {why}", flow.id()),
        Support::NotApplicable { why } => format!("NA: {why}"),
        Support::Gap { plan, meanwhile } => match meanwhile {
            Some(flow) => format!("GAP: {plan} (meanwhile {})", flow.id()),
            None => format!("GAP: {plan}"),
        },
    };
    if matches!(cell.support.kind(), Kind::Native | Kind::Fallback) && !cell.wired {
        out.push_str(" · unwired");
    }
    escape(&out)
}

pub fn markdown(registry: &Registry) -> String {
    let cells = registry.cells();
    let at = |tool: Tool, c: Capability| -> &Cell {
        cells
            .iter()
            .find(|x| x.tool == tool && x.capability == c)
            .expect("every tool and capability has a cell")
    };
    let mut out = String::new();
    out.push_str("# Provider matrix\n\n");
    out.push_str(
        "Generated from crates/engine/src/contract by the provider_matrix test; NOTCHLING_UPDATE_GOLDEN=1 rewrites it.\n\n",
    );
    out.push_str("| Tool | Native | Fallback | Not applicable | Gap | Unwired |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for tool in Tool::ALL {
        let of_tool = cells.iter().filter(|c| c.tool == tool);
        let count = |kind: Kind| of_tool.clone().filter(|c| c.support.kind() == kind).count();
        let unwired = of_tool
            .clone()
            .filter(|c| matches!(c.support.kind(), Kind::Native | Kind::Fallback) && !c.wired)
            .count();
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            tool.title(),
            count(Kind::Native),
            count(Kind::Fallback),
            count(Kind::NotApplicable),
            count(Kind::Gap),
            unwired
        ));
    }
    for group in Group::ALL {
        out.push_str(&format!("\n## {}\n\n", group.title()));
        out.push_str("| Capability | Claude Code | Codex | Antigravity |\n");
        out.push_str("| --- | --- | --- | --- |\n");
        for c in Capability::ALL.into_iter().filter(|c| c.group() == group) {
            out.push_str(&format!("| `{}` |", c.id()));
            for tool in Tool::ALL {
                out.push_str(&format!(" {} |", cell_text(at(tool, c))));
            }
            out.push('\n');
        }
    }
    out
}

pub fn json(registry: &Registry) -> Value {
    let cells: Vec<Value> = registry
        .cells()
        .iter()
        .map(|c| {
            json!({
                "tool": c.tool.name(),
                "capability": c.capability.id(),
                "support": c.support.kind().name(),
                "line": c.support.line(),
                "flow": c.support.flow().map(|f| f.id()),
                "wired": c.wired,
            })
        })
        .collect();
    json!({
        "tools": Tool::ALL.iter().map(|t| t.name()).collect::<Vec<_>>(),
        "cells": cells,
    })
}
