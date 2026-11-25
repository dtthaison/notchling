# Third-party code in crates/embed

`src/modernbert.rs` is adapted from `src/models/modernbert.rs` in candle-transformers 0.11.1 (https://github.com/huggingface/candle), dual-licensed MIT OR Apache-2.0, used here under Apache-2.0 (`src/LICENSE-candle`).

Changed from the original:

- Imports name `candle_core` and `candle_nn`; comments removed; the masked-LM and classifier heads dropped.
- Weight keys have no `model.` prefix.
- The MLP gate follows `hidden_activation` from `config.json` (SiLU for granite-embedding-97m-multilingual-r2, GELU by default).
- The attention masks and the rotary tables take the weights' dtype, and a masked position holds that dtype's lowest finite value, so a half-precision load runs and gives the vector a full-precision load gives.
- Linear layers and attention go through `matmul`, which runs half precision through f32 on macOS, where candle's Accelerate backend has none; attention uses the fused `softmax_last_dim`.
- The encoder can be loaded without its word table (`ModernBert::load_encoder`) and run on rows gathered by the caller (`forward_embedded`).

Model weights are never committed or downloaded by a test.

`examples/eval/qwen3.rs` is adapted from `src/models/qwen3.rs` in candle-transformers 0.11.1, under the same terms (`src/LICENSE-candle`). It is in the eval example only and never in the helper binary. Changed from the original: the `flash-attn` cargo feature and its CUDA path dropped (the CPU attention stays), the causal-LM head dropped, and the file holds no comments.

## Model cards checked

Read by hand for P4a; the maintainer decides, and P4b confirms. Nothing here is a legal finding.

- `ibm-granite/granite-embedding-97m-multilingual-r2` at `835ad14087e140460703cf0fae09f97d469d65c2` (https://huggingface.co/ibm-granite/granite-embedding-97m-multilingual-r2/blob/835ad14087e140460703cf0fae09f97d469d65c2/README.md): front matter `license: apache-2.0`; Model Details "License: Apache 2.0". The card does not mention Gemma or any other terms, and has no tokenizer attribution section. It says the model is "derived from the 311M model via layer pruning (22 to 12 layers) and vocabulary selection (262K to 180K tokens), using a compact tokenizer purpose-trained for multilingual coverage".
- `ibm-granite/granite-embedding-311m-multilingual-r2`, the model the 97M card names as its parent, at `44399559930365213510b1ee2eb15ded83374f0e` (https://huggingface.co/ibm-granite/granite-embedding-311m-multilingual-r2/blob/44399559930365213510b1ee2eb15ded83374f0e/README.md): `license: apache-2.0`, but its "Tokenizer Attribution" section says the 262K tokenizer "is derived from the Gemma 3 tokenizer" and that "use of the Gemma tokenizer is subject to the Gemma Terms of Use" (https://ai.google.dev/gemma/terms). The 97M model's 180K vocabulary is selected from that 262K one, so whether the Gemma terms reach the 97M tokenizer is open. The 97M card is silent on it.
- `sentence-transformers/static-similarity-mrl-multilingual-v1` at `b68f4122911bcffcd6e1f695f2d99cd6788972d8`: `license: apache-2.0`.
- `codefuse-ai/F2LLM-v2-330M` at `de3ccdfa5c7cd698e70f3161b8c211323ed41c15`: `license: apache-2.0`; base model `codefuse-ai/F2LLM-v2-0.6B-Preview-Pruned-330M` at `3fbd3b01ab49f08bb8f02203c42b4f4ecb14bc1f` (`license: apache-2.0`; "a pruned version of F2LLM-v2-0.6B-Preview"), which names `codefuse-ai/F2LLM-v2-0.6B-Preview`. Its tokenizer has Qwen-style special tokens (`<|im_end|>`, `<|endoftext|>`); the cards do not name tokenizer terms.

## Licence decision for the Standard model

Decided by the user, 2026-11-24, and not reopened here. Granite 97M (`ibm-granite/granite-embedding-97m-multilingual-r2`) is used as Apache 2.0, the licence on its model card. Its tokenizer comes from the Gemma 3 tokenizer (the 311M parent card says so, and the 97M vocabulary is selected from it), so the model falls under the Gemma Terms of Use (https://ai.google.dev/gemma/terms) as well. The download page in Settings and `docs/policies/privacy.md` name the Gemma 3 tokenizer origin and the Gemma Terms of Use in one plain line. The manifest (`crates/engine/data/models.json`) lists the file licence as Apache-2.0, the licence of the files themselves, and its test checks every entry against `deny.toml`.
