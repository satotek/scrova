import Foundation

/// Last.fm pages opened from the app. Both paths need percent-encoding, and a
/// slash in a name would otherwise become another path component.
enum LastfmLinks {
    static func track(artist: String, title: String) -> URL? {
        guard !artist.isEmpty, !title.isEmpty else { return nil }
        guard let artist = pathComponent(artist), let title = pathComponent(title) else { return nil }
        return URL(string: "https://www.last.fm/music/\(artist)/_/\(title)")
    }

    static func profile(username: String) -> URL? {
        guard !username.isEmpty, let username = pathComponent(username) else { return nil }
        return URL(string: "https://www.last.fm/user/\(username)")
    }

    private static func pathComponent(_ value: String) -> String? {
        var allowed = CharacterSet.urlPathAllowed
        allowed.remove(charactersIn: "/")
        return value.addingPercentEncoding(withAllowedCharacters: allowed)
    }
}
