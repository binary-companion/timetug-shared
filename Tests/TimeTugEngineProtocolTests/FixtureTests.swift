import Foundation
import Testing
import TimeTugEngine
@testable import TimeTugEngineProtocol

@Suite struct FixtureTests {
    private func json(_ data: Data) throws -> JSONValue { try JSONDecoder().decode(JSONValue.self, from: data) }
    private func envelope(_ name: String) throws -> Envelope {
        try JSONDecoder().decode(Envelope.self, from: Conformance.golden(name))
    }
    private func text(_ name: String) throws -> String {
        String(decoding: try Conformance.golden(name), as: UTF8.self)
    }

    /// Not in baseline mode: a type added since the baseline has no baseline fixture (additions are allowed).
    @Test func everyCatalogTypeHasAFixture() throws {
        guard !Conformance.isBaseline else { return }
        let names = Set(try Conformance.goldenFiles())
        for type in MessageCatalog.all {
            #expect(names.contains("\(type).json"), "missing fixture for \(type)")
        }
    }

    @Test func noOrphanFixtures() throws {
        let types = Set(MessageCatalog.all)
        for name in try Conformance.goldenFiles() where !name.hasPrefix("edge-") {
            #expect(types.contains(String(name.dropLast(5))), "\(name) is not a catalog type")
        }
        #expect(types.count == MessageCatalog.all.count, "duplicate type in the catalog")
    }

    @Test func fixtureTypeMatchesItsFileName() throws {
        for type in MessageCatalog.all {
            #expect(try envelope("\(type).json").type == type)
        }
    }

    @Test func everyFixtureRoundTripsAsParsedJSON() throws {
        for name in try Conformance.goldenFiles() where !name.hasPrefix("edge-") {
            let data = try Conformance.golden(name)
            let env = try JSONDecoder().decode(Envelope.self, from: data)
            let again = try JSONEncoder().encode(env)
            #expect(Conformance.matches(try json(again), fixture: try json(data)), "\(name)")
        }
    }

    /// The payload types of the contract: every fixture of a listed type decodes into its wire type and re-encodes
    /// to the same JSON (in baseline mode: to a superset of the baseline's, so an added field passes and a removed,
    /// renamed or retyped one fails).
    private static let payloadTypes: [(prefix: String, roundTrip: @Sendable (JSONValue) throws -> JSONValue)] = [
        ("ready", roundTrip(WireReady.self)),
        ("state", roundTrip(WireState.self)), ("edge-state-", roundTrip(WireState.self)),
        ("event.takeoverDue", roundTrip(WireTakeoverRequest.self)),
        ("event.widgetSnapshot", roundTrip(WireWidgetSnapshot.self)),
        ("event.diagnostic", roundTrip(WireDiagnostic.self)), ("edge-event-diagnostic-", roundTrip(WireDiagnostic.self)),
        ("edge-settings-patch-", roundTrip(WireSettingsPatch.self)),
        ("service.credentials.get", roundTrip(CredentialsKeyRequest.self)),
        ("service.credentials.remove", roundTrip(CredentialsKeyRequest.self)),
        ("service.credentials.set", roundTrip(CredentialsSetRequest.self)),
        ("service.credentials.updateRefreshToken", roundTrip(CredentialsUpdateRefreshTokenRequest.self)),
        ("service.http.send", roundTrip(HTTPSendRequest.self)),
        ("service.auth.authorize", roundTrip(AuthAuthorizeRequest.self)),
        ("service.auth.closeRedirect", roundTrip(AuthCloseRedirectRequest.self)),
        ("service.auth.promptCredentials", roundTrip(AuthPromptCredentialsRequest.self)),
        ("service.signIn.prepare", roundTrip(SignInPrepareRequest.self)),
        ("service.text.generate", roundTrip(TextGenerateRequest.self)),
    ]

    private static func roundTrip<T: Codable>(_ type: T.Type) -> @Sendable (JSONValue) throws -> JSONValue {
        { payload in
            let typed = try JSONDecoder().decode(T.self, from: JSONEncoder().encode(payload))
            return try JSONDecoder().decode(JSONValue.self, from: JSONEncoder().encode(typed))
        }
    }

