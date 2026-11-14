import Foundation
import Testing
import TimeTugSpike

func sendableProbe() async {
    let e = makeAgenda(count: 1)[0]
    await Task.detached { _ = e }.value          // BigEvent crossing isolation in Swift 6 mode
}
