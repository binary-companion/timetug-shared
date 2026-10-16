import Foundation
import Testing
import TimeTugEngineProtocol

/// The conformance scenarios through the C functions. The C# runner (phase 4, Tasks 11 and 12) runs the same files
/// against the native library on Windows.
@Suite(.timeLimit(.minutes(2))) struct ScenarioTests {
    static let scenarioNames: [String] = {
        ((try? ScenarioRunner.scenarioURLs()) ?? []).map { $0.deletingPathExtension().lastPathComponent }
    }()

    @Test(arguments: scenarioNames) func scenarioPasses(_ name: String) async throws {
        let url = ScenarioRunner.scenarioDirectory.appendingPathComponent("\(name).json")
        let result = try await ScenarioRunner(url: url).run()
        #expect(result.failures.isEmpty, "\(name): \(result.failures.joined(separator: "\n"))")
    }

    @Test func runnerReportsAMismatch() async {
        let bad = JSONValue.object([
            "name": .string("bad"),
            "config": .object(["v": .number(1), "platform": .string("linux"), "testing": .bool(true)]),
            "steps": .array([
                .object(["send": .object(["type": .string("start")])]),
                .object(["expect": .object(["events": .array([.object(["type": .string("nope")])])])]),
            ]),
        ])
        let result = await ScenarioRunner(scenario: bad, timeout: 0.5).run()
        #expect(result.failures.count == 1)
    }

    @Test func runnerReportsAStateMismatchAndStrayMessages() async {
        let bad = JSONValue.object([
            "config": .object(["v": .number(1), "platform": .string("linux"), "testing": .bool(true)]),
            "steps": .array([
                .object(["send": .object(["type": .string("start")])]),
                .object(["expect": .object(["state": .object(["takeoverVisible": .bool(true)])])]),
            ]),
        ])
        #expect(await ScenarioRunner(scenario: bad, timeout: 0.5).run().failures.count == 1)
        let stray = JSONValue.object([
            "config": .object(["v": .number(1), "platform": .string("linux"), "testing": .bool(true)]),
            "steps": .array([
                .object(["send": .object(["type": .string("start")])]),
                .object(["expect": .object(["noMoreMessagesFor": .number(0.3)])]),
            ]),
        ])
        #expect(await ScenarioRunner(scenario: stray, timeout: 0.5).run().failures.count == 1)
    }

    @Test func subsetMatching() {
        let actual = JSONValue.object([
            "type": .string("state"), "payload": .object(["a": .number(1), "list": .array([.number(1), .number(2)])]),
        ])
        #expect(ScenarioRunner.matches(.object(["type": .string("state")]), actual))
        #expect(ScenarioRunner.matches(.object(["payload": .object(["a": .number(1)])]), actual))
        #expect(!ScenarioRunner.matches(.object(["payload": .object(["a": .number(2)])]), actual))
        #expect(!ScenarioRunner.matches(.object(["payload": .object(["list": .array([.number(1)])])]), actual))
        #expect(ScenarioRunner.matches(.object(["missing": .null]), actual))
        #expect(!ScenarioRunner.matches(.object(["missing": .number(1)]), actual))
        let messages: [JSONValue] = [.object(["type": .string("a")]), .object(["type": .string("b")]),
                                     .object(["type": .string("a")])]
        #expect(ScenarioRunner.subsequence([.object(["type": .string("b")]), .object(["type": .string("a")])],
                                           in: messages, from: 0) == 3)
        #expect(ScenarioRunner.subsequence([.object(["type": .string("b")]), .object(["type": .string("b")])],
                                           in: messages, from: 0) == nil)
    }
}
