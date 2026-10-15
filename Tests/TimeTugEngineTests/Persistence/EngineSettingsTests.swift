import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

@Suite struct EngineSettingsTests {
    @Test func legacySettingsKeysCarryForward() throws {
        let data = try Data(contentsOf: Fixtures.url("settings-legacy.json"))
        let settings = try JSONDecoder().decode(TakeoverSettings.self, from: data)
        #expect(settings.enabled == false)
        #expect(settings.requireOtherAttendees == true)
        #expect(settings.leadTime == 120)
    }

    @Test func defaultsMatchTheSpec() {
        let s = EngineSettings()
        #expect(s.takeover == TakeoverSettings())
        #expect(s.inferenceEnabled == false)
        #expect(s.systemCalendarEnabled == true)
    }

    @Test func inMemoryStorageReturnsInitialValueAndRecordsSaves() {
        let storage = InMemorySettingsStorage(EngineSettings(inferenceEnabled: true))
        #expect(storage.load().inferenceEnabled)
        #expect(storage.saved.isEmpty)
        var changed = storage.load()
        changed.systemCalendarEnabled = false
        storage.save(changed)
        #expect(storage.load() == changed)
        #expect(storage.saved == [changed])
    }
}
