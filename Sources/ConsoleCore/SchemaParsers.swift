import Foundation

public struct ERColumn: Equatable, Sendable {
    public var name: String
    public var type: String
    public var isPrimaryKey = false
    public var isForeignKey = false
}

public struct ERTable: Equatable, Sendable {
    public var name: String
    public var columns: [ERColumn]
}

/// `from` holds the foreign key and points at `to`.
public struct ERRelation: Hashable, Sendable {
    public var from: String
    public var to: String
    public var label: String
    public var optional = false
    /// Guessed from a column name such as `walletId`, not declared in the schema.
    public var inferred = false
}

public struct ERModel: Equatable, Sendable {
    public var tables: [ERTable] = []
    public var relations: [ERRelation] = []

    public init(tables: [ERTable] = [], relations: [ERRelation] = []) {
        self.tables = tables
        self.relations = relations
    }

    /// The identifier mermaid and the viz bundle use for a table or column.
    public static func safeName(_ name: String) -> String {
        let cleaned = String(name.map { $0.isLetter || $0.isNumber || $0 == "_" ? $0 : "_" })
        return cleaned.isEmpty ? "_" : cleaned
    }

    /// Relations that get drawn, in drawing order: unknown targets are dropped (mermaid would draw empty
    /// boxes) and repeats are removed. The viz bundle maps its i-th relation line to the i-th entry.
    public func drawnRelations() -> [ERRelation] {
        let known = Set(tables.map(\.name))
        var seen = Set<ERRelation>()
        return relations.filter { known.contains($0.from) && known.contains($0.to) && seen.insert($0).inserted }
    }

    /// mermaid `erDiagram`.
    public func mermaid() -> String {
        let safe = Self.safeName
        var lines = ["erDiagram"]
        for table in tables {
            lines.append("  \(safe(table.name)) {")
            for column in table.columns {
                var marks: [String] = []
                if column.isPrimaryKey { marks.append("PK") }
                if column.isForeignKey { marks.append("FK") }
                lines.append("    \(safe(column.type)) \(safe(column.name))" + (marks.isEmpty ? "" : " " + marks.joined(separator: ",")))
            }
            lines.append("  }")
        }
        for relation in drawnRelations() {
            let line = relation.inferred ? "}o..o|" : (relation.optional ? "}o--o|" : "}o--||")
            lines.append("  \(safe(relation.from)) \(line) \(safe(relation.to)) : \"\(relation.label.replacingOccurrences(of: "\"", with: ""))\"")
        }
        return lines.joined(separator: "\n")
    }
}

// MARK: - Prisma

