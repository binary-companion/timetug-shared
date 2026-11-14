import TimeTugKitGenerated

/// The generic `FieldUpdate<Value>` of the 1.x libraries. UniFFI has no generics, so the bindings have one concrete
/// enum per instantiation (`FieldUpdateString`, `FieldUpdateReminders`, `FieldUpdateRecurrenceRule`); this type converts
/// to and from them.
public enum FieldUpdate<Value: Sendable>: Sendable {
    case keep
    case set(Value)
    case clear
}

extension FieldUpdate: Equatable where Value: Equatable {}

extension FieldUpdate where Value == String {
    public init(_ mirror: FieldUpdateString) {
        switch mirror {
        case .keep: self = .keep
        case .set(let value): self = .set(value)
        case .clear: self = .clear
        }
    }

    public var mirror: FieldUpdateString {
        switch self {
        case .keep: .keep
        case .set(let value): .set(value)
        case .clear: .clear
        }
    }
}

extension FieldUpdate where Value == [Reminder] {
    public init(_ mirror: FieldUpdateReminders) {
        switch mirror {
        case .keep: self = .keep
        case .set(let value): self = .set(value)
        case .clear: self = .clear
        }
    }

    public var mirror: FieldUpdateReminders {
        switch self {
        case .keep: .keep
        case .set(let value): .set(value)
        case .clear: .clear
        }
    }
}

extension FieldUpdate where Value == RecurrenceRule {
    public init(_ mirror: FieldUpdateRecurrenceRule) {
        switch mirror {
        case .keep: self = .keep
        case .set(let value): self = .set(value)
        case .clear: self = .clear
        }
    }

    public var mirror: FieldUpdateRecurrenceRule {
        switch self {
        case .keep: .keep
        case .set(let value): .set(value)
        case .clear: .clear
        }
    }
}
