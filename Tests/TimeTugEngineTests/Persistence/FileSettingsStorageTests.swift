import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

@Suite struct FileSettingsStorageTests {
    @Test func missingFileLoadsDefaults() throws {
        let storage = FileSettingsStorage(directory: try TempDir.make())
        #expect(storage.load() == EngineSettings())
    }

    @Test func roundTripsAcrossInstances() throws {
        let dir = try TempDir.make()
        var settings = EngineSettings()
        settings.inferenceEnabled = true
        settings.systemCalendarEnabled = false
        settings.takeover.leadTime = 120
        settings.takeover.takeoverCalendarKeys = ["a/b"]
        FileSettingsStorage(directory: dir).save(settings)
        #expect(FileSettingsStorage(directory: dir).load() == settings)
    }

    @Test func savesIntoMissingDirectory() throws {
        let dir = try TempDir.make().appendingPathComponent("nested/deeper")
        var settings = EngineSettings()
        settings.inferenceEnabled = true
        FileSettingsStorage(directory: dir).save(settings)
        #expect(FileSettingsStorage(directory: dir).load() == settings)
    }

    @Test func corruptFileLoadsDefaultsAndReportsOnce() throws {
        let dir = try TempDir.make()
        try Data("{not json".utf8).write(to: dir.appendingPathComponent("settings.json"))
        let box = LockedBox<[EngineDiagnostic]>([])
        let storage = FileSettingsStorage(directory: dir) { d in box.mutate { $0.append(d) } }
        #expect(storage.load() == EngineSettings())
        #expect(box.get().count == 1)
        #expect(box.get().first?.level == .warning)
        #expect(box.get().first?.category == "settings")
    }

    @Test func fileNameIsFixed() {
        #expect(FileSettingsStorage.fileName == "settings.json")
    }
}
