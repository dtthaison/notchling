use crate::commands::{acts, read, row, waits, Row, ACT_EXITS, READ_EXITS};

pub const ROWS: &[Row] = &[
    row(
    "facets",
    "notchling facets <sessions|pages> <tool|project|account|section|owner|kind|state> [--prefix <text>] [--limit <n>] [--json] [--socket <path>]",
    "the values a search facet can take in this session's organization, each with a label and a count, to use in a search filter; accounts and owners are masked",
    &[read("search.facets")],
    "none",
    true,
    READ_EXITS,
    "notchling-pages",
),
    row(
        "semantic",
        "notchling semantic [--json] [--socket <path>]",
        "whether search by meaning is on, its models, the budget and the last error, with no counts of what is indexed since they span every organization",
        &[read("embed.status")],
        "none",
        true,
        READ_EXITS,
        "notchling-pages",
    ),
    waits(row(
        "semantic download",
        "notchling semantic download <model> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "ask to download a Semantic Search model, which needs Semantic Search on; the user says yes on a card that names the model and its size, and the download runs in the background",
        &[acts("embed.request")],
        "request",
        true,
        ACT_EXITS,
        "notchling-pages",
    )),
];
