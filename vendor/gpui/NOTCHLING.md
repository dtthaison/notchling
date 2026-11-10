# GPUI 0.2.2, patched

This is the published `gpui` 0.2.2 crate (Apache-2.0, `LICENSE-APACHE`), with its examples and
tests left out of the manifest, used through `[patch.crates-io]` in the workspace `Cargo.toml`.

## The one change

`src/elements/text.rs`, `TextLayout::layout`: the measure closure now clones the element's runs
on every call (`let mut runs = base_runs.clone();`). Upstream truncated the captured runs in place
(`LineWrapper::truncate_line`), so when Taffy measured the same text a second time at another
width, as it does during a window resize, the already shortened runs no longer matched the newly
truncated text. A run boundary landed inside a multi-byte character (the "…" suffix, or
Vietnamese text) and `MacTextSystem::layout_line` panicked slicing the string. Every label cut
with `.elide()` (`notchling_kit::ui`) took this path. Crash reports `notchling-app-2026-11-11-0111*.ips`.

## Leaving the patch

When a newer `gpui` on crates.io fixes this, bump the version in `crates/app` and `crates/kit`,
remove `[patch.crates-io]` and delete this folder.
