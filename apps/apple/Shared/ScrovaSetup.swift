import Foundation
import ScrovaKit

/// Bundle and on-disk setup shared by the macOS and iOS apps. Each app still
/// gets its own container; only the path layout and the user-facing wording match.
enum ScrovaSetup {
    static let usernameDefaultsKey = "lastfmUsername"

    static func makeConfig() throws -> ScrovaConfig {
        // Inside the sandbox this resolves to the app's container.
        let support = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true
        ).appending(path: "Scrova", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: support, withIntermediateDirectories: true)

        let info = Bundle.main.infoDictionary ?? [:]
        return ScrovaConfig(
            databasePath: support.appending(path: "scrova.sqlite").path(percentEncoded: false),
            lastfmApiKey: info["ScrovaLastfmApiKey"] as? String ?? "",
            lastfmSharedSecret: info["ScrovaLastfmSharedSecret"] as? String ?? ""
        )
    }

    /// Builds without Config/Secrets.xcconfig get empty values, and Last.fm
    /// answers those with an unhelpful "invalid parameters" error.
    static var hasLastfmCredentials: Bool {
        let info = Bundle.main.infoDictionary ?? [:]
        return ["ScrovaLastfmApiKey", "ScrovaLastfmSharedSecret"].allSatisfy {
            !(info[$0] as? String ?? "").isEmpty
        }
    }

    static let missingCredentialsNotice = "This build has no Last.fm API key, so it cannot sign in."

    static func describe(_ error: Error) -> String {
        switch error {
        case ScrovaError.Network:
            "Last.fm is unreachable. Scrobbles stay queued."
        case ScrovaError.ApiKeySuspended:
            "Last.fm has suspended this build’s API key."
        case ScrovaError.NotSignedIn:
            "Sign in to Last.fm to send scrobbles."
        case let ScrovaError.Lastfm(code, message):
            "Last.fm error \(code): \(message)"
        default:
            error.localizedDescription
        }
    }
}
