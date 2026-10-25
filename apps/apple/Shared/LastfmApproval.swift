import Foundation
import ScrovaKit

/// Last.fm's desktop flow never calls the app back after the user approves,
/// so the app keeps asking for the session until the token is approved.
enum LastfmApproval {
    struct Expired: LocalizedError {
        var errorDescription: String? { "The Last.fm sign-in timed out. Try again." }
    }

    /// auth.getToken documents a 60-minute token lifetime.
    private static let tokenLifetime: Duration = .seconds(60 * 60)
    /// Fast enough to feel immediate after approving, far below the rate limit.
    private static let pollInterval: Duration = .seconds(3)

    /// Throws `CancellationError` when the surrounding task is cancelled.
    static func waitForSession(_ scrova: Scrova, token: String) async throws -> LastfmAccount {
        let deadline = ContinuousClock.now + tokenLifetime
        while true {
            do {
                return try await scrova.completeAuthorization(token: token)
            } catch ScrovaError.AuthorizationPending, ScrovaError.Network {
                // A dropped connection during a long wait should not make the
                // user approve again; the token is still good.
            }
            guard ContinuousClock.now + pollInterval < deadline else { throw Expired() }
            try await Task.sleep(for: pollInterval)
        }
    }
}
