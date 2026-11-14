import Foundation
import Testing
import TimeTugKit

/// A @MainActor settings-less fake: the sign-in form of an app lives on the main actor.
final class MainActorAuthorization: AuthorizationInteraction {
    let prompts = Locked(0)
    func beginOauthRedirect() async throws -> OAuthRedirectHandle { throw HostError.Denied }
    func promptCredentials(fields: [CredentialField]) async throws -> [String: String] {
        await MainActor.run {
            MainActor.preconditionIsolated()
            prompts.update { $0 += 1 }
            return ["link": "https://feeds.example.com/team.ics"]
        }
    }
}

@Suite(.serialized) struct ReviewFocusTests {
    // Review focus 2: a host callback that throws or never returns.

    @Test func aThrowingHostIsAnErrorTheEngineSurvives() async throws {
        let host = Host()
        host.http.feed.set(feed(events: 1))
        let engine = try makeEngine(host, directory: temporaryDirectory("throwing"))
        await engine.start()
        host.authorization.promptMode.set(.throwing(.Denied))
        await engine.send(command: .addAccount(kindId: "icalsub"))
        #expect(await waitUntil("the sign-in to fail") { host.observer.summaries.get().last?.accountStatus.isWorking == false })
        #expect(engine.currentSummary().accounts.isEmpty)
        // The engine is intact: the next attempt works.
        host.authorization.promptMode.set(.works)
        await engine.send(command: .addAccount(kindId: "icalsub"))
        #expect(await waitUntil("the account") { engine.currentSummary().accounts.count == 1 })
        engine.shutdown()
    }

    @Test func aTransportFailureIsAFailedSignInNotACrash() async throws {
        let host = Host()
        host.http.mode.set(.throwing(.Network(detail: "offline")))
        let engine = try makeEngine(host, directory: temporaryDirectory("network"))
        await engine.start()
        await engine.send(command: .addAccount(kindId: "icalsub"))
        #expect(await waitUntil("the failed sign-in") { engine.currentSummary().accountStatus.errorMessage != nil })
        engine.shutdown()
    }

    @Test func aCallThatNeverReturnsIsCancelledByTheEngineAndNeverHangsIt() async throws {
        let host = Host()
        host.authorization.promptMode.set(.neverReturns)
        let engine = try makeEngine(host, directory: temporaryDirectory("never"))
        await engine.start()
        async let adding: Void = engine.send(command: .addAccount(kindId: "icalsub"))
        #expect(await waitUntil("the form to be shown") { host.authorization.promptStarted.get() == 1 })
        await engine.send(command: .cancelAuthorization)
        await adding   // returns: the engine dropped the call, nothing hangs
        #expect(await waitUntil("the host call to see its cancellation") { host.authorization.promptCancelled.get() == 1 })
        engine.shutdown()
    }

    // Review focus 3: the host object outliving the engine, or the reverse; no callback after shutdown returns.

    @Test func lifecycleLoopOf500() async throws {
        let directory = temporaryDirectory("loop")
        for index in 0..<500 {
            let host = Host()
            host.http.feed.set(feed(events: 1))
            let engine = try makeEngine(host, directory: directory)
            async let started: Void = engine.start()
            async let sent: Void = engine.send(command: .refresh)
            if index % 2 == 0 { engine.shutdown() }
            _ = await (started, sent)
            engine.shutdown()
            let calls = host.observer.calls.get()
            #expect(host.observer.calls.get() == calls)
        }
    }

    @Test func theObserverOutlivesTheEngineAndTheEngineIsReleasedWhileACallRuns() async throws {
        let host = Host()
        host.http.feed.set(feed(events: 1))
        // The only strong reference to the engine is held by the task that sends the command, so the engine is released
        // (and shut down by its drop) the moment that task ends, possibly while the observer is being called.
        let pending = Task.detached { [engine = try makeEngine(host, directory: temporaryDirectory("release"))] in
            await engine.start()
            await engine.send(command: .addAccount(kindId: "icalsub"))
        }
        await pending.value
        try await Task.sleep(nanoseconds: 100_000_000)
        let calls = host.observer.calls.get()
        try await Task.sleep(nanoseconds: 100_000_000)
        #expect(host.observer.calls.get() == calls)
    }

    // Review focus 4: a main-actor implementation must not deadlock while the main actor waits on the engine.

    @MainActor @Test func aMainActorFakeDoesNotDeadlockWhileTheMainActorAwaitsTheEngine() async throws {
        let host = Host()
        host.http.feed.set(feed(events: 1))
        let authorization = MainActorAuthorization()
        let interfaces = EngineHostInterfaces(credentials: host.credentials, authorization: authorization, signIn: FakeSignIn(), settings: host.settings, http: host.http, diagnostics: nil, textGenerator: nil, externalTokens: nil, systemCalendar: nil)
        let engine = try Engine.newForTesting(config: host.config(directory: temporaryDirectory("main")), host: interfaces, observer: host.observer, clockStart: clockStart)
        await engine.start()
        await engine.send(command: .addAccount(kindId: "icalsub"))   // awaited from the main actor
        #expect(authorization.prompts.get() == 1)
        #expect(await waitUntil("the account") { engine.currentSummary().accounts.count == 1 })
        engine.shutdown()
    }

    // Review focus 5: unusual Unicode and very long strings round-trip unchanged.

    @Test func unusualUnicodeAndLongStringsRoundTrip() async throws {
        let title = "e\u{0301} 😀 👩‍👩‍👧‍👦 日本語 \u{202E}rtl\u{202C} \u{200B}zero width"
        let long = String(repeating: "x😀é", count: 5_000)
        let host = Host()
        host.http.feed.set(feed(events: 2, title: { $0 == 0 ? title : long }, notes: long))
        let engine = try makeEngine(host, directory: temporaryDirectory("unicode"))
        await engine.start()
        await engine.send(command: .addAccount(kindId: "icalsub"))
        #expect(await waitUntil("both events") { engine.currentEvents().count == 2 })
        let titles = Set(engine.currentEvents().map(\.event.title))
        #expect(titles == [title, long])
        #expect(engine.currentEvents().allSatisfy { $0.event.notes == long })
        engine.shutdown()
    }
}
