# GPUI 0.2.2, patched

This is the published `gpui` 0.2.2 crate (Apache-2.0, `LICENSE-APACHE`), with its examples and
tests left out of the manifest, used through `[patch.crates-io]` in the workspace `Cargo.toml`.

## The changes

The first is a crash fix, the second a small addition.

### Measure closure

`src/elements/text.rs`, `TextLayout::layout`: the measure closure now clones the element's runs
on every call (`let mut runs = base_runs.clone();`). Upstream truncated the captured runs in place
(`LineWrapper::truncate_line`), so when Taffy measured the same text a second time at another
width, as it does during a window resize, the already shortened runs no longer matched the newly
truncated text. A run boundary landed inside a multi-byte character (the "…" suffix, or
Vietnamese text) and `MacTextSystem::layout_line` panicked slicing the string. Every label cut
with `.elide()` (`notchling_kit::ui`) took this path. Crash reports `notchling-app-2026-11-11-0111*.ips`.

### Where the file picker opens

`PathPromptOptions` has a new `directory: Option<PathBuf>`, and the macOS `prompt_for_paths` sets it with `setDirectoryURL:` before opening the panel (`src/platform.rs`, `src/platform/mac/platform.rs`). Every other platform ignores it. Upstream 0.2.2 can't choose the folder a picker opens in; Notchling's SSH Key picker starts in `~/.ssh`. Callers that don't care pass `None`.

## Leaving the patch

When a newer `gpui` on crates.io fixes the first and offers a start folder for the picker, bump the version in `crates/app` and `crates/kit`,
remove `[patch.crates-io]` and delete this folder.
