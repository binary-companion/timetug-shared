import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

private func waitUntil(_ condition: @Sendable () async -> Bool) async throws {
    for _ in 0..<250 {
        if await condition() { return }
        try await Task.sleep(for: .milliseconds(20))
    }
    Issue.record("condition not reached")
}

private let caldavFields = [
    CredentialField(key: "username", label: "User name"),
    CredentialField(key: "password", label: "Password", isSecret: true),
]

@Suite struct AccountManagerTests {
    @Test func addAccountStoresTheConnectionAndAppliesSources() async throws {
        let h = try AccountHarness(kinds: [FakeConnectorKind()])
        await h.manager.addAccount(kindID: "google")
        let status = await h.manager.status()
        #expect(status.accounts.map(\.connectionID) == ["1"])
        #expect(status.errorMessage == nil)
        #expect(!status.isWorking)
        let stored = await FileConnectionStore(url: h.accountsFile).connections()
        #expect(stored.map(\.connectionID) == ["1"])
        #expect(h.applied.get().last == ["eventkit", "google-1"])
    }

    @Test func aDuplicateDisplayNameIsRejectedAndItsFreshSecretsRemoved() async throws {
        let kind = FakeConnectorKind()
        let h = try AccountHarness(kinds: [kind])
        await h.manager.addAccount(kindID: "google")
        kind.nextConnectionID = "2"
        await h.manager.addAccount(kindID: "google")
        let status = await h.manager.status()
        #expect(status.accounts.map(\.connectionID) == ["1"])
        #expect(status.errorMessage == "a@x.test is already added.")
        #expect(try await h.credentials.secrets(for: "2") == nil)
        #expect(try await h.credentials.secrets(for: "1") != nil)
    }

    @Test func aDuplicateThatReusesTheStoredConnectionIDKeepsTheStoredSecrets() async throws {
        let h = try AccountHarness(kinds: [FakeConnectorKind()])
        await h.manager.addAccount(kindID: "google")
        await h.manager.addAccount(kindID: "google")
        #expect(await h.manager.status().accounts.map(\.connectionID) == ["1"])
        #expect(try await h.credentials.secrets(for: "1") != nil)
    }

    @Test func cancelledSignInDiscardsUnownedSecrets() async throws {
        let kind = FakeConnectorKind(outcome: .succeedThenCancel(connectionID: "c1"))
        let h = try AccountHarness(kinds: [kind])
        await h.manager.addAccount(kindID: "google")
        #expect(try await h.credentials.secrets(for: "c1") == nil)
        let status = await h.manager.status()
        #expect(status.accounts.isEmpty)
        #expect(status.errorMessage == nil)
    }

    @Test func aFailedAccountSaveDiscardsTheNewSignIn() async throws {
        let dir = try TempDir.make()
        try Data("x".utf8).write(to: dir.appendingPathComponent("blocker"))
        // The accounts file would have to live under a regular file, so every save fails.
        let h = try AccountHarness(kinds: [FakeConnectorKind()], directory: dir.appendingPathComponent("blocker"))
        await h.manager.addAccount(kindID: "google")
        let status = await h.manager.status()
        #expect(status.errorMessage != nil)
        #expect(status.accounts.isEmpty)
        #expect(try await h.credentials.secrets(for: "1") == nil)
    }

    @Test func removalRunsInTheDocumentedOrder() async throws {
        let h = try AccountHarness(kinds: [FakeConnectorKind()])
        await h.manager.addAccount(kindID: "google")
        h.settings.save(EngineSettings(takeover: {
            var t = TakeoverSettings()
            t.takeoverCalendarKeys = ["google-1/c", "eventkit/x"]
            t.hiddenCalendarKeys = ["google-1/d"]
            return t
        }()))
        await h.syncState.setToken("t", for: "1", scope: "s")
        h.log.set([])

        await h.manager.removeAccount(connectionID: "1")

        // Stored connection first (the reconcile already sees it gone), then source, selections, sync state, secrets.
        #expect(h.log.get() == ["reconcile", "selections", "sync", "secrets"])
        #expect(await h.reconciler.storedAtReconcile.last == false)
        #expect(await FileConnectionStore(url: h.accountsFile).connections().isEmpty)
        #expect(h.settings.load().takeover.takeoverCalendarKeys == ["eventkit/x"])
        #expect(h.settings.load().takeover.hiddenCalendarKeys.isEmpty)
        #expect(await h.syncState.token(for: "1", scope: "s") == nil)
        #expect(try await h.credentials.secrets(for: "1") == nil)
        #expect(h.applied.get().last == ["eventkit"])
        #expect(await h.manager.status().errorMessage == nil)
    }

