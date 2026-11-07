import Foundation

/// What the shared viz bundle (viz/viz.js) renders. Encoded with sorted keys so the JSON is stable.
public struct VizSpec: Codable, Equatable, Sendable {
    public enum Kind: String, Codable, Sendable { case flow, diagram, entity, chart }

    public struct Message: Codable, Equatable, Sendable {
        public var title: String
        public var detail: String?
        public init(title: String, detail: String? = nil) { self.title = title; self.detail = detail }
    }

    public struct Failure: Codable, Equatable, Sendable {
        public var title: String
        public var message: String
        public var source: String?
        public init(title: String, message: String, source: String? = nil) {
            self.title = title; self.message = message; self.source = source
        }
    }

    public var kind: Kind
    public var empty: Message?
    public var error: Failure?
    public var flow: FlowSpec?
    public var diagram: DiagramSpec?
    public var entity: EntitySpec?
    public var chart: ChartSpec?

    public init(kind: Kind, empty: Message? = nil, error: Failure? = nil, flow: FlowSpec? = nil,
                diagram: DiagramSpec? = nil, entity: EntitySpec? = nil, chart: ChartSpec? = nil) {
        self.kind = kind; self.empty = empty; self.error = error
        self.flow = flow; self.diagram = diagram; self.entity = entity; self.chart = chart
    }

    public func json() throws -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        return String(decoding: try encoder.encode(self), as: UTF8.self)
    }
}

public struct FlowCall: Codable, Equatable, Sendable {
    public var tool: String
    public var target: String
    public var subagent: Bool
    public var edit: Bool
    /// Milliseconds since 1970, for JS `Date`.
    public var time: Double?
}

public struct FlowTurn: Codable, Equatable, Sendable {
    public var index: Int
    public var prompt: String
    public var calls: [FlowCall]
}

public struct FlowFile: Codable, Equatable, Sendable {
    public var path: String
    public var name: String
    public var count: Int
}

public struct FlowSpec: Codable, Equatable, Sendable {
    public var branch: String?
    public var diffStat: String?
    public var files: [FlowFile]
    public var turns: [FlowTurn]
    public var totalCalls: Int
}

public struct DiagramBlock: Codable, Equatable, Sendable {
    public var id: String
    public var label: String
    public var source: String
    public var origin: String
    public var time: Double?

    public init(id: String, label: String, source: String, origin: String, time: Double?) {
        self.id = id; self.label = label; self.source = source; self.origin = origin; self.time = time
    }
}

public struct DiagramSpec: Codable, Equatable, Sendable {
    /// Newest first.
    public var blocks: [DiagramBlock]

    public init(blocks: [DiagramBlock]) { self.blocks = blocks }
}

public struct EntityFile: Codable, Equatable, Sendable {
    public var path: String
    public var tables: Int
}

public struct EntityTable: Codable, Equatable, Sendable {
    /// Identifier as drawn in the diagram (see `ERModel.safeName`).
    public var id: String
    public var name: String
    public var columns: Int
}

/// One drawn relation, in the same order as the diagram's relation lines.
public struct EntityLink: Codable, Equatable, Sendable {
    public var from: String
    public var to: String
    public var inferred: Bool
}

public struct EntitySpec: Codable, Equatable, Sendable {
    public var mermaid: String
    public var tables: [EntityTable]
    public var links: [EntityLink]
    public var files: [EntityFile]
    public var tableCount: Int
    public var relationCount: Int
    public var inferredCount: Int
    public var warnings: [String]
}

public struct UsagePoint: Codable, Equatable, Sendable {
    public var time: Double
    public var input: Int
    public var output: Int
    public var cacheCreation: Int
    public var cacheRead: Int
}

public struct ChartCount: Codable, Equatable, Sendable {
    public var name: String
    public var count: Int
}

public struct ModelCount: Codable, Equatable, Sendable {
    public var model: String
    public var count: Int
}

public struct UsageTotals: Codable, Equatable, Sendable {
    public var input: Int
    public var output: Int
    public var cacheCreation: Int
    public var cacheRead: Int
}

public struct ChartSpec: Codable, Equatable, Sendable {
    public var tokens: [UsagePoint]
    public var totals: UsageTotals
    public var tools: [ChartCount]
    public var models: [ModelCount]
}
