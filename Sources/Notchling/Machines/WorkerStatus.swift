import Foundation

/// What `sd.py worker status --json` prints (sd 1.19.1; `Worker.status()` in sdlib/worker.py is the contract).
/// A machine never sends this to us: the poller reads it over SSH. It decodes leniently on purpose, because the
/// worker on a machine can be older or newer than Notchling: an unknown key is ignored, and a missing key, a
/// `null` or a value of the wrong type becomes the field's default (nil, 0, "" or empty). One malformed entry in
/// `ready`, `needs_you` or `stuck` is dropped and the rest are kept. Decode with `WorkerStatus.decode(_:)`; it
/// reads snake_case keys and ISO-8601 dates with or without fractional seconds.
struct WorkerStatus: Decodable, Equatable {
    var machine = ""
    var version = ""
    /// Why the worker is paused (host load or the production health check), nil while it works.
    var paused: String?
    /// `github`, `claude`, `host` -> "ok" or "error: ..." / "limit" / "paused: ...".
    var health: [String: String] = [:]
    var current: Job?
    var ready: [ReadyEntry] = []
    var needsYou: [NeedsYouEntry] = []
    var stuck: [StuckEntry] = []
    var jobsTonight = 0
    var weeklyPct: Double?
    var tokenExpiresInDays: Int?
    /// When the worker process last started (a reboot or a restart), nil before its first start.
    var bootedAt: Date?
    /// When the worker's state file was first made.
    var since: Date?
    /// GitHub writes the worker gave up on. `status` itself does not print them in 1.19.1 (only `health.github`
    /// says "N outbox items failed"), so this is 0 unless a worker sends a list (counted) or a number.
    var outboxFailed = 0
    /// Issues queued for a kind no machine serves, e.g. `["ios"]`.
    var unrouted: [String] = []
    /// nil when the machine has no TestFlight poller.
    var testflight: TestFlightStatus?

    struct Job: Decodable, Equatable {
        var repo = ""
        var issue = 0
        var branch: String?
        var stage = ""
        var startedAt: Date?

        init(repo: String = "", issue: Int = 0, branch: String? = nil, stage: String = "", startedAt: Date? = nil) {
            self.repo = repo; self.issue = issue; self.branch = branch; self.stage = stage; self.startedAt = startedAt
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: Keys.self)
            repo = c.lenient(.repo) ?? ""
            issue = c.lenient(.issue) ?? 0
            branch = c.lenient(.branch)
            stage = c.lenient(.stage) ?? ""
            startedAt = c.lenient(.startedAt)
        }