    @Test func removalIsIdempotent() async throws {
        let h = try AccountHarness(kinds: [FakeConnectorKind()])
        await h.manager.addAccount(kindID: "google")
        await h.manager.removeAccount(connectionID: "1")
        await h.manager.removeAccount(connectionID: "1")
        let status = await h.manager.status()
        #expect(status.accounts.isEmpty)
        #expect(status.errorMessage == nil)
    }

    @Test func aSecretsFailureDuringRemovalReportsItButTheAccountIsGone() async throws {
        let h = try AccountHarness(kinds: [FakeConnectorKind()], failingRemoval: true)
        await h.manager.addAccount(kindID: "google")
        await h.manager.removeAccount(connectionID: "1")
        let status = await h.manager.status()
        #expect(status.accounts.isEmpty)
        #expect(await FileConnectionStore(url: h.accountsFile).connections().isEmpty)
        #expect(status.errorMessage?.hasPrefix("The account was removed, but its saved sign-in could not be deleted") == true)
    }

    @Test func reauthorizeReplacesTheAccountAndRebuildsItsSource() async throws {
        let kind = FakeConnectorKind()
        let h = try AccountHarness(kinds: [kind])
        await h.manager.addAccount(kindID: "google")
        let buildsBefore = await h.reconciler.builds
        await h.manager.reauthorize(connectionID: "1")
        #expect(kind.reauthorized == 1)
        #expect(await h.reconciler.builds == buildsBefore + 1)
        #expect(h.applied.get().last == ["eventkit", "google-1"])
        #expect(await h.manager.status().errorMessage == nil)
    }

    @Test func reauthorizingAsADifferentAccountSurfacesTheErrorAndKeepsTheOldAccount() async throws {
        let kind = FakeConnectorKind()
        let h = try AccountHarness(kinds: [kind])
        await h.manager.addAccount(kindID: "google")
        let before = await h.manager.status().accounts
        kind.reauthorizeError = CalendarCore.SourceError.invalidResponse("signed in as someone else")
        await h.manager.reauthorize(connectionID: "1")
        let status = await h.manager.status()
        #expect(status.accounts == before)
        #expect(status.errorMessage == "Sign-in failed: signed in as someone else.")
        #expect(await FileConnectionStore(url: h.accountsFile).connections() == before)
    }

    @Test func cancellingDuringSignInAgainChangesNothing() async throws {
        let kind = FakeConnectorKind()
        let h = try AccountHarness(kinds: [kind])
        await h.manager.addAccount(kindID: "google")
        let before = await h.manager.status().accounts
        let buildsBefore = await h.reconciler.builds
        kind.reauthorizeSuspends = true
        await h.manager.beginReauthorize(connectionID: "1")
        try await waitUntil { await h.manager.status().isWorking }
        await h.manager.cancelAuthorization()
        try await waitUntil { await !h.manager.status().isWorking }
        let status = await h.manager.status()
        #expect(status.accounts == before)
        #expect(status.errorMessage == nil)
        #expect(await h.reconciler.builds == buildsBefore)
    }

    @Test func anUnreadableAccountsFileLoadsNoneShowsTheMessageAndIsNeverWritten() async throws {
        let dir = try TempDir.make()
        let bytes = Data("not json".utf8)
        try bytes.write(to: dir.appendingPathComponent("accounts.json"))
        var initial = EngineSettings()
        initial.takeover.takeoverCalendarKeys = ["google-1/c", "google-9/c", "eventkit/x"]
        let h = try AccountHarness(kinds: [FakeConnectorKind()], initial: initial, directory: dir)
        await h.manager.start()
        let status = await h.manager.status()
        #expect(status.accounts.isEmpty)
        #expect(status.errorMessage == "Your saved accounts could not be read, so none were loaded. Nothing was deleted.")
        #expect(try Data(contentsOf: h.accountsFile) == bytes)
        #expect(h.settings.load().takeover.takeoverCalendarKeys == ["google-1/c", "google-9/c", "eventkit/x"])
        // A later add must not overwrite the unreadable file either.
        await h.manager.addAccount(kindID: "google")
        #expect(try Data(contentsOf: h.accountsFile) == bytes)
    }

    @Test func startSweepsOrphanedSelectionsForKnownKindPrefixesOnly() async throws {
        let dir = try TempDir.make()
        try await FileConnectionStore(url: dir.appendingPathComponent("accounts.json"))
            .save([Connection(kindID: "google", connectionID: "1", displayName: "a@x.test")])
        var initial = EngineSettings()
        initial.takeover.takeoverCalendarKeys = ["google-1/c", "google-9/c", "eventkit/x", "mystery-3/c"]
        let h = try AccountHarness(kinds: [FakeConnectorKind()], initial: initial, directory: dir)
        await h.manager.start()
        #expect(h.settings.load().takeover.takeoverCalendarKeys == ["google-1/c", "eventkit/x", "mystery-3/c"])
        #expect(await h.manager.status().accounts.map(\.connectionID) == ["1"])
    }

