import Foundation

/// The `notchling-delegate` skill: how an agent hands a job (an image, a video, a second-opinion review)
/// to another tool with `Notchling --delegate`. Written for Claude Code, Codex and Antigravity CLI, in
/// each tool's skills folder when that tool is installed, so any of them can ask the others.
enum DelegateSkill {
    static let name = "notchling-delegate"

    static func dir(_ tool: VaultSkill.Tool) -> URL {
        tool.dir.deletingLastPathComponent().appendingPathComponent(name)
    }

    static func update() {
        let binary = Bundle.main.bundlePath + "/Contents/MacOS/Notchling"
        for tool in VaultSkill.Tool.allCases where tool.installed {
            let file = dir(tool).appendingPathComponent("SKILL.md")
            let text = content(binary: binary, tool: tool)
            if (try? String(contentsOf: file, encoding: .utf8)) == text { continue }
            try? FileManager.default.createDirectory(at: dir(tool), withIntermediateDirectories: true)
            try? text.write(to: file, atomically: true, encoding: .utf8)
        }
    }

    static func content(binary: String, tool: VaultSkill.Tool) -> String {
        """
        ---
        name: notchling-delegate
        description: Use to hand a job to another AI tool on this Mac and get its result back — generate an image (Codex or Antigravity), get a second-opinion code review from a different model (Antigravity CLI with Gemini, or Codex with GPT), or any task the user wants another agent to do — with a chosen model and effort. Runs through Notchling, waits for the other tool to finish, prints its reply and the files it made.
        ---

        # Delegate to another agent

        ```sh
        '\(binary)' --delegate <codex|agy> [--model <m>] [--effort <e>] [--folder <dir>] [--out <dir>] [--write] [--session <id>] [--timeout <s>] [--json] -- '<prompt>'
        ```

        - `codex`: OpenAI Codex (GPT models). Its image tool makes images.
        - `agy`: Antigravity CLI (Gemini, also Claude and GPT-OSS models). Its image subagent makes images.
        - `--model`, `--effort`: pick both. `'\(binary)' --delegate-models` lists each tool's models and efforts. For agy, effort is low/medium/high (`xhigh`/`max` become high), and `gemini-4-argon-high` is read as model plus effort.
        - `--folder`: the workspace the other agent works in (default: the current folder). Name it for a review.
        - `--write`: let it change files in that folder. Without it, it only reads (Codex read-only sandbox; agy's permission requests are declined and listed). Use `--write` only when the job is to change files.
        - `--out <dir>`: copy the images, videos or audio it generated there. Without it, they are listed where the tool saved them.
        - `--session <id>`: continue that session (the id is in the output), for a follow-up on the same job.
        - Exit code 0 when it finished, 1 when it failed, 124 on timeout (default 1800 s). Nested delegation stops at two levels.

        It blocks until the other tool's turn ends; nothing polls. Run it in the background and carry on: \(tool.waitNote)

        Output: a header line (tool, model, effort, time, tokens), `Session:`, `Generated:` and `Copied to:` file paths, any `Declined` permissions, then the other agent's reply. Read files it names before relying on them, and tell the user which tool and model did the work.

        ## Examples

        ```sh
        '\(binary)' --delegate codex --out ./assets -- 'Generate a 1024x1024 app icon: a paper plane, flat, blue on white.'
        '\(binary)' --delegate agy --model gemini-4-argon --effort high --folder . -- 'Review the uncommitted changes (git diff HEAD) for bugs. List each with file:line and why. Do not change files.'
        '\(binary)' --delegate codex --model \(CodexModels.load().first?.slug ?? "<model>") --effort high --write --folder ./site -- 'Add a dark mode to styles.css.'
        ```

        Write the prompt so the other agent needs nothing from this conversation: the goal, the files, what to return. It sees the folder, not your context.
        \(tool.delegateNote)
        """
    }
}

private extension VaultSkill.Tool {
    var waitNote: String {
        switch self {
        case .claude: return "with Bash `run_in_background: true` you are told when it exits, then read its output. In the foreground give it the longest timeout (600000 ms); an image takes about a minute, a review several."
        case .codex: return "give it a long timeout; an image takes about a minute, a review several."
        case .antigravity: return "give the terminal tool a long timeout; an image takes about a minute, a review several."
        }
    }

    var delegateNote: String {
        switch self {
        case .claude:
            return ""
        case .codex:
            return """

            ## In Codex's sandbox

            The other tool needs the network and its own folders, so run `--delegate` with escalated permissions (outside the sandbox) and give the user a one-line reason.

            """
        case .antigravity:
            return """

            ## In Antigravity

            Run it through the terminal tool; with `--sandbox` ask the user to allow the command, since the other tool needs the network.

            """
        }
    }
}
