import Testing

@Suite struct CanaryFailingTests {
    @Test func deliberateFailure() {
        #expect(1 == 2, "deliberate canary failure: this must turn every job red")
    }
}
