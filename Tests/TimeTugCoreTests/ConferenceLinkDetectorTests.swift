import Foundation
import Testing
@testable import TimeTugCore

private func detect(location: String? = nil, url: String? = nil, notes: String? = nil) -> URL? {
    ConferenceLinkDetector.detect(location: location, url: url.flatMap(URL.init(string:)), notes: notes)
}

@Test func findsZoomInLocation() {
    #expect(detect(location: "https://acme.zoom.us/j/123456789?pwd=abc")?.host == "acme.zoom.us")
}

@Test func findsMeetInNotes() {
    #expect(detect(notes: "Join: https://meet.google.com/abc-defg-hij.")?.absoluteString
        == "https://meet.google.com/abc-defg-hij")
}

@Test func findsTeamsInsideHTMLWithEntities() {
    let notes = #"<p><a href="https://teams.microsoft.com/l/meetup-join/19%3ameeting?a=1&amp;b=2">Join</a></p>"#
    let result = detect(notes: notes)
    #expect(result?.host == "teams.microsoft.com")
    #expect(result?.query == "a=1&b=2")
}

@Test func unwrapsOutlookSafeLinks() {
    let wrapped = "https://nam02.safelinks.protection.outlook.com/?url=https%3A%2F%2Fteams.microsoft.com%2Fl%2Fmeetup-join%2F0&data=05"
    #expect(detect(notes: "Click \(wrapped) now")?.host == "teams.microsoft.com")
}

@Test func unwrapsGoogleRedirects() {
    let wrapped = "https://www.google.com/url?q=https://acme.zoom.us/j/1&sa=D"
    #expect(detect(notes: wrapped)?.host == "acme.zoom.us")
}

@Test func acceptsZoomMtgScheme() {
    #expect(detect(notes: "zoommtg://zoom.us/join?confno=123")?.scheme == "zoommtg")
}

@Test func ignoresNonProviderLinksInText() {
    #expect(detect(notes: "Agenda: https://docs.google.com/document/d/abc") == nil)
}

@Test func locationBeatsNotes() {
    let result = detect(location: "https://meet.google.com/aaa-bbbb-ccc",
                        notes: "https://acme.zoom.us/j/1")
    #expect(result?.host == "meet.google.com")
}

@Test func returnsFirstProviderLinkSkippingOthers() {
    let notes = "Doc https://docs.google.com/x then https://acme.zoom.us/j/9"
    #expect(detect(notes: notes)?.host == "acme.zoom.us")
}

@Test func eventURLFallbackWhenNotAllowlisted() {
    #expect(detect(url: "https://example.com/meeting/42")?.absoluteString == "https://example.com/meeting/42")
}

@Test func slackHuddleNeedsHuddlePath() {
    #expect(detect(notes: "https://app.slack.com/huddle/T1/C1") != nil)
    #expect(detect(notes: "https://app.slack.com/client/T1/C1") == nil)
}

@Test func noLinksReturnsNil() {
    #expect(detect(location: "Room 4", notes: "Bring laptop") == nil)
}