    @Test func systemCalendarToggleOffKeepsStoredSelections() async throws {
        var initial = EngineSettings()
        initial.takeover.takeoverCalendarKeys = ["eventkit/x"]
        let h = try AccountHarness(kinds: [FakeConnectorKind()], initial: initial)
        await h.manager.start()
        #expect(h.applied.get().last == ["eventkit"])

        await h.manager.setSystemCalendarEnabled(false)
        #expect(h.applied.get().last == [])
        #expect(h.settings.load().systemCalendarEnabled == false)
        #expect(h.settings.load().takeover.takeoverCalendarKeys.contains("eventkit/x"))

        await h.manager.setSystemCalendarEnabled(true)
        #expect(h.applied.get().last == ["eventkit"])
        #expect(h.settings.load().takeover.takeoverCalendarKeys.contains("eventkit/x"))
    }

    @Test func aRejectedPasswordReopensTheFormWithTheErrorAndWhatWasTyped() async throws {
        let kind = FakeConnectorKind(
            id: "caldav", displayName: "Other CalDAV", authorization: .password(fields: caldavFields), outcome: .password(good: "right"))
        let h = try AccountHarness(kinds: [kind], passwords: ["wrong", "right"])
        await h.manager.addAccount(kindID: "caldav")
        let armed = h.signIn.prepared
        #expect(armed.count == 2)
        #expect(armed[0].title == "Other CalDAV account")
        #expect(armed[0].error == nil)
        #expect(armed[1].error == "User name or password was not accepted.")
        #expect(armed[1].values == ["username": "me"])
        let status = await h.manager.status()
        #expect(status.accounts.map(\.displayName) == ["me"])
        #expect(status.errorMessage == nil)
        #expect(status.waitingStyle == .inApp)
    }

    @Test func cancellingThePasswordRetryLoopAddsNothingAndShowsNoError() async throws {
        let kind = FakeConnectorKind(
            id: "caldav", displayName: "Other CalDAV", authorization: .password(fields: caldavFields), outcome: .password(good: "right"))
        // One wrong attempt, then the form is cancelled (the fake form throws a cancellation when it has no answer).
        let h = try AccountHarness(kinds: [kind], passwords: ["wrong"])
        await h.manager.addAccount(kindID: "caldav")
        let status = await h.manager.status()
        #expect(status.accounts.isEmpty)
        #expect(status.errorMessage == nil)
        #expect(h.signIn.prepared.count == 2)
    }

    @Test func browserSignInsWaitForTheBrowser() async throws {
        let h = try AccountHarness(kinds: [FakeConnectorKind()])
        await h.manager.addAccount(kindID: "google")
        #expect(await h.manager.status().waitingStyle == .browser)
    }

    @Test func describeSignInNamesTheKindsOwnFields() {
        let icloud = [CredentialField(key: "username", label: "Apple ID"), CredentialField(key: "password", label: "App-specific password", isSecret: true)]
        #expect(AccountManager.describeSignIn(CalendarCore.SourceError.authExpired, fields: icloud) == "Apple ID or app-specific password was not accepted.")
        #expect(AccountManager.describeSignIn(CalendarCore.SourceError.server(status: 500), fields: icloud) == "Sign-in failed (server(status: 500)).")
    }

    @Test func aRejectedLinkIsDescribedForAKindWithOnlyASecretField() {
        let link = [CredentialField(key: "link", label: "iCal link", isSecret: true)]
        #expect(AccountManager.describeSignIn(CalendarCore.SourceError.authExpired, fields: link) == "iCal link was not accepted.")
        #expect(AccountManager.describeSignIn(CalendarCore.SourceError.invalidResponse("that link did not return a calendar"), fields: link)
                == "Sign-in failed: that link did not return a calendar.")
    }

    @Test func otherCalDAVIsOfferedLast() async throws {
        let caldav = FakeConnectorKind(id: "caldav", displayName: "Other CalDAV", authorization: .password(fields: caldavFields))
        let h = try AccountHarness(kinds: [caldav, FakeConnectorKind(), FakeConnectorKind(id: "apple", displayName: "Apple Calendar", authorization: .system)])
        #expect(await h.manager.availableKindIDs() == ["google", "caldav"])
    }
}
