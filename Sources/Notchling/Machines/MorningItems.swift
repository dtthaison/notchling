import Foundation

enum MorningKind { case ready, needsYou, stuck }

/// The worker entry behind a morning item.
enum MorningEntry: Equatable {
    case ready(ReadyEntry)
    case needsYou(NeedsYouEntry)
    case stuck(StuckEntry)
}

/// One thing the overnight workers left for you: a branch to merge, a question to answer, or a job to retry.
struct MorningItem: Identifiable, Equatable {
    /// `<machine>/<repo>#<issue>`
    let id: String
    let kind: MorningKind
    let machine: String
    let repo: String
    let issue: Int
    let title: String
    let entry: MorningEntry
    /// The latest worker comment on the issue, with the hidden `<!-- sd:… -->` marks removed.
    let comment: String?
}

/// One issue as `gh issue list --json number,title,labels,comments` prints it.
struct GHIssue: Decodable, Equatable {
    var number = 0
    var title = ""
    var labels: [String] = []
    var comments: [Comment] = []

    struct Comment: Decodable, Equatable {
        var author = ""
        var body = ""
        var createdAt: Date?

        init(author: String = "", body: String = "", createdAt: Date? = nil) {
            self.author = author; self.body = body; self.createdAt = createdAt
        }

        private enum Keys: String, CodingKey { case author, body, createdAt }
        private enum AuthorKeys: String, CodingKey { case login }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: Keys.self)
            author = (try? c.nestedContainer(keyedBy: AuthorKeys.self, forKey: .author))
                .flatMap { try? $0.decode(String.self, forKey: .login) } ?? ""
            body = (try? c.decode(String.self, forKey: .body)) ?? ""
            createdAt = (try? c.decode(String.self, forKey: .createdAt)).flatMap(StatusDates.parse)
        }
    }

    init(number: Int = 0, title: String = "", labels: [String] = [], comments: [Comment] = []) {
        self.number = number; self.title = title; self.labels = labels; self.comments = comments
    }

    private enum Keys: String, CodingKey { case number, title, labels, comments }
    private enum LabelKeys: String, CodingKey { case name }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        number = try c.decode(Int.self, forKey: .number)
        title = (try? c.decode(String.self, forKey: .title)) ?? ""
        var names: [String] = []
        if var list = try? c.nestedUnkeyedContainer(forKey: .labels) {
            while !list.isAtEnd {
                if let l = try? list.nestedContainer(keyedBy: LabelKeys.self), let n = try? l.decode(String.self, forKey: .name) {
                    names.append(n)
                } else {
                    _ = try? list.decode(Skipped.self)
                }
            }
        }
        labels = names
        comments = (try? c.decode([Comment].self, forKey: .comments)) ?? []
    }

    /// Reads the JSON array `gh issue list --json …` prints; empty when it is not one.
    static func decodeList(_ text: String) -> [GHIssue] {
        (try? JSONDecoder().decode([GHIssue].self, from: Data(text.utf8))) ?? []
    }

    private struct Skipped: Decodable { init(from decoder: Decoder) throws {} }
}

enum MorningItems {
    static let workerMark = "<!-- sd:worker -->"

    /// Ready first, then Needs you, then Stuck. Within a group the oldest comes first: a ready branch by when it
    /// became ready, the others (the status carries no date) by issue number, which grows with time.
    /// `issues` is keyed by repo (`owner/name`).
    static func build(reports: [String: LastReport], issues: [String: [GHIssue]]) -> [MorningItem] {
        var ready: [(MorningItem, Date?)] = []
        var needs: [MorningItem] = []
        var stuck: [MorningItem] = []

        for machine in reports.keys.sorted() {
            guard let status = reports[machine]?.status else { continue }
            func make(_ kind: MorningKind, _ repo: String, _ n: Int, _ entry: MorningEntry) -> MorningItem {
                let issue = issues[repo]?.first { $0.number == n }
                return MorningItem(id: id(machine: machine, repo: repo, issue: n), kind: kind, machine: machine, repo: repo,
                                   issue: n, title: issue?.title ?? "#\(n)", entry: entry,
                                   comment: issue.flatMap { latestWorkerComment($0, machine: machine) })
            }
            for e in status.ready { ready.append((make(.ready, e.repo, e.issue, .ready(e)), e.readyAt)) }
            for e in status.needsYou { needs.append(make(.needsYou, e.repo, e.issue, .needsYou(e))) }
            for e in status.stuck { stuck.append(make(.stuck, e.repo, e.issue, .stuck(e))) }
        }

        let byIssue: (MorningItem, MorningItem) -> Bool = { ($0.issue, $0.machine, $0.repo) < ($1.issue, $1.machine, $1.repo) }
        ready.sort { a, b in
            switch (a.1, b.1) {
            case let (x?, y?) where x != y: return x < y
            case (_?, nil): return true
            case (nil, _?): return false
            default: return byIssue(a.0, b.0)
            }
        }
        return ready.map(\.0) + needs.sorted(by: byIssue) + stuck.sorted(by: byIssue)
    }

    static func id(machine: String, repo: String, issue: Int) -> String { "\(machine)/\(repo)#\(issue)" }

    /// A worker comment ends with the hidden `<!-- sd:worker -->` mark (trailing whitespace allowed) or starts
    /// with `<machine>: `.
    static func isWorkerComment(_ body: String, machine: String) -> Bool {
        var end = body.endIndex
        while end > body.startIndex, body[body.index(before: end)].isWhitespace { end = body.index(before: end) }
        return body[..<end].hasSuffix(workerMark) || body.hasPrefix("\(machine): ")
    }

    /// Removes every `<!-- sd:… -->` mark and the blank space around what is left; nil when nothing is left.
    static func cleanComment(_ body: String) -> String? {
        let text = body.replacingOccurrences(of: #"(?s)<!--\s*sd:.*?-->"#, with: "", options: .regularExpression)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return text.isEmpty ? nil : text
    }

    static func latestWorkerComment(_ issue: GHIssue, machine: String) -> String? {
        issue.comments.last { isWorkerComment($0.body, machine: machine) }.flatMap { cleanComment($0.body) }
    }

    /// The labels the worker puts on an issue it leaves for you.
    static let labels = ["sd:ready", "sd:needs-you", "sd:stuck"]

    /// Asks GitHub for the open issues under each label of each repo and merges them by issue number.
    /// A failing call leaves that label out; the items then show `#<n>` for a title.
    static func fetchIssues(repos: [String], runner: CommandRunning) async -> [String: [GHIssue]] {
        var all: [String: [GHIssue]] = [:]
        for repo in Set(repos).sorted() {
            var byNumber: [Int: GHIssue] = [:]
            for label in labels {
                let argv = [MachineTools.path("gh"), "issue", "list", "-R", repo, "--label", label,
                            "--json", "number,title,labels,comments", "--limit", "100"]
                let result = await runner.run(argv, cwd: nil, timeout: 30)
                guard result.code == 0 else { continue }
                for issue in GHIssue.decodeList(result.out) { byNumber[issue.number] = issue }
            }
            all[repo] = byNumber.values.sorted { $0.number < $1.number }
        }
        return all
    }
}

/// Where the tools a morning action runs live. `ProcessRunner` needs a full path, and a menu bar app's PATH
/// does not hold Homebrew.
enum MachineTools {
    static func path(_ name: String) -> String {
        for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
            let candidate = "\(dir)/\(name)"
            if FileManager.default.isExecutableFile(atPath: candidate) { return candidate }
        }
        return "/usr/bin/\(name)"
    }
}