public enum PrismaParser {
    public static func parse(_ text: String) -> ERModel {
        struct RawModel { var name: String; var lines: [String] }
        var raws: [RawModel] = []
        var current: RawModel?
        for rawLine in text.components(separatedBy: "\n") {
            let line = rawLine.trimmingCharacters(in: .whitespaces)
            if current == nil {
                if let name = capture(#"^model\s+(\w+)\s*\{"#, in: line) { current = RawModel(name: name, lines: []) }
            } else if line.hasPrefix("}") {
                raws.append(current!); current = nil
            } else if !line.isEmpty, !line.hasPrefix("//") {
                current!.lines.append(line)
            }
        }
        let modelNames = Set(raws.map(\.name))
        var model = ERModel()
        var pendingForeignKeys: [String: [String]] = [:]
        for raw in raws {
            var columns: [ERColumn] = []
            var compositeKeys: [String] = []
            for line in raw.lines {
                if line.hasPrefix("@@") {
                    if line.hasPrefix("@@id"), let inside = capture(#"\[([^\]]*)\]"#, in: line) {
                        compositeKeys = inside.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }
                    }
                    continue
                }
                let parts = line.split(whereSeparator: { $0 == " " || $0 == "\t" }).map(String.init)
                guard parts.count >= 2 else { continue }
                let fieldName = parts[0]
                var typeName = parts[1]
                let isOptional = typeName.hasSuffix("?")
                let isList = typeName.hasSuffix("[]")
                typeName = typeName.replacingOccurrences(of: "?", with: "").replacingOccurrences(of: "[]", with: "")
                let attributes = parts.dropFirst(2).joined(separator: " ")

                if modelNames.contains(typeName) {
                    // A relation field. Only the side that names `fields:` holds the foreign key.
                    if !isList, attributes.contains("@relation"), let fields = capture(#"fields:\s*\[([^\]]*)\]"#, in: attributes) {
                        let foreignKeys = fields.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }
                        for key in foreignKeys {
                            if let index = columns.firstIndex(where: { $0.name == key }) { columns[index].isForeignKey = true }
                        }
                        model.relations.append(ERRelation(from: raw.name, to: typeName, label: fieldName, optional: isOptional))
                        pendingForeignKeys[raw.name, default: []] += foreignKeys
                    }
                    continue
                }
                columns.append(ERColumn(name: fieldName, type: typeName + (isList ? "_list" : ""),
                                        isPrimaryKey: attributes.contains("@id")))
            }
            for key in compositeKeys {
                if let index = columns.firstIndex(where: { $0.name == key }) { columns[index].isPrimaryKey = true }
            }
            model.tables.append(ERTable(name: raw.name, columns: columns))
        }
        // Foreign key fields can be declared after the relation field that names them.
        for (table, keys) in pendingForeignKeys {
            guard let tableIndex = model.tables.firstIndex(where: { $0.name == table }) else { continue }
            for key in keys {
                if let index = model.tables[tableIndex].columns.firstIndex(where: { $0.name == key }) {
                    model.tables[tableIndex].columns[index].isForeignKey = true
                }
            }
        }
        return model
    }
}

// MARK: - SQL

public enum SQLSchemaParser {
    public static func parse(_ text: String) -> ERModel {
        let source = stripComments(text)
        var model = ERModel()
        let pattern = try! NSRegularExpression(
            pattern: #"create\s+(?:temp(?:orary)?\s+)?table\s+(?:if\s+not\s+exists\s+)?([`"\[]?[\w.]+[`"\]]?)\s*\("#,
            options: [.caseInsensitive])
        let ns = source as NSString
        for match in pattern.matches(in: source, range: NSRange(location: 0, length: ns.length)) {
            let name = unquote(ns.substring(with: match.range(at: 1)))
            let bodyStart = match.range.location + match.range.length
            guard let body = balancedBody(in: ns, from: bodyStart) else { continue }
            var columns: [ERColumn] = []
            var primaryKeys: [String] = []
            for part in splitTopLevel(body) {
                let trimmed = part.trimmingCharacters(in: .whitespacesAndNewlines)
                let upper = trimmed.uppercased()
                let tokens = trimmed.split(whereSeparator: { $0.isWhitespace }).map(String.init)
                guard let first = tokens.first else { continue }
                var constraint = upper
                if upper.hasPrefix("CONSTRAINT"), tokens.count > 2 { constraint = tokens.dropFirst(2).joined(separator: " ").uppercased() }

                if constraint.hasPrefix("PRIMARY KEY") {
                    primaryKeys += names(inParensOf: trimmed)
                } else if constraint.hasPrefix("FOREIGN KEY") {
                    let keys = names(inParensOf: trimmed)
                    if let target = capture(#"references\s+([`"\[]?[\w.]+[`"\]]?)"#, in: trimmed, options: [.caseInsensitive]) {
                        for key in keys {
                            if let index = columns.firstIndex(where: { $0.name == key }) { columns[index].isForeignKey = true }
                        }
                        model.relations.append(ERRelation(from: name, to: unquote(target), label: keys.joined(separator: ",")))
                    }
                } else if ["UNIQUE", "CHECK", "INDEX", "KEY", "FULLTEXT", "EXCLUDE"].contains(where: { constraint.hasPrefix($0) }) {
                    continue
                } else {
                    let columnName = unquote(first)
                    let type = tokens.count > 1 ? tokens[1].replacingOccurrences(of: ",", with: "") : "text"
                    var column = ERColumn(name: columnName, type: type.components(separatedBy: "(").first ?? type)
                    if upper.contains("PRIMARY KEY") { column.isPrimaryKey = true }
                    if let target = capture(#"references\s+([`"\[]?[\w.]+[`"\]]?)"#, in: trimmed, options: [.caseInsensitive]) {
                        column.isForeignKey = true
                        model.relations.append(ERRelation(from: name, to: unquote(target), label: columnName,
                                                          optional: !upper.contains("NOT NULL")))
                    }
                    columns.append(column)
                }
            }
            for key in primaryKeys {
                if let index = columns.firstIndex(where: { $0.name == key }) { columns[index].isPrimaryKey = true }
            }
            model.tables.append(ERTable(name: name, columns: columns))
        }
        return model
    }

