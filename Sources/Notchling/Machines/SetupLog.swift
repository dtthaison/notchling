import Foundation

/// What a machine's output and results must not show: the vault values its steps use, and the admin password
/// of the Fix in flight. Built per call and never stored on the engine.
struct SetupMasks {
    /// Secret value → what shows instead (`[secret:<name>]`).
    var vault: [String: String] = [:]
    var adminPassword: String?

    static let adminMask = "[admin password]"
    private static let minimumMasked = 4

    func apply(_ text: String) -> String {
        guard !text.isEmpty else { return text }
        var text = text
        if !vault.isEmpty {
            let redactor = Redactor(vault)
            var data = redactor.feed(Data(text.utf8))
            data.append(redactor.finish())
            text = String(decoding: data, as: UTF8.self)
        }
        // Plain replacement, from 4 characters up like `Redactor`: a shorter one would turn ordinary words of the log
        // into the mask and hint at the password. (`sudo -S -p ''` never echoes it.)
        if let pw = adminPassword, pw.utf8.count >= Self.minimumMasked { text = text.replacingOccurrences(of: pw, with: Self.adminMask) }
        return text
    }

    /// The longest string that is masked: a line may be cut that far from its end without splitting a secret.
    var longest: Int { max(vault.keys.map { $0.utf8.count }.max() ?? 0, adminPassword?.utf8.count ?? 0) }

    func apply(_ result: CheckResult) -> CheckResult {
        var r = result
        switch r.state {
        case .pass(let s): r.state = .pass(apply(s))
        case .fail(let s): r.state = .fail(apply(s))
        case .cantTell(let s): r.state = .cantTell(apply(s))
        }
        r.rows = r.rows.map { SubRow(name: apply($0.name), ok: $0.ok, detail: apply($0.detail)) }
        return r
    }

    /// Output a cut has made: masks it first, then drops the line the cut ran through, which can hold the start
    /// or the end of a value that no longer matches. A head cut (`ProcessRunner`'s 2 MiB limit keeps the end and
    /// puts a notice first) loses the first line after the notice; a tail cut (`head -c` on the machine) the last.
    func applyToCut(_ text: String, headCut: Bool, tailCut: Bool) -> String {
        guard headCut || tailCut else { return text }
        let notice = ProcessRunner.cutNotice
        var body = headCut ? String(text.dropFirst(notice.count)) : text
        body = apply(body)
        if headCut {
            if let nl = body.firstIndex(of: "\n") { body = String(body[body.index(after: nl)...]) } else { body = "" }
        }
        if tailCut {
            if let nl = body.lastIndex(of: "\n") { body = String(body[..<nl]) } else { body = "" }
        }
        return headCut ? notice + body : body
    }
}

/// Collects the output of one Fix. `add` fires from the stdout and stderr readers at once, and for a moment
/// after the run returned (an orphan holding the pipe). Every call takes the lock, appends to the line being
/// built *of its own pipe* (`OutputStreamTag.current`), masks and yields each complete line into one serial stream
/// the MainActor drains, so the order is kept and a value cut between two chunks of one pipe is whole again in
/// its line even when the other pipe wrote in between. Once `end()` ran, nothing more gets in.
final class SetupFixLog: @unchecked Sendable {
    private let lock = NSLock()
    private let masks: SetupMasks
    private let continuation: AsyncStream<String>.Continuation
    /// The unfinished line of stdout and of stderr.
    private var partial: [OutputChannel: String] = [.stdout: "", .stderr: ""]
    private var ended = false
    /// A line this long without a newline is let out in pieces (a progress bar, a binary dump).
    private static let longLine = 4096

    init(masks: SetupMasks, continuation: AsyncStream<String>.Continuation) {
        self.masks = masks
        self.continuation = continuation
    }

    func add(_ chunk: String) {
        let channel = OutputStreamTag.current
        lock.lock()
        defer { lock.unlock() }
        guard !ended, !chunk.isEmpty else { return }
        var lines = ((partial[channel] ?? "") + chunk).components(separatedBy: "\n")
        var rest = lines.removeLast()
        for line in lines { emit(line) }
        if rest.count > Self.longLine {
            // mask first, cut afterwards: a cut through a secret would leave a fragment that no longer matches.
            // The end stays back, so a secret that is still arriving is whole in the next piece.
            let masked = masks.apply(rest)
            let keep = min(max(masks.longest, 1), masked.count)
            let cut = masked.index(masked.endIndex, offsetBy: -keep)
            emit(String(masked[..<cut]))
            rest = String(masked[cut...])
        }
        partial[channel] = rest
    }

    func end() {
        lock.lock()
        defer { lock.unlock() }
        guard !ended else { return }
        ended = true
        for channel in [OutputChannel.stdout, .stderr] {
            if let rest = partial[channel], !rest.isEmpty { emit(rest) }
            partial[channel] = ""
        }
        continuation.finish()
    }

    private func emit(_ line: String) {
        var line = line
        if line.hasSuffix("\r") { line.removeLast() }
        continuation.yield(masks.apply(line))
    }
}