    @Test func typedPayloadsRoundTrip() throws {
        var checked = 0
        for name in try Conformance.goldenFiles() {
            let stem = String(name.dropLast(5))
            guard let entry = Self.payloadTypes.first(where: { $0.prefix.hasSuffix("-") ? stem.hasPrefix($0.prefix) : stem == $0.prefix })
            else { continue }
            let payload = try #require(try envelope(name).payload, "\(name)")
            let again = try entry.roundTrip(payload)
            #expect(Conformance.matches(again, fixture: payload), "\(name) does not round-trip through its wire type")
            checked += 1
        }
        #expect(checked >= 15, "only \(checked) fixtures had a wire type")
    }

    @Test func coversAllowsAddedFieldsOnly() {
        let base = JSONValue.object(["a": .number(1), "list": .array([.object(["b": .string("x")])])])
        #expect(JSONValue.object(["a": .number(1), "new": .bool(true),
                                  "list": .array([.object(["b": .string("x"), "c": .null])])]).covers(base))
        #expect(!JSONValue.object(["list": .array([.object(["b": .string("x")])])]).covers(base))           // removed
        #expect(!JSONValue.object(["a": .string("1"), "list": .array([.object(["b": .string("x")])])]).covers(base))  // retyped
        #expect(!JSONValue.object(["a": .number(2), "list": .array([.object(["b": .string("x")])])]).covers(base))    // changed
        #expect(!JSONValue.object(["a": .number(1), "list": .array([])]).covers(base))                      // shortened
        #expect(!JSONValue.object(["a": .number(1), "list": .array([.object(["b": .string("x")]), .null])]).covers(base))
    }

    @Test func engineMessagesMatchTheirGoldenFixtures() throws {
        for (name, message) in GoldenMessages.all {
            let fromEngine = try json(try JSONEncoder().encode(message))
            let fromFile = try json(try Conformance.golden("\(name).json"))
            #expect(Conformance.matches(fromEngine, fixture: fromFile), "\(name) drifted from its fixture")
        }
    }

    @Test func stateFixtureDecodesIntoTheWireType() throws {
        let payload = try #require(try envelope("state.json").payload)
        let state = try JSONDecoder().decode(WireState.self, from: JSONEncoder().encode(payload))
        #expect(state.revision == 12)
        #expect(state.events.count == 3)
        #expect(state.events.contains { $0.isAllDay && $0.startDate == "2026-10-16" && $0.start == nil })
        #expect(state.events.contains { !$0.members.isEmpty })
        #expect(state.candidates.count == 1)
    }

    @Test func readyFixtureDecodes() throws {
        let payload = try #require(try envelope("ready.json").payload)
        let ready = try JSONDecoder().decode(WireReady.self, from: JSONEncoder().encode(payload))
        #expect(ready.protocolVersions == [1])
        #expect(!ready.capabilities.systemCalendar)
    }

    @Test func everyCommandFixtureIsAccepted() throws {
        let state = GoldenMessages.state
        // The fixtures name event ids from the golden state.
        let decoder = CommandDecoder(testing: true)
        for type in MessageCatalog.hostToEngine {
            let outcome = decoder.decode(try envelope("\(type).json"), state: state)
            if case .rejected(let d) = outcome { Issue.record(Comment(rawValue: "\(type) rejected: \(d.message)")) }
        }
    }

    @Test func unknownTypeFixtureIsRejectedNotThrown() throws {
        guard case .rejected(let d) = CommandDecoder(testing: false).decode(try envelope("edge-unknown-type.json"), state: .empty)
        else { Issue.record("not rejected"); return }
        #expect(d.message == "unknown command type")
    }

    @Test func versionTwoFixtureIsRejected() throws {
        guard case .rejected(let d) = CommandDecoder(testing: false).decode(try envelope("edge-version-2.json"), state: .empty)
        else { Issue.record("not rejected"); return }
        #expect(d.message == "unsupported protocol version")
    }

    @Test func unknownFieldFixtureIsIgnored() throws {
        let env = try envelope("edge-unknown-field.json")
        #expect(env.id == .string("cmd-y"))
        guard case .engine(.refresh) = CommandDecoder(testing: false).decode(env, state: .empty)
        else { Issue.record("not a refresh"); return }
    }

    @Test func settingsPatchFixtures() throws {
        let full = try #require(try envelope("edge-settings-patch-full.json").payload)
        let patch = try JSONDecoder().decode(WireSettingsPatch.self, from: JSONEncoder().encode(full))
        #expect(patch.leadTime == 60 && patch.enabled == false && patch.systemCalendarEnabled == false)
        var settings = EngineSettings()
        patch.apply(to: &settings)
        #expect(settings.takeover.hiddenCalendarKeys == ["a/2"])
        #expect(!settings.systemCalendarEnabled)

        var untouched = EngineSettings()
        let empty = try #require(try envelope("edge-settings-patch-empty.json").payload)
        try JSONDecoder().decode(WireSettingsPatch.self, from: JSONEncoder().encode(empty)).apply(to: &untouched)
        #expect(untouched == EngineSettings())
    }

    @Test func replyErrorFixturesCoverEveryKind() throws {
        for kind in [ReplyErrorKind.network, .cancelled, .notFound, .denied, .failed] {
            let parsed = WireReply.parse(try text("edge-reply-error-\(kind.rawValue).json"))
            #expect(parsed == .error(kind: kind, message: "\(kind.rawValue) example"))
        }
    }

    @Test func serviceRequestsDecodeIntoTheirDTOs() throws {
        func payload(_ type: String) throws -> Data {
            try JSONEncoder().encode(try #require(try envelope("\(type).json").payload))
        }
        let dec = JSONDecoder()
        let http = try dec.decode(HTTPSendRequest.self, from: payload("service.http.send"))
        #expect(http.method == "POST" && http.body != nil)
        #expect(try dec.decode(CredentialsSetRequest.self, from: payload("service.credentials.set")).secrets.count == 1)
        #expect(try dec.decode(CredentialsUpdateRefreshTokenRequest.self,
                               from: payload("service.credentials.updateRefreshToken")).token.isEmpty == false)
        #expect(try dec.decode(AuthPromptCredentialsRequest.self, from: payload("service.auth.promptCredentials")).fields.count == 2)
        #expect(try dec.decode(SignInPrepareRequest.self, from: payload("service.signIn.prepare")).help?.linkTitle == "Learn more")
        #expect(try dec.decode(TextGenerateRequest.self, from: payload("service.text.generate")).prompt.isEmpty == false)
        // Requests carry a numeric id the host passes back to tt_engine_reply.
        for type in MessageCatalog.services {
            guard case .number? = try envelope("\(type).json").id else { Issue.record(Comment(rawValue: "\(type) id")); continue }
        }
    }

    @Test func serviceRepliesDecodeIntoTheirDTOs() throws {
        func result(_ name: String) throws -> Data {
            guard case .result(let value) = WireReply.parse(try text(name)) else { throw CocoaError(.coderReadCorrupt) }
            return try JSONEncoder().encode(value)
        }
        let dec = JSONDecoder()
        #expect(try dec.decode(HTTPSendReply.self, from: result("edge-reply-http-send.json")).status == 200)
        #expect(try dec.decode(CredentialsGetReply.self, from: result("edge-reply-credentials-get.json")).secrets != nil)
        #expect(try dec.decode(CredentialsGetReply.self, from: result("edge-reply-credentials-get-none.json")).secrets == nil)
        #expect(try dec.decode(CredentialsSnapshotReply.self, from: result("edge-reply-credentials-snapshot.json")).snapshot != nil)
        #expect(try dec.decode(CredentialsUpdateRefreshTokenReply.self, from: result("edge-reply-credentials-update.json")).updated)
        #expect(try dec.decode(AuthBeginRedirectReply.self, from: result("edge-reply-auth-begin.json")).sessionID == "s-1")
        #expect(try dec.decode(AuthAuthorizeReply.self, from: result("edge-reply-auth-authorize.json")).url.hasPrefix("http://127.0.0.1"))
        #expect(try dec.decode(AuthPromptCredentialsReply.self, from: result("edge-reply-auth-prompt.json")).values.count == 2)
        #expect(try dec.decode(SignInLastValuesReply.self, from: result("edge-reply-signin-last.json")).values != nil)
        #expect(try dec.decode(TextAvailabilityReply.self, from: result("edge-reply-text-availability.json")).kind == "available")
        #expect(try dec.decode(TextGenerateReply.self, from: result("edge-reply-text-generate.json")).text == "yes")
    }
}
