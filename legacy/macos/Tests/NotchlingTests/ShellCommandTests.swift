import XCTest
@testable import Notchling

/// "!" commands copied from Claude's answer in a terminal, put back together before sending.
/// The wrapped examples are real ones from this Mac's transcripts.
final class ShellCommandNormalizeTests: XCTestCase {
    func n(_ s: String) -> String { ShellCommand.normalize(s) }

    func testDetection() {
        XCTAssertTrue(ShellCommand.isCommand("!git status"))
        XCTAssertTrue(ShellCommand.isCommand("  !ls"))
        XCTAssertFalse(ShellCommand.isCommand("run git status!"))
        XCTAssertFalse(ShellCommand.isCommand("/clear"))
        XCTAssertFalse(ShellCommand.isCommand(""))
    }

    func testOtherTextIsLeftAlone() {
        let text = "Please run:\n  git status\nand tell me"
        XCTAssertEqual(n(text), text)
    }

    func testWrappedInTheMiddleOfAPath() {
        XCTAssertEqual(n("! nohup bash /private/tmp/claude-501/x/92318983-3f1e-4de7-916c-25\n  3a34ed5267/scratchpad/run.sh"),
                       "!nohup bash /private/tmp/claude-501/x/92318983-3f1e-4de7-916c-253a34ed5267/scratchpad/run.sh")
    }

    func testWrappedAtASpace() {
        XCTAssertEqual(n("! ssh root@1.2.3.4 'nohup apt-get -y install gitlab-runner </dev/null >/root/r.log \n  2>&1 &'"),
                       "!ssh root@1.2.3.4 'nohup apt-get -y install gitlab-runner </dev/null >/root/r.log 2>&1 &'")
    }

    func testWrappedSeveralTimes() {
        XCTAssertEqual(n("!7z x \"/Users/me/keys to Son.7z\" \n  -o\"/Users/me/go/src/ling\n  eva\""),
                       "!7z x \"/Users/me/keys to Son.7z\" -o\"/Users/me/go/src/lingeva\"")
    }

    func testBackslashContinuationIsKept() {
        // The shell joins these itself; the indentation only separates words.
        let text = "!docker run \\\n  -p 80:80 \\\n  nginx"
        XCTAssertEqual(n(text), text)
    }

    func testSeparateCommandsStaySeparate() {
        XCTAssertEqual(n("!cd ~/go/src\nmake test"), "!cd ~/go/src\nmake test")
    }

    func testScriptBodiesKeepTheirWordsApart() {
        XCTAssertEqual(n("!for f in *.txt; do\n  echo $f\ndone"), "!for f in *.txt; do echo $f\ndone")
        XCTAssertEqual(n("!ls |\n  wc -l"), "!ls | wc -l")
    }

    func testHeredocsAreLeftAlone() {
        let text = "!cat <<EOF > a.txt\n  indented content\nEOF"
        XCTAssertEqual(n(text), text)
    }

    func testPromptsFencesAndTypography() {
        XCTAssertEqual(n("!$ git status --short"), "!git status --short")
        XCTAssertEqual(n("!```bash\ngit log -1\n```"), "!git log -1")
        XCTAssertEqual(n("!git commit -m \u{201C}fix\u{201D} \u{2014}amend"), "!git commit -m \"fix\" --amend")
        XCTAssertEqual(n("!echo\u{00A0}hi   \n"), "!echo hi")
    }

    func testLeadingSpacesAndBlankFirstLine() {
        XCTAssertEqual(n("  !  git status"), "!git status")
        XCTAssertEqual(n("!\n  git status"), "!git status")
    }

    func testRunningTwiceChangesNothing() {
        for text in ["!a\n  b", "!for f in x; do\n  echo $f\ndone", "!docker run \\\n  nginx", "!$ ls", "!cat <<EOF\n  x\nEOF"] {
            XCTAssertEqual(n(n(text)), n(text), text)
        }
    }
}
