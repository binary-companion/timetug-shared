import CalendarCore
import Foundation
import Testing
import TimeTugEngine
@testable import TimeTugEngineC
import TimeTugEngineProtocol

/// `RemoteExternalTokenSource`: the host's account service (Windows' Web Account Manager) as the library's
/// `ExternalTokenSource`, over `service.token.*`. The mapping is the spec's table.
@Suite struct RemoteExternalTokenSourceTests {
    private let start = Date(timeIntervalSince1970: 1_760_000_000)
    private let account = ExternalAccount(id: "acct-1", email: "me@example.com", connectionID: "conn-1")
    private let scopes = ["User.Read", "Calendars.Read"]

    private func make() -> (RemoteExternalTokenSource, RequestTable, LockedBox<[Envelope]>, ManualEngineClock, LockedBox<[EngineDiagnostic]>) {
        let sent = LockedBox<[Envelope]>([])
        let diagnostics = LockedBox<[EngineDiagnostic]>([])
        let clock = ManualEngineClock(start: start)
        let table = RequestTable(send: { envelope in sent.mutate { $0.append(envelope) } }, clock: clock)
        let source = RemoteExternalTokenSource(table: table, diagnostics: { diagnostic in diagnostics.mutate { $0.append(diagnostic) } })
        return (source, table, sent, clock, diagnostics)
    }

    private func requestID(_ envelope: Envelope) throws -> UInt64 {
        guard case .number(let id)? = envelope.id else { throw CocoaError(.coderReadCorrupt) }
        return id
    }

    /// Waits for the first request, answers it with `reply`, and returns the request.
    @discardableResult
    private func answer(
        _ table: RequestTable, _ sent: LockedBox<[Envelope]>, with reply: String, index: Int = 0
    ) async throws -> Envelope {
        try #require(await poll("request \(index) is sent") { sent.value.count > index })
        let envelope = sent.value[index]
        table.fulfill(id: try requestID(envelope), reply: reply)
        return envelope
    }

    private func get(_ source: RemoteExternalTokenSource, force: Bool = false) -> Task<ExternalAccessToken, Error> {
        let account = account, scopes = scopes
        return Task { try await source.accessToken(for: account, scopes: scopes, forceRefresh: force) }
    }

    private func signIn(_ source: RemoteExternalTokenSource, account: ExternalAccount? = nil) -> Task<ExternalSignInResult, Error> {
        let scopes = scopes
        return Task { try await source.signIn(ExternalSignInRequest(account: account, email: account?.email, scopes: scopes)) }
    }

    // MARK: - service.token.get