        private enum Keys: String, CodingKey { case repo, issue, branch, stage, startedAt }
    }

    struct ReadyEntry: Decodable, Equatable {
        var repo = ""
        var issue = 0
        var branch = ""
        var commit = ""
        /// The worktree on the machine.
        var worktree = ""
        /// The verify evidence on the machine; empty from a worker that does not send it.
        var evidenceDir = ""
        var readyAt: Date?

        init(repo: String = "", issue: Int = 0, branch: String = "", commit: String = "", worktree: String = "",
             evidenceDir: String = "", readyAt: Date? = nil) {
            self.repo = repo; self.issue = issue; self.branch = branch; self.commit = commit
            self.worktree = worktree; self.evidenceDir = evidenceDir; self.readyAt = readyAt
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: Keys.self)
            repo = c.lenient(.repo) ?? ""
            issue = c.lenient(.issue) ?? 0
            branch = c.lenient(.branch) ?? ""
            commit = c.lenient(.commit) ?? ""
            worktree = c.lenient(.worktree) ?? ""
            evidenceDir = c.lenient(.evidenceDir) ?? ""
            readyAt = c.lenient(.readyAt)
        }

        private enum Keys: String, CodingKey { case repo, issue, branch, commit, worktree, evidenceDir, readyAt }
    }

    struct NeedsYouEntry: Decodable, Equatable {
        var repo = ""
        var issue = 0
        /// nil for an issue the worker's triage sent to you before any branch existed.
        var branch: String?
        /// nil for the same triage entries.
        var worktree: String?
        var reason = ""
        var question: String?
        var recommend: String?
        var options: [String] = []

        init(repo: String = "", issue: Int = 0, branch: String? = nil, worktree: String? = nil, reason: String = "",
             question: String? = nil, recommend: String? = nil, options: [String] = []) {
            self.repo = repo; self.issue = issue; self.branch = branch; self.worktree = worktree
            self.reason = reason; self.question = question; self.recommend = recommend; self.options = options
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: Keys.self)
            repo = c.lenient(.repo) ?? ""
            issue = c.lenient(.issue) ?? 0
            branch = c.lenient(.branch)
            worktree = c.lenient(.worktree)
            reason = c.lenient(.reason) ?? ""
            question = c.lenient(.question)
            recommend = c.lenient(.recommend)
            options = c.lenient(.options) ?? []
        }

        private enum Keys: String, CodingKey { case repo, issue, branch, worktree, reason, question, recommend, options }
    }

    struct StuckEntry: Decodable, Equatable {
        var repo = ""
        var issue = 0
        var branch: String?
        var worktree: String?
        var reason = ""
        var reportTail = ""

        init(repo: String = "", issue: Int = 0, branch: String? = nil, worktree: String? = nil, reason: String = "",
             reportTail: String = "") {
            self.repo = repo; self.issue = issue; self.branch = branch; self.worktree = worktree
            self.reason = reason; self.reportTail = reportTail
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: Keys.self)
            repo = c.lenient(.repo) ?? ""
            issue = c.lenient(.issue) ?? 0
            branch = c.lenient(.branch)
            worktree = c.lenient(.worktree)
            reason = c.lenient(.reason) ?? ""
            reportTail = c.lenient(.reportTail) ?? ""
        }

        private enum Keys: String, CodingKey { case repo, issue, branch, worktree, reason, reportTail }
    }

    /// The TestFlight feedback poller's state (`testflight.status()` in sdlib/testflight.py).
    struct TestFlightStatus: Decodable, Equatable {
        /// No new feedback for over 7 days.
        var quiet = false
        var lastNewAt: Date?
        /// Submissions skipped after GitHub refused them.
        var failed: Failed?
        /// The poller's state file is damaged.
        var error: String?

        struct Failed: Decodable, Equatable {
            var count = 0
            var newestId = ""
            var error = ""
            var at: Date?

            init(count: Int = 0, newestId: String = "", error: String = "", at: Date? = nil) {
                self.count = count; self.newestId = newestId; self.error = error; self.at = at
            }

            init(from decoder: Decoder) throws {
                let c = try decoder.container(keyedBy: Keys.self)
                count = c.lenient(.count) ?? 0
                newestId = c.lenient(.newestId) ?? ""
                error = c.lenient(.error) ?? ""
                at = c.lenient(.at)
            }

            private enum Keys: String, CodingKey { case count, newestId, error, at }
        }

        init(quiet: Bool = false, lastNewAt: Date? = nil, failed: Failed? = nil, error: String? = nil) {
            self.quiet = quiet; self.lastNewAt = lastNewAt; self.failed = failed; self.error = error
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: Keys.self)
            quiet = c.lenient(.quiet) ?? false
            lastNewAt = c.lenient(.lastNewAt)
            failed = c.lenient(.failed)
            error = c.lenient(.error)
        }

        private enum Keys: String, CodingKey { case quiet, lastNewAt, failed, error }
    }

    init() {}

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        machine = c.lenient(.machine) ?? ""
        version = c.lenient(.version) ?? ""
        paused = c.lenient(.paused)
        health = c.lenient(.health) ?? [:]
        current = c.lenient(.current)
        ready = c.lossyArray(.ready)
        needsYou = c.lossyArray(.needsYou)
        stuck = c.lossyArray(.stuck)
        jobsTonight = c.lenient(.jobsTonight) ?? 0
        weeklyPct = c.lenient(.weeklyPct)
        tokenExpiresInDays = c.lenient(.tokenExpiresInDays)
        bootedAt = c.lenient(.bootedAt)
        since = c.lenient(.since)
        outboxFailed = c.lenient(.outboxFailed) ?? c.lenient([Skip].self, .outboxFailed)?.count ?? 0
        unrouted = c.lenient(.unrouted) ?? []
        testflight = c.lenient(.testflight)
    }

    private enum Keys: String, CodingKey {
        case machine, version, paused, health, current, ready, needsYou, stuck, jobsTonight, weeklyPct
        case tokenExpiresInDays, bootedAt, since, outboxFailed, unrouted, testflight
    }

    /// Reads one `worker status --json` document; nil when it is not a JSON object at all.
    static func decode(_ data: Data) -> WorkerStatus? {
        try? decoder.decode(WorkerStatus.self, from: data)
    }

    // Keys are converted from snake_case, which also reaches the keys of `health` (github, claude, host: no
    // underscores today, so unchanged).
    private static let decoder: JSONDecoder = {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        d.dateDecodingStrategy = .custom { decoder in
            let text = try decoder.singleValueContainer().decode(String.self)
            guard let date = StatusDates.parse(text) else {
                throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "not an ISO-8601 date: \(text)"))
            }
            return date
        }
        return d
    }()
}

/// ISO-8601 with or without fractional seconds (the worker writes `2026-11-05T22:10:31Z`).
enum StatusDates {
    private static let plain = ISO8601DateFormatter()
    private static let fractional: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    static func parse(_ text: String) -> Date? {
        plain.date(from: text) ?? fractional.date(from: text)
    }
}

/// Accepts any JSON value, to count the elements of an array whose shape we do not read.
private struct Skip: Decodable {
    init(from decoder: Decoder) throws {}
}

/// An array element that decodes to nil instead of failing the whole array.
private struct Lossy<T: Decodable>: Decodable {
    let value: T?
    init(from decoder: Decoder) throws { value = try? T(from: decoder) }
}

private extension KeyedDecodingContainer {
    /// The value for `key`, or nil when it is missing, null or of another type.
    func lenient<T: Decodable>(_ key: Key) -> T? {
        (try? decodeIfPresent(T.self, forKey: key)) ?? nil
    }

    func lenient<T: Decodable>(_ type: T.Type, _ key: Key) -> T? {
        (try? decodeIfPresent(type, forKey: key)) ?? nil
    }

    func lossyArray<T: Decodable>(_ key: Key) -> [T] {
        let all: [Lossy<T>]? = lenient(key)
        return all?.compactMap(\.value) ?? []
    }
}
