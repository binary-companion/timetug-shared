import CalendarCore
import Foundation
import TimeTugCore

/// Synthetic, deterministic resolver timing. No real calendar data is read.
private struct Generator {
    var state: UInt64 = 0x5eed
    mutating func next() -> UInt64 {
        state = state &* 6364136223846793005 &+ 1442695040888963407
        return state
    }
}

private func events(count: Int, shape: String) -> [TimeTugCalendarEvent] {
    var random = Generator()
    let origin = Date(timeIntervalSince1970: 1_800_000_000)
    return (0..<count).map { index in
        let slot: Int
        let title: String
        switch shape {
        case "sparse":
            slot = index * 60
            title = "Unique \(index)"
        case "dense":
            slot = Int(random.next() % 60)
            title = "Meeting \(index)"
        default:
            slot = index / 4
            title = "Duplicate \(index / 4)"
        }
        let start = origin.addingTimeInterval(TimeInterval(slot * 60))
        let event = CalendarEvent(eventID: "e\(index)", calendarID: "c\(index % 4)",
                                  title: title, start: start, end: start.addingTimeInterval(1800))
        return TimeTugCalendarEvent(event: event, sourceID: "synthetic")
    }
}

let repetitions = 5
print("shape,count,median_ms,output_events")
for shape in ["sparse", "dense", "duplicates"] {
    for count in [100, 500, 1_000, 5_000] {
        let input = events(count: count, shape: shape)
        var times: [Double] = []
        var outputCount = 0
        for repetition in 0...repetitions {
            let start = DispatchTime.now().uptimeNanoseconds
            let output = DuplicateResolver.resolve(events: input, calendars: [], lessons: LessonBook(), verdicts: nil)
            let end = DispatchTime.now().uptimeNanoseconds
            outputCount = output.events.count
            if repetition > 0 { times.append(Double(end - start) / 1_000_000) }
        }
        times.sort()
        print("\(shape),\(count),\(String(format: "%.3f", times[times.count / 2])),\(outputCount)")
    }
}