    private static func stripComments(_ text: String) -> String {
        var result = text
        if let block = try? NSRegularExpression(pattern: #"/\*.*?\*/"#, options: [.dotMatchesLineSeparators]) {
            result = block.stringByReplacingMatches(in: result, range: NSRange(result.startIndex..., in: result), withTemplate: "")
        }
        if let line = try? NSRegularExpression(pattern: #"--[^\n]*"#) {
            result = line.stringByReplacingMatches(in: result, range: NSRange(result.startIndex..., in: result), withTemplate: "")
        }
        return result
    }

    private static func unquote(_ name: String) -> String {
        var cleaned = name.trimmingCharacters(in: CharacterSet(charactersIn: "`\"[]"))
        if let dot = cleaned.lastIndex(of: ".") { cleaned = String(cleaned[cleaned.index(after: dot)...]) }
        return cleaned
    }

    private static func names(inParensOf text: String) -> [String] {
        guard let inside = capture(#"\(([^)]*)\)"#, in: text) else { return [] }
        return inside.split(separator: ",").map { unquote($0.trimmingCharacters(in: .whitespaces)) }
    }

    /// Text between the "(" that ends at `from` and its matching ")".
    private static func balancedBody(in text: NSString, from: Int) -> String? {
        var depth = 1
        var index = from
        while index < text.length {
            let character = text.character(at: index)
            if character == 0x28 { depth += 1 }
            if character == 0x29 {
                depth -= 1
                if depth == 0 { return text.substring(with: NSRange(location: from, length: index - from)) }
            }
            index += 1
        }
        return nil
    }

    private static func splitTopLevel(_ body: String) -> [String] {
        var parts: [String] = []
        var depth = 0
        var current = ""
        for character in body {
            if character == "(" { depth += 1 }
            if character == ")" { depth -= 1 }
            if character == ",", depth == 0 { parts.append(current); current = "" } else { current.append(character) }
        }
        if !current.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { parts.append(current) }
        return parts
    }
}

// MARK: - Drift

public enum DriftParser {
    public static func parse(_ text: String) -> ERModel {
        var model = ERModel()
        let ns = text as NSString
        let header = try! NSRegularExpression(pattern: #"\bclass\s+(\w+)\s+extends\s+Table\b[^{]*\{"#)
        for match in header.matches(in: text, range: NSRange(location: 0, length: ns.length)) {
            let name = ns.substring(with: match.range(at: 1))
            let bodyStart = match.range.location + match.range.length
            guard let body = braceBody(in: ns, from: bodyStart) else { continue }

            var columns: [ERColumn] = []
            let columnPattern = try! NSRegularExpression(
                pattern: #"\b(\w+?)Column(?:<[^>]*>)?\s+get\s+(\w+)\s*=>\s*([^;]*);"#, options: [.dotMatchesLineSeparators])
            let bodyNS = body as NSString
            for column in columnPattern.matches(in: body, range: NSRange(location: 0, length: bodyNS.length)) {
                let type = bodyNS.substring(with: column.range(at: 1)).lowercased()
                let columnName = bodyNS.substring(with: column.range(at: 2))
                let definition = bodyNS.substring(with: column.range(at: 3))
                var entry = ERColumn(name: columnName, type: type, isPrimaryKey: definition.contains("autoIncrement()"))
                if let target = capture(#"references\(\s*(\w+)\s*,"#, in: definition) {
                    entry.isForeignKey = true
                    model.relations.append(ERRelation(from: name, to: target, label: columnName,
                                                      optional: definition.contains("nullable()")))
                }
                columns.append(entry)
            }
            if let keys = capture(#"primaryKey\s*=>\s*\{([^}]*)\}"#, in: body) {
                for key in keys.split(separator: ",").map({ $0.trimmingCharacters(in: .whitespaces) }) {
                    if let index = columns.firstIndex(where: { $0.name == key }) { columns[index].isPrimaryKey = true }
                }
            }
            model.tables.append(ERTable(name: name, columns: columns))
        }
        return model
    }

    /// Drift tables rarely declare `references(...)`: a `walletId` column next to a `Wallets` table is
    /// the usual shape. These guesses are flagged `inferred` and drawn dashed.
    public static func inferRelations(_ model: ERModel) -> ERModel {
        var result = model
        func normalized(_ name: String) -> String { name.lowercased().replacingOccurrences(of: "_", with: "") }
        func singular(_ name: String) -> String {
            let lower = normalized(name)
            if lower.hasSuffix("ies") { return String(lower.dropLast(3)) + "y" }
            if lower.hasSuffix("ses") { return String(lower.dropLast(2)) }
            if lower.hasSuffix("s") { return String(lower.dropLast()) }
            return lower
        }
        var bySingular: [String: String] = [:]
        for table in model.tables { bySingular[singular(table.name)] = table.name }
        let declared = Set(model.relations.map { "\($0.from).\($0.label)" })
        for table in model.tables {
            for column in table.columns where !column.isForeignKey && !declared.contains("\(table.name).\(column.name)") {
                let lower = normalized(column.name)
                guard lower.hasSuffix("id"), lower.count > 2 else { continue }
                let stem = String(lower.dropLast(2))
                guard let target = bySingular[stem], target != table.name else { continue }
                result.relations.append(ERRelation(from: table.name, to: target, label: column.name, optional: true, inferred: true))
            }
        }
        return result
    }

    private static func braceBody(in text: NSString, from: Int) -> String? {
        var depth = 1
        var index = from
        while index < text.length {
            let character = text.character(at: index)
            if character == 0x7B { depth += 1 }
            if character == 0x7D {
                depth -= 1
                if depth == 0 { return text.substring(with: NSRange(location: from, length: index - from)) }
            }
            index += 1
        }
        return nil
    }
}

func capture(_ pattern: String, in text: String, options: NSRegularExpression.Options = []) -> String? {
    guard let regex = try? NSRegularExpression(pattern: pattern, options: options) else { return nil }
    let ns = text as NSString
    guard let match = regex.firstMatch(in: text, range: NSRange(location: 0, length: ns.length)),
          match.numberOfRanges > 1, match.range(at: 1).location != NSNotFound else { return nil }
    return ns.substring(with: match.range(at: 1))
}

// MARK: - Scanner

public struct SchemaScanResult: Equatable, Sendable {
    public var model: ERModel
    public var files: [EntityFile]
    public var warnings: [String]
}

/// Finds schemas under a project folder. Skips build and dependency folders, tests, generated code and
/// schema snapshots, and stops at 200 files or 2 MB read. SQL under `migrations/` counts only when the
/// project has no schema file of its own, since a migration history repeats and alters the same tables.
public enum SchemaScanner {
    public static let skippedDirectories: Set<String> = [
        "node_modules", ".build", "build", ".dart_tool", "Pods", ".git",
        "test", "tests", "__tests__", "generated", "migrations-snapshots", "drift_schemas",
    ]
    public static let skippedFileSuffixes = [".g.dart", ".freezed.dart"]
    static let migrationDirectories: Set<String> = ["migrations", "migration"]
    public static let maxFiles = 200
    public static let maxBytes = 2_000_000
    static let maxVisited = 30_000

    public static func scan(root: URL, maxSeconds: TimeInterval = 5, fileManager: FileManager = .default) -> SchemaScanResult {
        let deadline = Date().addingTimeInterval(maxSeconds)
        var parsedFiles: [(file: EntityFile, model: ERModel, isMigration: Bool)] = []
        var files: [EntityFile] = []
        var warnings: [String] = []
        var bytesRead = 0
        var visited = 0

        guard let enumerator = fileManager.enumerator(
            at: root, includingPropertiesForKeys: [.isDirectoryKey, .fileSizeKey], options: [.skipsHiddenFiles, .skipsPackageDescendants]) else {
            return SchemaScanResult(model: ERModel(), files: [], warnings: ["Không đọc được thư mục \(root.path)."])
        }
        for case let url as URL in enumerator {
            visited += 1
            if visited > maxVisited { warnings.append("Dừng quét sau \(maxVisited) mục."); break }
            if visited % 100 == 0, Date() > deadline { warnings.append("Dừng quét sau \(Int(maxSeconds)) giây."); break }
            let values = try? url.resourceValues(forKeys: [.isDirectoryKey, .fileSizeKey])
            if values?.isDirectory == true {
                if skippedDirectories.contains(url.lastPathComponent) { enumerator.skipDescendants() }
                continue
            }
            let ext = url.pathExtension.lowercased()
            guard ["prisma", "sql", "dart"].contains(ext) else { continue }
            if skippedFileSuffixes.contains(where: { url.lastPathComponent.hasSuffix($0) }) { continue }
            let size = values?.fileSize ?? 0
            if files.count >= maxFiles { warnings.append("Đã đọc đủ \(maxFiles) file, bỏ phần còn lại."); break }
            if bytesRead + size > maxBytes { warnings.append("Đã đọc đủ 2 MB, bỏ phần còn lại."); break }
            if ext == "dart", size > 600_000 { continue }

            let text: String
            do {
                text = try String(contentsOf: url, encoding: .utf8)
            } catch {
                ConsoleLog.error("SchemaScanner.scan", url.path, "read failed: \(error)")
                continue
            }
            let parsed: ERModel
            switch ext {
            case "prisma": parsed = PrismaParser.parse(text)
            case "sql": parsed = SQLSchemaParser.parse(text)
            default:
                guard text.contains("extends Table") else { continue }
                parsed = DriftParser.parse(text)
            }
            bytesRead += size
            guard !parsed.tables.isEmpty else { continue }
            let path = relativePath(of: url, from: root)
            let inMigrations = path.split(separator: "/").dropLast().contains { migrationDirectories.contains(String($0)) }
            files.append(EntityFile(path: path, tables: parsed.tables.count))
            parsedFiles.append((EntityFile(path: path, tables: parsed.tables.count), parsed, inMigrations))
        }
        let hasSchemaFile = parsedFiles.contains { !$0.isMigration }
        var merged = ERModel()
        for entry in parsedFiles where !(hasSchemaFile && entry.isMigration) {
            merged.tables += entry.model.tables
            merged.relations += entry.model.relations
        }
        if hasSchemaFile, parsedFiles.contains(where: \.isMigration) {
            files = parsedFiles.filter { !$0.isMigration }.map(\.file)
            warnings.append("Bỏ qua SQL trong migrations/ vì đã có file schema.")
        }
        merged = deduplicated(merged)
        // Drift guesses run across the merged model so a table in one file can point at another.
        if files.contains(where: { $0.path.hasSuffix(".dart") }) { merged = DriftParser.inferRelations(merged) }
        return SchemaScanResult(model: merged, files: files, warnings: warnings)
    }

    /// The same table in several files (Drift keeps one generated schema snapshot per version) is one
    /// table: the version with the most columns wins, the first seen on a tie.
    static func deduplicated(_ model: ERModel) -> ERModel {
        var order: [String] = []
        var best: [String: ERTable] = [:]
        for table in model.tables {
            if let existing = best[table.name] {
                if table.columns.count > existing.columns.count { best[table.name] = table }
            } else {
                best[table.name] = table
                order.append(table.name)
            }
        }
        var seen = Set<ERRelation>()
        let relations = model.relations.filter { seen.insert($0).inserted }
        return ERModel(tables: order.compactMap { best[$0] }, relations: relations)
    }

    private static func relativePath(of url: URL, from root: URL) -> String {
        let base = root.resolvingSymlinksInPath().path
        let path = url.resolvingSymlinksInPath().path
        return path.hasPrefix(base + "/") ? String(path.dropFirst(base.count + 1)) : path
    }
}
