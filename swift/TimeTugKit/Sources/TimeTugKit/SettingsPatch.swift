import TimeTugKitGenerated

extension SettingsPatch {
    /// The patch that turns `old` into `new`: each setting that differs is set, the rest stays `nil`. Replaces the closure
    /// `(inout EngineSettings) -> Void` of the 1.x `applySettings` command.
    public init(from old: EngineSettings, to new: EngineSettings) {
        self.init(
            leadTime: new.takeover.leadTime == old.takeover.leadTime ? nil : new.takeover.leadTime,
            takeoverCalendarKeys: new.takeover.takeoverCalendarKeys == old.takeover.takeoverCalendarKeys
                ? nil : new.takeover.takeoverCalendarKeys,
            hiddenCalendarKeys: new.takeover.hiddenCalendarKeys == old.takeover.hiddenCalendarKeys
                ? nil : new.takeover.hiddenCalendarKeys,
            requireConferenceLink: new.takeover.requireConferenceLink == old.takeover.requireConferenceLink
                ? nil : new.takeover.requireConferenceLink,
            requireOtherAttendees: new.takeover.requireOtherAttendees == old.takeover.requireOtherAttendees
                ? nil : new.takeover.requireOtherAttendees,
            skipAllDayEvents: new.takeover.skipAllDayEvents == old.takeover.skipAllDayEvents
                ? nil : new.takeover.skipAllDayEvents,
            enabled: new.takeover.enabled == old.takeover.enabled ? nil : new.takeover.enabled,
            inferenceEnabled: new.inferenceEnabled == old.inferenceEnabled ? nil : new.inferenceEnabled,
            systemCalendarEnabled: new.systemCalendarEnabled == old.systemCalendarEnabled
                ? nil : new.systemCalendarEnabled)
    }

    /// True when the patch changes nothing.
    public var isEmpty: Bool {
        self == SettingsPatch()
    }
}

extension EngineCommand {
    /// `applySettings { $0.takeover.leadTime = 120 }`, as the 1.x command was spelled: the closure edits a copy of `current`
    /// and the difference is sent as a patch.
    public static func applySettings(from current: EngineSettings, _ change: (inout EngineSettings) -> Void) -> EngineCommand {
        var edited = current
        change(&edited)
        return .applySettings(SettingsPatch(from: current, to: edited))
    }
}
