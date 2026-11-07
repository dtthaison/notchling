import Foundation

/// Whether this process is a test run (`swift test` loads XCTest into it). Code that reaches the user's
/// screen or browser by default (opening a page, a form) checks it, so a test that builds a step with the
/// production defaults can't open anything.
enum TestRun {
    static let active: Bool = NSClassFromString("XCTestCase") != nil
}