    @Test func getOkReturnsTheTokenAndExpiry() async throws {
        let (source, table, sent, _, _) = make()
        let result = get(source, force: true)
        let request = try await answer(
            table, sent, with: #"{"v":1,"result":{"status":"ok","accessToken":"tok","expiresAt":"2026-10-09T09:53:20.000Z"}}"#)
        let token = try await result.value
        #expect(token.accessToken == "tok")
        #expect(token.expiresAt == WireDates.date("2026-10-09T09:53:20.000Z"))
        #expect(request.type == "service.token.get")
        #expect(request.payload?["connectionID"] == .string("conn-1"))
        #expect(request.payload?["accountID"] == .string("acct-1"))
        #expect(request.payload?["email"] == .string("me@example.com"))
        #expect(request.payload?["forceRefresh"] == .bool(true))
        #expect(request.payload?["scopes"] == .array([.string("User.Read"), .string("Calendars.Read")]))
    }

    @Test func getInteractionRequiredIsAuthExpired() async throws {
        let (source, table, sent, _, _) = make()
        let result = get(source)
        try await answer(table, sent, with: #"{"v":1,"result":{"status":"interactionRequired","reason":"signedOut"}}"#)
        await #expect(throws: SourceError.authExpired) { try await result.value }
    }

    @Test func getUnavailableIsInvalidResponse() async throws {
        let (source, table, sent, _, _) = make()
        let result = get(source)
        try await answer(table, sent, with: #"{"v":1,"result":{"status":"unavailable","reason":"brokerUnavailable"}}"#)
        await #expect(throws: SourceError.invalidResponse("Windows sign-in is not available")) { try await result.value }
    }

    @Test func getOkWithoutAccessTokenIsInvalidResponse() async throws {
        let (source, table, sent, _, _) = make()
        let result = get(source)
        try await answer(table, sent, with: #"{"v":1,"result":{"status":"ok","expiresAt":"2026-10-09T09:53:20.000Z"}}"#)
        await #expect(throws: SourceError.invalidResponse("the host's token answer was incomplete")) { try await result.value }
    }

    @Test func getOkWithAnEmptyAccessTokenIsInvalidResponse() async throws {
        let (source, table, sent, _, _) = make()
        let result = get(source)
        try await answer(table, sent, with: #"{"v":1,"result":{"status":"ok","accessToken":"","expiresAt":"2026-10-09T09:53:20.000Z"}}"#)
        await #expect(throws: SourceError.invalidResponse("the host's token answer was incomplete")) { try await result.value }
    }

    @Test func getOkWithGarbageExpiresAtIsInvalidResponse() async throws {
        let (source, table, sent, _, _) = make()
        let result = get(source)
        try await answer(table, sent, with: #"{"v":1,"result":{"status":"ok","accessToken":"tok","expiresAt":"tomorrow"}}"#)
        await #expect(throws: SourceError.invalidResponse("the host's token answer was incomplete")) { try await result.value }
    }

    @Test func getUnknownStatusIsInvalidResponse() async throws {
        let (source, table, sent, _, _) = make()
        let result = get(source)
        try await answer(table, sent, with: #"{"v":1,"result":{"status":"somethingNew","accessToken":"tok","expiresAt":"2026-10-09T09:53:20.000Z"}}"#)
        await #expect(throws: SourceError.invalidResponse("the host's token answer was incomplete")) { try await result.value }
    }

    @Test func getResultOfTheWrongShapeIsInvalidResponse() async throws {
        let (source, table, sent, _, _) = make()
        let result = get(source)
        try await answer(table, sent, with: #"{"v":1,"result":"nope"}"#)
        do {
            _ = try await result.value
            Issue.record("a result of the wrong shape was accepted")
        } catch let error as SourceError {
            guard case .invalidResponse = error else { Issue.record("expected invalidResponse, got \(error)"); return }
        }
    }

    @Test func getNetworkErrorIsSourceNetwork() async throws {
        let (source, table, sent, _, _) = make()
        let result = get(source)
        try await answer(table, sent, with: #"{"v":1,"error":{"kind":"network","message":"offline"}}"#)
        do {
            _ = try await result.value
            Issue.record("no error")
        } catch let error as SourceError {
            guard case .network = error else { Issue.record("expected network, got \(error)"); return }
        }
    }

    @Test func getCancelledErrorIsCancellation() async throws {
        let (source, table, sent, _, _) = make()
        let result = get(source)
        try await answer(table, sent, with: #"{"v":1,"error":{"kind":"cancelled"}}"#)
        await #expect(throws: CancellationError.self) { try await result.value }
    }

    @Test func getOtherErrorKindsAreInvalidResponseWithoutTheHostsText() async throws {
        for kind in ["failed", "denied", "notFound"] {
            let (source, table, sent, _, _) = make()
            let result = get(source)
            try await answer(table, sent, with: #"{"v":1,"error":{"kind":"\#(kind)","message":"secret-token-123"}}"#)
            do {
                _ = try await result.value
                Issue.record("no error for \(kind)")
            } catch let error as SourceError {
                guard case .invalidResponse(let text) = error else { Issue.record("expected invalidResponse, got \(error)"); continue }
                #expect(!text.contains("secret-token-123"))
            }
        }
    }

    @Test func getTimesOutAfterSixtySecondsOnTheEngineClock() async throws {
        let (source, _, sent, clock, _) = make()
        let result = get(source)
        try #require(await poll("the request is sent") { !sent.value.isEmpty })
        try #require(await poll("the timer sleeps") { clock.pendingSleeps == 1 })
        clock.advance(by: 59)
        #expect(clock.pendingSleeps == 1)
        clock.advance(by: 2)
        do {
            _ = try await result.value
            Issue.record("the request did not time out")
        } catch let error as SourceError {
            guard case .network = error else { Issue.record("expected network, got \(error)"); return }
        }
    }

    // MARK: - service.token.signIn

    @Test func signInHasNoTimeout() async throws {
        let (source, table, sent, clock, _) = make()
        let result = signIn(source)
        try #require(await poll("the request is sent") { !sent.value.isEmpty })
        clock.advance(by: 600)
        #expect(clock.pendingSleeps == 0)
        #expect(table.pendingCount == 1, "still waiting for the person")
        table.fulfill(
            id: try requestID(sent.value[0]),
            reply: #"{"v":1,"result":{"status":"ok","accountID":"acct-1","accessToken":"tok","expiresAt":"2026-10-09T09:53:20.000Z"}}"#)
        _ = try await result.value
    }

    @Test func signInOkReturnsAccountAndToken() async throws {
        let (source, table, sent, _, _) = make()
        let result = signIn(source, account: account)
        let request = try await answer(
            table, sent, with: #"{"v":1,"result":{"status":"ok","accountID":"acct-2","accessToken":"tok","expiresAt":"2026-10-09T09:53:20.000Z"}}"#)
        let signedIn = try await result.value
        #expect(signedIn.account.id == "acct-2")
        #expect(signedIn.token.accessToken == "tok")
        #expect(signedIn.token.expiresAt == WireDates.date("2026-10-09T09:53:20.000Z"))
        #expect(request.type == "service.token.signIn")
        #expect(request.payload?["accountID"] == .string("acct-1"))
        #expect(request.payload?["email"] == .string("me@example.com"))
        #expect(request.payload?["scopes"] == .array([.string("User.Read"), .string("Calendars.Read")]))
        guard case .string(let signInID)? = request.payload?["signInID"] else { Issue.record("no signInID"); return }
        #expect(!signInID.isEmpty)
    }

    @Test func firstSignInSendsNoAccount() async throws {
        let (source, table, sent, _, _) = make()
        let result = signIn(source)
        let request = try await answer(
            table, sent, with: #"{"v":1,"result":{"status":"ok","accountID":"a","accessToken":"t","expiresAt":"2026-10-09T09:53:20.000Z"}}"#)
        _ = try await result.value
        #expect(request.payload?["accountID"] == nil)
        #expect(request.payload?["email"] == nil)
    }

    @Test func signInIDsAreFreshPerCall() async throws {
        let (source, table, sent, _, _) = make()
        let reply = #"{"v":1,"result":{"status":"ok","accountID":"a","accessToken":"t","expiresAt":"2026-10-09T09:53:20.000Z"}}"#
        let first = signIn(source)
        try await answer(table, sent, with: reply, index: 0)
        _ = try await first.value
        let second = signIn(source)
        try await answer(table, sent, with: reply, index: 1)
        _ = try await second.value
        #expect(sent.value[0].payload?["signInID"] != sent.value[1].payload?["signInID"])
    }

    @Test func signInBlockedIsAdminApprovalRequired() async throws {
        let (source, table, sent, _, _) = make()
        let result = signIn(source)
        try await answer(table, sent, with: #"{"v":1,"result":{"status":"blocked","reason":"adminConsentRequired"}}"#)
        await #expect(throws: SourceError.invalidResponse("admin approval is required")) { try await result.value }
    }

    @Test func signInUnavailableIsInvalidResponse() async throws {
        let (source, table, sent, _, _) = make()
        let result = signIn(source)
        try await answer(table, sent, with: #"{"v":1,"result":{"status":"unavailable","reason":"brokerUnavailable"}}"#)
        await #expect(throws: SourceError.invalidResponse("Windows sign-in is not available")) { try await result.value }
    }

    @Test func signInOkWithoutAnAccountIDIsInvalidResponse() async throws {
        let (source, table, sent, _, _) = make()
        let result = signIn(source)
        try await answer(table, sent, with: #"{"v":1,"result":{"status":"ok","accessToken":"t","expiresAt":"2026-10-09T09:53:20.000Z"}}"#)
        await #expect(throws: SourceError.invalidResponse("the host's token answer was incomplete")) { try await result.value }
    }

    @Test func signInNetworkErrorIsSourceNetworkAndCancelledIsCancellation() async throws {
        let (source, table, sent, _, _) = make()
        let network = signIn(source)
        try await answer(table, sent, with: #"{"v":1,"error":{"kind":"network"}}"#, index: 0)
        do {
            _ = try await network.value
            Issue.record("no error")
        } catch let error as SourceError {
            guard case .network = error else { Issue.record("expected network, got \(error)"); return }
        }
        let cancelled = signIn(source)
        try await answer(table, sent, with: #"{"v":1,"error":{"kind":"cancelled"}}"#, index: 1)
        await #expect(throws: CancellationError.self) { try await cancelled.value }
    }

    @Test func cancellingTheSigningInTaskSendsCancelSignInWithTheSameSignInID() async throws {
        let (source, table, sent, _, _) = make()
        let result = signIn(source)
        try #require(await poll("the sign-in is sent") { !sent.value.isEmpty })
        let signInID = sent.value[0].payload?["signInID"]
        result.cancel()
        await #expect(throws: CancellationError.self) { try await result.value }
        try #require(await poll("the cancel is sent") { sent.value.count == 2 })
        #expect(sent.value[1].type == "service.token.cancelSignIn")
        #expect(sent.value[1].payload?["signInID"] == signInID)
        // The host answers the cancel; nothing is left pending.
        table.fulfill(id: try requestID(sent.value[1]), reply: #"{"v":1,"result":{}}"#)
        await poll("nothing pending") { table.pendingCount == 0 }
    }

    @Test func aFailingCancelSignInIsSilent() async throws {
        let (source, table, sent, _, _) = make()
        let result = signIn(source)
        try #require(await poll("the sign-in is sent") { !sent.value.isEmpty })
        result.cancel()
        _ = try? await result.value
        try #require(await poll("the cancel is sent") { sent.value.count == 2 })
        table.fulfill(id: try requestID(sent.value[1]), reply: #"{"v":1,"error":{"kind":"failed","message":"x"}}"#)
        await poll("nothing pending") { table.pendingCount == 0 }
    }

    @Test func lateSignInAnswerAfterCancelIsDropped() async throws {
        let (source, table, sent, _, _) = make()
        let result = signIn(source)
        try #require(await poll("the sign-in is sent") { !sent.value.isEmpty })
        let id = try requestID(sent.value[0])
        result.cancel()
        await #expect(throws: CancellationError.self) { try await result.value }
        // The dialog was answered after all: the engine ignores it, so no account results.
        table.fulfill(id: id, reply: #"{"v":1,"result":{"status":"ok","accountID":"a","accessToken":"t","expiresAt":"2026-10-09T09:53:20.000Z"}}"#)
        #expect(table.trackedIDCount == 0)
    }

    @Test func noCancelIsSentForASignInThatAlreadyAnswered() async throws {
        let (source, table, sent, _, _) = make()
        let result = signIn(source)
        try await answer(
            table, sent, with: #"{"v":1,"result":{"status":"ok","accountID":"a","accessToken":"t","expiresAt":"2026-10-09T09:53:20.000Z"}}"#)
        _ = try await result.value
        try await Task.sleep(nanoseconds: 50_000_000)
        #expect(sent.value.count == 1)
    }

    // MARK: - Diagnostics

    @Test func aDocumentedReasonIsLoggedAsACodeOnly() async throws {
        let (source, table, sent, _, diagnostics) = make()
        let result = get(source)
        try await answer(table, sent, with: #"{"v":1,"result":{"status":"interactionRequired","reason":"mfa"}}"#)
        _ = try? await result.value
        let messages = diagnostics.value.map(\.message)
        #expect(messages.count == 1)
        #expect(messages.first?.contains("mfa") == true)
    }

    @Test func anUndocumentedReasonIsDropped() async throws {
        let (source, table, sent, _, diagnostics) = make()
        let result = get(source)
        try await answer(
            table, sent, with: #"{"v":1,"result":{"status":"interactionRequired","reason":"me@example.com is locked out"}}"#)
        _ = try? await result.value
        #expect(diagnostics.value.isEmpty)
    }

    @Test func diagnosticsNeverContainTokenOrAccountIDOrEmail() async throws {
        let sentinels = ["SENTINEL-TOKEN-9", "acct-1", "me@example.com", "conn-1"]
        let replies = [
            #"{"v":1,"result":{"status":"ok","accessToken":"SENTINEL-TOKEN-9","expiresAt":"2026-10-09T09:53:20.000Z","reason":"consent"}}"#,
            #"{"v":1,"result":{"status":"interactionRequired","reason":"SENTINEL-TOKEN-9 acct-1 me@example.com"}}"#,
            #"{"v":1,"error":{"kind":"failed","message":"boom SENTINEL-TOKEN-9 acct-1 me@example.com conn-1"}}"#,
            #"{"v":1,"error":{"kind":"network","message":"SENTINEL-TOKEN-9"}}"#,
        ]
        for reply in replies {
            let (source, table, sent, _, diagnostics) = make()
            let result = get(source)
            try await answer(table, sent, with: reply)
            _ = try? await result.value
            let text = diagnostics.value.map { "\($0.category) \($0.message)" }.joined(separator: "\n")
            for sentinel in sentinels { #expect(!text.contains(sentinel), "\(sentinel) leaked into a diagnostic") }
        }
    }
}
