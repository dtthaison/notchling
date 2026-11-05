import XCTest
@testable import ConsoleCore

final class SchemaParserTests: XCTestCase {
    func testPrismaModelsColumnsAndRelations() {
        let schema = """
        generator client { provider = "prisma-client-js" }
        enum Role { USER ADMIN }
        model User {
          id    Int     @id @default(autoincrement())
          email String  @unique
          role  Role    @default(USER)
          posts Post[]
        }
        // comment line
        model Post {
          id       Int    @id
          title    String
          tags     String[]
          author   User?  @relation(fields: [authorId], references: [id])
          authorId Int?
          @@index([authorId])
        }
        model Membership {
          userId Int
          teamId Int
          user   User @relation(fields: [userId], references: [id])
          @@id([userId, teamId])
        }
        """
        let model = PrismaParser.parse(schema)
        XCTAssertEqual(model.tables.map(\.name), ["User", "Post", "Membership"])
        let user = model.tables[0]
        XCTAssertEqual(user.columns.map(\.name), ["id", "email", "role"], "relation list field is not a column")
        XCTAssertTrue(user.columns[0].isPrimaryKey)
        XCTAssertEqual(user.columns[2].type, "Role")
        let post = model.tables[1]
        XCTAssertEqual(post.columns.map(\.name), ["id", "title", "tags", "authorId"])
        XCTAssertEqual(post.columns[2].type, "String_list")
        XCTAssertTrue(post.columns[3].isForeignKey, "foreign key declared after the relation field")
        let membership = model.tables[2]
        XCTAssertTrue(membership.columns.allSatisfy(\.isPrimaryKey), "@@id marks the composite key")
        XCTAssertTrue(membership.columns[0].isForeignKey)
        XCTAssertEqual(model.relations, [
            ERRelation(from: "Post", to: "User", label: "author", optional: true),
            ERRelation(from: "Membership", to: "User", label: "user"),
        ])
    }

    func testSQLCreateTableWithInlineAndTableLevelConstraints() {
        let sql = """
        -- users first
        CREATE TABLE IF NOT EXISTS users (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          email VARCHAR(255) NOT NULL UNIQUE,
          created TEXT DEFAULT (datetime('now'))
        );
        /* orders */
        CREATE TABLE "orders" (
          id INTEGER,
          user_id INTEGER NOT NULL REFERENCES users(id),
          coupon_id INTEGER,
          total NUMERIC(10, 2),
          PRIMARY KEY (id),
          CONSTRAINT fk_coupon FOREIGN KEY (coupon_id) REFERENCES coupons (id),
          CHECK (total >= 0)
        );
        """
        let model = SQLSchemaParser.parse(sql)
        XCTAssertEqual(model.tables.map(\.name), ["users", "orders"])
        XCTAssertEqual(model.tables[0].columns.map(\.name), ["id", "email", "created"])
        XCTAssertTrue(model.tables[0].columns[0].isPrimaryKey)
        XCTAssertEqual(model.tables[0].columns[1].type, "VARCHAR")
        let orders = model.tables[1]
        XCTAssertEqual(orders.columns.map(\.name), ["id", "user_id", "coupon_id", "total"])
        XCTAssertTrue(orders.columns[0].isPrimaryKey, "table-level PRIMARY KEY")
        XCTAssertTrue(orders.columns[1].isForeignKey)
        XCTAssertTrue(orders.columns[2].isForeignKey, "named FOREIGN KEY constraint")
        XCTAssertEqual(orders.columns[3].type, "NUMERIC")
        XCTAssertEqual(model.relations.map(\.to).sorted(), ["coupons", "users"])
        XCTAssertEqual(model.relations.first { $0.to == "users" }?.optional, false, "NOT NULL means required")
    }

    func testDriftTablesWithDeclaredAndInferredRelations() {
        let dart = """
        import 'package:drift/drift.dart';
        class Wallets extends Table {
          TextColumn get id => text()();
          TextColumn get name => text()();
          @override
          Set<Column<Object>> get primaryKey => {id};
        }
        @DataClassName('TxRow')
        class Transactions extends Table {
          IntColumn get id => integer().autoIncrement()();
          TextColumn get walletId => text().nullable()();
          TextColumn get categoryId => text().references(Categories, #id)();
          IntColumn get amount => integer()
              .withDefault(const Constant(0))();
        }
        class Categories extends Table {
          TextColumn get id => text()();
          TextColumn get parentId => text().nullable()();
          @override
          Set<Column> get primaryKey => {id};
        }
        class Helper<Tbl extends Table> {}
        """
        let parsed = DriftParser.parse(dart)
        XCTAssertEqual(parsed.tables.map(\.name), ["Wallets", "Transactions", "Categories"], "a generic bound is not a table")
        XCTAssertEqual(parsed.tables[1].columns.map(\.name), ["id", "walletId", "categoryId", "amount"])
        XCTAssertTrue(parsed.tables[1].columns[0].isPrimaryKey, "autoIncrement")
        XCTAssertTrue(parsed.tables[0].columns[0].isPrimaryKey, "primaryKey set")
        XCTAssertEqual(parsed.tables[1].columns[3].type, "int")
        XCTAssertEqual(parsed.relations, [ERRelation(from: "Transactions", to: "Categories", label: "categoryId")])

        let withGuesses = DriftParser.inferRelations(parsed)
        let inferred = withGuesses.relations.filter(\.inferred)
        XCTAssertEqual(inferred.map { "\($0.from)->\($0.to):\($0.label)" }, ["Transactions->Wallets:walletId"],
                       "walletId points at Wallets; parentId has no table; categoryId is already declared")
    }

