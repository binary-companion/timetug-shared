import Foundation

/// Finds a video-conference join link in event text. Uses a known-provider allowlist so a
/// "Join" button never opens something like a shared document.
public enum ConferenceLinkDetector {
    struct Provider {
        let host: String
        var pathPrefix: String? = nil
    }

    /// Data, not logic: add providers here.
    static let providers: [Provider] = [
        Provider(host: "zoom.us"), Provider(host: "zoom.com"),
        Provider(host: "meet.google.com"),
        Provider(host: "teams.microsoft.com"), Provider(host: "teams.live.com"),
        Provider(host: "webex.com"),
        Provider(host: "gotomeet.me"), Provider(host: "gotomeeting.com"),
        Provider(host: "whereby.com"),
        Provider(host: "meet.jit.si"),
        Provider(host: "app.slack.com", pathPrefix: "/huddle"),
    ]

    /// Scans location, then url, then notes for an allowlisted link. If none is found but the
    /// event's own `url` is a web link, returns that (the invite put it there on purpose).
    public static func detect(location: String?, url: URL?, notes: String?) -> URL? {
        for text in [location, url?.absoluteString, notes] {
            if let text, let link = firstProviderLink(in: text) { return link }
        }
        if let url, let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https" {
            return url
        }
        return nil
    }

    static func firstProviderLink(in text: String) -> URL? {
        for candidate in candidates(in: text) {
            if let link = unwrap(candidate), isProvider(link) { return link }
        }
        return nil
    }

    static func candidates(in text: String) -> [String] {
        let decoded = text.replacingOccurrences(of: "&amp;", with: "&")
        guard let regex = try? NSRegularExpression(pattern: #"(?:https?|zoommtg)://[^\s<>"'\)\]]+"#) else {
            return []
        }
        let range = NSRange(decoded.startIndex..., in: decoded)
        return regex.matches(in: decoded, range: range).compactMap { match in
            Range(match.range, in: decoded).map {
                String(decoded[$0]).trimmingCharacters(in: CharacterSet(charactersIn: ".,;:!?"))
            }
        }
    }

    /// Unwraps Outlook SafeLinks and Google redirect URLs (up to a few layers).
    static func unwrap(_ raw: String) -> URL? {
        var current = URL(string: raw)
        for _ in 0..<3 {
            guard let url = current, let host = url.host?.lowercased() else { break }
            let inner: String?
            if host.hasSuffix("safelinks.protection.outlook.com") {
                inner = queryValue(url, "url")
            } else if host == "www.google.com", url.path == "/url" {
                inner = queryValue(url, "q") ?? queryValue(url, "url")
            } else {
                break
            }
            guard let inner, let next = URL(string: inner) else { break }
            current = next
        }
        return current
    }

    static func isProvider(_ url: URL) -> Bool {
        guard let scheme = url.scheme?.lowercased() else { return false }
        if scheme == "zoommtg" { return true }
        guard scheme == "http" || scheme == "https", let host = url.host?.lowercased() else { return false }
        return providers.contains { provider in
            (host == provider.host || host.hasSuffix("." + provider.host))
                && (provider.pathPrefix.map { url.path.hasPrefix($0) } ?? true)
        }
    }

    private static func queryValue(_ url: URL, _ name: String) -> String? {
        URLComponents(url: url, resolvingAgainstBaseURL: false)?
            .queryItems?.first { $0.name == name }?.value
    }
}
