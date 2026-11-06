import Foundation

/// Step 14: which repositories the issues token reaches and when it expires. Task 8 fills the bodies.
enum TokenScope {
    static func step(for machine: Machine, job: SDWorkerSettings) -> SetupStep {
        SetupStep(id: .tokenScope, title: "Token scope", needs: [], cadence: .slow,
                  probe: .notBuilt, fix: .notBuiltRun())
    }
}

/// GitHub's API over `URLSession`. The token stays in the Authorization header.
struct URLSessionFetcher: HTTPFetching {
    init() {}
    func get(_ url: URL, bearer: String) async -> HTTPAnswer { HTTPAnswer(status: 0, headers: [:], body: Data()) }
}
