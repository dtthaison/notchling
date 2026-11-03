import ConsoleCore
import Foundation
import Observation

/// Keeps the Visualize specs of the selected session current. Recomputes off the main thread, and only
/// when a transcript of the session (or the folder it points at) changed.
@MainActor
@Observable
final class VizStore {
    enum State: Equatable {
        case idle
        case loading
        case ready(VizBundle)
        case failed(String)
    }

    private(set) var state: State = .idle
    /// Session the current `state` belongs to.
    private(set) var stateSessionId: String?

    @ObservationIgnored private let engine = VizEngine()
    @ObservationIgnored private let queue = DispatchQueue(label: "local.agentconsole.viz", qos: .utility)
    @ObservationIgnored private var timer: DispatchSourceTimer?
    @ObservationIgnored private let lock = NSLock()
    @ObservationIgnored nonisolated(unsafe) private var target: (session: Session, cwd: String?)?
    @ObservationIgnored nonisolated(unsafe) private var lastFingerprint: String?
    @ObservationIgnored nonisolated(unsafe) private var forceRefresh = false
    /// QA only: adds fixture mermaid blocks to the Diagram tab when a real transcript has none.
    /// `-vizDemoMermaid YES` shows the valid fixture first; `-vizDemoMermaid invalid` shows the broken one first.
    @ObservationIgnored private let demoMermaid = UserDefaults.standard.string(forKey: "vizDemoMermaid")

    func start() {
        guard timer == nil else { return }
        let source = DispatchSource.makeTimerSource(queue: queue)
        source.schedule(deadline: .now(), repeating: .seconds(2), leeway: .milliseconds(200))
        source.setEventHandler { [weak self] in self?.tick() }
        source.resume()
        timer = source
    }

    /// Points the store at a session (nil clears it). Cheap; call whenever the selection or list changes.
    func setTarget(session: Session?, cwdOverride: String?) {
        let changedSession = target?.session.id != session?.id
        lock.lock()
        target = session.map { ($0, cwdOverride) }
        if changedSession { lastFingerprint = nil }
        lock.unlock()
        if changedSession {
            stateSessionId = session?.id
            state = session == nil ? .idle : .loading
            queue.async { [weak self] in self?.tick() }
        }
    }

    private(set) var entitySpecs: [String: VizSpec] = [:]
    @ObservationIgnored private var entityLoading: Set<String> = []

    /// Scans `cwd` for schemas in the background; the result lands in `entitySpecs[cwd]`.
    func requestEntity(cwd: String) {
        guard !entityLoading.contains(cwd) else { return }
        entityLoading.insert(cwd)
        let engine = self.engine
        queue.async { [weak self] in
            let spec = engine.entitySpec(cwd: cwd)
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    self?.entityLoading.remove(cwd)
                    if self?.entitySpecs[cwd] != spec { self?.entitySpecs[cwd] = spec }
                }
            }
        }
    }

    func retry() {
        lock.lock(); lastFingerprint = nil; lock.unlock()
        entitySpecs = [:]
        state = .loading
        queue.async { [weak self] in self?.tick() }
    }

    var bundle: VizBundle? {
        if case .ready(let bundle) = state { return bundle }
        return nil
    }

    private nonisolated func tick() {
        lock.lock()
        let current = target
        let previous = lastFingerprint
        lock.unlock()
        guard let current else { return }
        let fingerprint = engine.fingerprint(of: current.session, cwd: current.cwd)
        guard fingerprint != previous else { return }
        let bundle = engine.build(session: current.session, cwdOverride: current.cwd)
        lock.lock(); lastFingerprint = fingerprint; lock.unlock()
        let sessionId = current.session.id
        DispatchQueue.main.async { [weak self] in
            MainActor.assumeIsolated { self?.publish(bundle, for: sessionId) }
        }
    }

    private func publish(_ bundle: VizBundle, for sessionId: String) {
        guard stateSessionId == sessionId else { return }
        var result = bundle
        if let demoMermaid, !demoMermaid.isEmpty, demoMermaid != "NO", result.diagram.diagram == nil {
            var blocks = VizStore.demoDiagram.diagram?.blocks ?? []
            if demoMermaid == "invalid" { blocks.reverse() }
            result.diagram = VizSpec(kind: .diagram, diagram: DiagramSpec(blocks: blocks))
            result.signals.hasRecentMermaid = true
        }
        if state != .ready(result) { state = .ready(result) }
    }

    /// Fixture for checking the Diagram tab when no real transcript has a mermaid block (one valid, one not).
    static let demoDiagram = VizSpec(kind: .diagram, diagram: DiagramSpec(blocks: [
        DiagramBlock(id: "demo-1", label: "1. flowchart LR (demo fixture)", source: """
        flowchart LR
          Hook[Notchling hook] -->|requests/*.json| Store[SessionStore]
          Store --> UI[Agent Console]
          UI -->|responses/*.json| Hook
          Store --> Viz[VizEngine]
          Viz --> Web[viz/index.html]
        """, origin: "demo", time: nil),
        DiagramBlock(id: "demo-2", label: "2. graph TD, invalid (demo fixture)", source: "graph TD\n  A --> \n  B[unclosed", origin: "demo", time: nil),
    ]))
}
