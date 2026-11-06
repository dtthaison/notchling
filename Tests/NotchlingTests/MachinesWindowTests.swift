import XCTest
@testable import Notchling

/// The pure parts of the Machines window: what a header chip says and what a new question's peek says.
final class MachinesWindowTests: XCTestCase {
    private let now = Date(timeIntervalSince1970: 1_800_000_000)
    private let m1 = Machine(name: "m1", host: "m1", user: "u", sdPath: "/sd.py", fileVault: true, repos: [])

    private func fixtureStatus() throws -> WorkerStatus {
        let url = try XCTUnwrap(Bundle.module.url(forResource: "worker-status", withExtension: "json", subdirectory: "Fixtures"))
        return try XCTUnwrap(WorkerStatus.decode(try Data(contentsOf: url)))
    }

    func testChipShowsOnlineWithJobsWeekAndCurrentJob() throws {
        let report = LastReport(status: try fixtureStatus(), at: now)
        let chip = MachineChip.make(machine: m1, report: report, previous: nil, now: now)
        XCTAssertEqual(chip.word, "online")
        XCTAssertEqual(chip.tone, .good)
        XCTAssertEqual(chip.details, ["3 jobs tonight", "week 43%", "now dtthaison/lingeva-mono #214 · implementation"])
        XCTAssertNil(chip.error)
    }

    func testChipWarnsWhenTheTokenIsNearExpiry() throws {
        // the fixture's token has 14 days left
        let chip = MachineChip.make(machine: m1, report: LastReport(status: try fixtureStatus(), at: now), previous: nil, now: now)
        XCTAssertEqual(chip.tokenWarning, "GitHub token on vps-sing-1 expires in 14 days")
    }

    func testChipForAMachineNotPolledYet() {
        let chip = MachineChip.make(machine: m1, report: nil, previous: nil, now: now)
        XCTAssertEqual(chip.word, "checking…")
        XCTAssertEqual(chip.tone, .neutral)
    }

    func testChipForAFailedPollIsOfflineWithTheError() {
        let chip = MachineChip.make(machine: m1, report: LastReport(at: now, error: "timed out"), previous: nil, now: now)
        XCTAssertEqual(chip.word, "offline")
        XCTAssertEqual(chip.tone, .bad)
        XCTAssertEqual(chip.error, "timed out")
        XCTAssertTrue(chip.details.isEmpty)
    }

    func testChipForAPausedMachineIsAWarning() throws {
        var status = try fixtureStatus()
        status.paused = "production health check failed"
        let chip = MachineChip.make(machine: m1, report: LastReport(status: status, at: now), previous: nil, now: now)
        XCTAssertEqual(chip.word, MachineHealth.paused)
        XCTAssertEqual(chip.tone, .warn)
    }

    func testPeekTextPrefersTheQuestionThenTheReason() {
        XCTAssertEqual(MachineAlert.text(machine: "m1", entry: .init(issue: 207, reason: "asked", question: "Cap at 50 or 100?")),
                       "m1 #207: Cap at 50 or 100?")
        XCTAssertEqual(MachineAlert.text(machine: "m1", entry: .init(issue: 230, reason: "triage: needs a design call")),
                       "m1 #230: triage: needs a design call")
        XCTAssertEqual(MachineAlert.text(machine: "m1", entry: .init(issue: 5)), "m1 #5: needs you")
    }

    // MARK: Fields

    /// On macOS `TextField(onCommit:)` also fires when focus merely moves away, which posted a half-typed answer.
    /// A hosted-field check needs a window server; this scans the sources instead.
    func testNoFieldInMachinesPostsOnCommit() throws {
        let sources = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().appendingPathComponent("Sources/Notchling/Machines")
        let files = (try? FileManager.default.contentsOfDirectory(atPath: sources.path)) ?? []
        try XCTSkipIf(files.isEmpty, "the sources are not next to the tests")
        for file in files where file.hasSuffix(".swift") {
            let code = try String(contentsOf: sources.appendingPathComponent(file), encoding: .utf8)
                .split(separator: "\n", omittingEmptySubsequences: false)
                .filter { !$0.trimmingCharacters(in: .whitespaces).hasPrefix("//") }
                .joined(separator: "\n")
            XCTAssertFalse(code.contains("onCommit"), "\(file) uses onCommit")
        }
        let window = try String(contentsOf: sources.appendingPathComponent("MachinesWindow.swift"), encoding: .utf8)
        XCTAssertEqual(window.components(separatedBy: ".onSubmit(onSubmit)").count - 1, 1, "the note field submits on Return only")
    }
}
