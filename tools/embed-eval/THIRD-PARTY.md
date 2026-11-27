# Third-party code in tools/embed-eval

`rs/src/modernbert.rs` and `rs/src/qwen3.rs` are copies of `src/models/modernbert.rs` and `src/models/qwen3.rs` from candle-transformers 0.11.1 (https://github.com/huggingface/candle), dual-licensed MIT OR Apache-2.0, used here under Apache-2.0 (`rs/src/LICENSE-candle`).

Changed from the originals:

- Both: imports name `candle_core` and `candle_transformers` instead of `candle` and `crate`; weight keys have no `model.` prefix; comments removed.
- `modernbert.rs`: the MLP gate uses SiLU instead of GELU (granite-embedding-97m-multilingual-r2).
- `qwen3.rs`: `clear_kv_cache` is public.

Model weights are never committed: `py/download.py` fetches them into `models/`, which git ignores.
