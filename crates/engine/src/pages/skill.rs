pub const NAME: &str = "notchling-pages";

pub const TEMPLATE: &str = r#"---
name: notchling-pages
description: Use to find or read an Artifact page published earlier on this Mac — from a claude.ai/artifact/… or claude.ai/code/artifact/… link, a bare artifact id (like KVBHDXYvY8f1b1AZG6PbAR), or words from its title or project — and whenever an Artifact read returns 404 or the page belongs to another Claude account. Notchling keeps a local copy of every publish.
---

# Notchling pages

Notchling saves a local copy of every page published with the Artifact tool, whichever account published it. Artifacts are private to the login that published them, so after an account switch a link from another account returns 404; the local copy still reads.

## Find a page

```sh
'{binary}' page '<link | id | words>'
```

It prints the title, link, the account that published it, project, date, the local file, its supporting files, and the original source file if that still exists. Exit code 1 means nothing matched. `'{binary}' pages ['<words>']` lists every saved page (id, title, account), newest first.

## Read it

Read the "Local copy" file with the Read tool. An HTML copy is the published page wrapped in a doctype skeleton so it opens from disk; the page itself is inside `<body>`. Supporting files sit next to it, at their published paths.

An Artifact tool `read` of a link Notchling keeps doesn't reach claude.ai: Notchling's hook stops it and names the local copy instead (that isn't an error; read the file). Links it doesn't keep are read from claude.ai as usual. To get edits made on claude.ai after the last publish from this Mac, read with `path: "index.html"`; only the account that published the page can.

## Change it

claude.ai lets only the account that published a page ("Published by") publish to its link. Edit a copy of the original source file (or of the local copy), not the files in Notchling's folder, then publish with the page's link as `url`, whichever account is active.
- Publisher is the active account: Claude Code publishes it as usual.
- Publisher is another account Notchling has saved: Notchling's hook publishes it with that account's saved login and stops Claude Code's own call with a message starting "Done, not an error". The page is updated; don't publish it again. This covers a single HTML file, with or without `edits`. Supporting `files`, capabilities, a contract or a Markdown page aren't covered, and the publish is refused by claude.ai.
- If Notchling says the page has a newer version on claude.ai than the last publish from this Mac, ask the user before overwriting it (`force: true`).
- Publishing without `url` makes a new artifact with a new link, owned by the active account; the old link keeps the old version.
"#;

pub fn content(binary: &str) -> String {
    TEMPLATE.replace("{binary}", binary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skill_names_the_binary_and_the_commands_the_cli_has() {
        let text = content("/Apps/N");
        assert!(text.starts_with("---\nname: notchling-pages\n"));
        assert!(text.contains("'/Apps/N' page '<link | id | words>'"));
        assert!(text.contains("'/Apps/N' pages ['<words>']"));
        assert!(!text.contains("{binary}"));
        assert!(!text.contains("--page"), "the Swift spelling");
        assert_eq!(NAME, "notchling-pages");
    }
}