    func testMermaidOutputDropsUnknownTargetsAndMarksKeys() {
        let model = ERModel(
            tables: [ERTable(name: "orders", columns: [ERColumn(name: "id", type: "int", isPrimaryKey: true),
                                                       ERColumn(name: "user_id", type: "int", isForeignKey: true)])],
            relations: [ERRelation(from: "orders", to: "users", label: "user_id"),
                        ERRelation(from: "orders", to: "orders", label: "parent", inferred: true)])
        let text = model.mermaid()
        XCTAssertTrue(text.hasPrefix("erDiagram"))
        XCTAssertTrue(text.contains("int id PK"))
        XCTAssertTrue(text.contains("int user_id FK"))
        XCTAssertFalse(text.contains("users"), "target table was not parsed")
        XCTAssertTrue(text.contains("orders }o..o| orders : \"parent\""))
    }

    private func tree(_ files: [String: String]) throws -> URL {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("schema-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        for (path, content) in files {
            let url = root.appendingPathComponent(path)
            try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try Data(content.utf8).write(to: url)
        }
        return root
    }

    func testScannerSkipsDependencyAndBuildFolders() throws {
        let table = "CREATE TABLE t (id INTEGER PRIMARY KEY);"
        let root = try tree([
            "db/schema.sql": table,
            "prisma/schema.prisma": "model A {\n  id Int @id\n}\n",
            "lib/tables.dart": "class Notes extends Table { TextColumn get id => text()(); }",
            "lib/plain.dart": "class Plain {}",
            "node_modules/x/schema.sql": table,
            ".build/y.sql": table,
            "build/z.sql": table,
            ".dart_tool/w.sql": table,
            "ios/Pods/p.sql": table,
            ".git/hooks/h.sql": table,
        ])
        let result = SchemaScanner.scan(root: root)
        XCTAssertEqual(result.files.map(\.path).sorted(), ["db/schema.sql", "lib/tables.dart", "prisma/schema.prisma"])
        XCTAssertEqual(result.model.tables.count, 3)
    }

    func testSameTableInSeveralFilesIsMergedKeepingTheWidestVersion() throws {
        let root = try tree([
            "lib/tables.dart": "class Notes extends Table {\n  TextColumn get id => text()();\n  TextColumn get body => text()();\n}",
            "lib/old/schema_v1.dart": "class Notes extends Table {\n  TextColumn get id => text()();\n}",
        ])
        let result = SchemaScanner.scan(root: root)
        XCTAssertEqual(result.files.count, 2, "both files are listed as read")
        XCTAssertEqual(result.model.tables.count, 1)
        XCTAssertEqual(result.model.tables[0].columns.map(\.name), ["id", "body"])
    }

    func testScannerSkipsTestsGeneratedCodeAndSchemaSnapshots() throws {
        let table = "CREATE TABLE t (id INTEGER PRIMARY KEY);"
        let drift = "class Notes extends Table { TextColumn get id => text()(); }"
        let root = try tree([
            "lib/tables.dart": drift,
            "test/drift/generated/schema_v1.dart": drift,
            "tests/a.sql": table,
            "src/__tests__/b.sql": table,
            "lib/generated/c.dart": drift,
            "migrations-snapshots/d.sql": table,
            "drift_schemas/e.sql": table,
            "lib/db.g.dart": drift,
            "lib/model.freezed.dart": drift,
        ])
        let result = SchemaScanner.scan(root: root)
        XCTAssertEqual(result.files.map(\.path), ["lib/tables.dart"])
    }

    func testMigrationsCountOnlyWhenThereIsNoSchemaFile() throws {
        let migration1 = "CREATE TABLE users (id INTEGER PRIMARY KEY);"
        let migration2 = "CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT);"
        let onlyMigrations = try tree(["db/migrations/001.sql": migration1, "db/migrations/002.sql": migration2])
        let migrationsOnly = SchemaScanner.scan(root: onlyMigrations)
        XCTAssertEqual(migrationsOnly.files.map(\.path).sorted(), ["db/migrations/001.sql", "db/migrations/002.sql"], "real migrations matter when they are all there is")
        XCTAssertEqual(migrationsOnly.model.tables.first?.columns.count, 2, "latest version of the table wins")

        let both = try tree(["db/migrations/001.sql": migration1, "db/schema.sql": "CREATE TABLE accounts (id INTEGER);"])
        let preferred = SchemaScanner.scan(root: both)
        XCTAssertEqual(preferred.files.map(\.path), ["db/schema.sql"])
        XCTAssertEqual(preferred.model.tables.map(\.name), ["accounts"])
        XCTAssertTrue(preferred.warnings.contains { $0.contains("migrations") })
    }

    func testScannerStopsWhenTheTimeBudgetRunsOut() throws {
        var files: [String: String] = [:]
        for index in 0..<400 { files["other/f\(index).txt"] = "x" }
        files["sql/schema.sql"] = "CREATE TABLE t (id INT);"
        let result = SchemaScanner.scan(root: try tree(files), maxSeconds: -1)
        XCTAssertTrue(result.warnings.contains { $0.contains("giây") })
    }

    func testScannerStopsAtTheFileCap() throws {
        var files: [String: String] = [:]
        for index in 0..<(SchemaScanner.maxFiles + 15) { files["sql/t\(index).sql"] = "CREATE TABLE t\(index) (id INT);" }
        let result = SchemaScanner.scan(root: try tree(files))
        XCTAssertEqual(result.files.count, SchemaScanner.maxFiles)
        XCTAssertTrue(result.warnings.contains { $0.contains("200") })
    }
}
