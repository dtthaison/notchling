import Foundation
import XCTest
@testable import Notchling

/// Answers a batched probe call the way a machine would: probe 0 (Reachable) says `reachable`, every other probe
/// says nothing and exits 1. Any other call (a Fix, a read) gets an empty success. Records every call and starts
/// nothing.
final class ReachableRunner: CommandRunning, @unchecked Sendable {
    private let lock = NSLock()
    private var recorded: [RecordedCall] = []
    let reachable: String
    /// When set, calls go here and the runner keeps none: a test that walks a model for a secret must not find
    /// the stdin of a delivery in the runner the model holds.
    private let sink: (@Sendable (RecordedCall) -> Void)?

    init(reachable: String, sink: (@Sendable (RecordedCall) -> Void)? = nil) {
        self.reachable = reachable
        self.sink = sink
    }

    var calls: [RecordedCall] { lock.withLock { recorded } }

    func run(_ argv: [String], cwd: String?, timeout: TimeInterval) async -> CommandResult {
        await run(argv, cwd: cwd, env: [:], stdin: nil, timeout: timeout, onOutput: nil)
    }

    func run(_ argv: [String], cwd: String?, env: [String: String], timeout: TimeInterval) async -> CommandResult {
        await run(argv, cwd: cwd, env: env, stdin: nil, timeout: timeout, onOutput: nil)
    }

    func run(_ argv: [String], cwd: String?, env: [String: String], stdin: Data?, timeout: TimeInterval,
             onOutput: (@Sendable (String) -> Void)?) async -> CommandResult {
        let call = RecordedCall(argv: argv, env: env, stdin: stdin, cwd: cwd)
        if let sink { sink(call) } else { lock.withLock { recorded.append(call) } }
        guard let script = argv.last, script.contains("out=$( /bin/sh -c ") else {
            return CommandResult(code: 0, out: "", err: "")
        }
        let nonceRe = try! NSRegularExpression(pattern: "@@([0-9a-f]{16}) 0 %s@@")
        guard let m = nonceRe.firstMatch(in: script, range: NSRange(script.startIndex..., in: script)),
              let r = Range(m.range(at: 1), in: script) else { return CommandResult(code: 0, out: "", err: "") }
        let nonce = String(script[r])
        var out = ""
        for index in 0..<(script.components(separatedBy: "out=$( /bin/sh -c ").count - 1) {
            out += (index == 0 ? reachable : "") + "\n@@\(nonce) \(index) \(index == 0 ? 0 : 1)@@\n"
        }
        return CommandResult(code: 0, out: out, err: "")
    }
}

/// Whether `needle` is anywhere inside `value`'s stored properties, however deep (strings, optionals, enum payloads,
/// collections, `@Published` storage). Used to prove a secret is not kept.
func holds(_ needle: String, in value: Any, depth: Int = 0, seen: inout Set<ObjectIdentifier>) -> Bool {
    if let s = value as? String { return s.contains(needle) }
    if let d = value as? Data { return String(decoding: d, as: UTF8.self).contains(needle) }
    guard depth < 8 else { return false }
    let mirror = Mirror(reflecting: value)
    if mirror.displayStyle == .class, let object = value as? AnyObject {
        guard seen.insert(ObjectIdentifier(object)).inserted else { return false }
    }
    var current: Mirror? = mirror
    while let m = current {
        for child in m.children where holds(needle, in: child.value, depth: depth + 1, seen: &seen) { return true }
        current = m.superclassMirror
    }
    return false
}

/// The registry as a window sees it: a state to load and every list it was asked to save.
final class TestRegistry: @unchecked Sendable {
    private let lock = NSLock()
    private var current: RegistryState
    private var writes: [[Machine]] = []
    var refuseSaves = false

    init(_ state: RegistryState) { current = state }
    convenience init(machines: [Machine]) { self.init(.loaded(machines, migrated: false)) }

    var state: RegistryState { lock.withLock { current } }
    var machines: [Machine] {
        if case .loaded(let list, _) = state { return list }
        return []
    }
    var saves: [[Machine]] { lock.withLock { writes } }

    func save(_ list: [Machine]) -> Bool {
        lock.withLock {
            if refuseSaves { return false }
            if case .unreadable = current { return false }
            writes.append(list)
            current = .loaded(list, migrated: false)
            return true
        }
    }
}
