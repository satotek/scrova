import AuthenticationServices
import MediaPlayer
import Observation
import ScrovaKit
import SwiftUI
import UIKit

/// iOS counterpart of the menu-bar model. Playback comes from `NowPlayingObserver`;
/// Last.fm traffic stays in Rust.
@MainActor
@Observable
final class AppModel {
    enum Account: Equatable {
        case signedOut
        /// Scrova polls Last.fm until the user approves on this page.
        case awaitingApproval(url: String)
        case signedIn(username: String)
    }

    private let scrova: Scrova?
    private let observer = NowPlayingObserver()
    private let authPresenter = AuthPresenter()
    private var authSession: ASWebAuthenticationSession?
    private var approvalTask: Task<Void, Never>?

    /// Shared by the History tab and the now-playing "Last scrobbled" row.
    let history: HistoryPager

    private(set) var currentTrack: Track?
    private(set) var artwork: UIImage?
    private(set) var tint: Color?
    private(set) var progress: SessionProgress?
    private(set) var playbackState: PlaybackState = .stopped
    @ObservationIgnored private var foreground = false
    @ObservationIgnored private var isRecovering = false
    private(set) var recentScrobbles: [ScrobbleRecord] = []
    private(set) var pendingCount: UInt64 = 0
    private(set) var account: Account = .signedOut
    private(set) var accountNotice: String?
    private(set) var isSending = false
    private(set) var setupFailure: String?
    private(set) var libraryDenied = false
    private(set) var libraryAuthorizationStatus: MPMediaLibraryAuthorizationStatus = MPMediaLibrary.authorizationStatus()
    private(set) var backgroundRefreshStatus: UIBackgroundRefreshStatus = UIApplication.shared.backgroundRefreshStatus

    init() {
        let core: Scrova?
        do {
            core = try Scrova(config: ScrovaSetup.makeConfig())
        } catch {
            core = nil
            setupFailure = "Scrova could not start: \(error.localizedDescription)"
        }
        scrova = core
        history = HistoryPager(scrova: core)
        restoreSession()
        refreshQueue()
        refreshPermissions()
        observer.onEvent = { [weak self] event in
            self?.ingest(event)
        }
        observer.onArtwork = { [weak self] image in
            guard let self else { return }
            artwork = image
            tint = image?.cgImage.flatMap(ArtworkTint.color)
            if let cgImage = image?.cgImage, let track = currentTrack {
                ArtworkCache.store(cgImage, for: track)
            }
        }
        observer.start()
    }

    func activate() {
        Task {
            let status = await observer.requestAuthorizationIfNeeded()
            libraryAuthorizationStatus = status
            libraryDenied = status == .denied || status == .restricted
            backgroundRefreshStatus = UIApplication.shared.backgroundRefreshStatus
            observer.poll()
            await recoverHistory()
            flush()
        }
        // The view exists, so this is a foreground launch, not a refresh-task wake.
        setForeground(true)
        FlushScheduler.scheduleNext()
    }

    /// Refresh values the Settings permission rows display.
    func refreshPermissions() {
        libraryAuthorizationStatus = MPMediaLibrary.authorizationStatus()
        libraryDenied = libraryAuthorizationStatus == .denied || libraryAuthorizationStatus == .restricted
        backgroundRefreshStatus = UIApplication.shared.backgroundRefreshStatus
    }

    func setForeground(_ isForeground: Bool) {
        let returning = isForeground && !foreground
        foreground = isForeground
        observer.setForeground(isForeground)
        // The process often survives in the background, so launch is not the
        // only time to look for plays made while away: a walk with the screen
        // off ends with the app being brought back, not relaunched.
        if returning {
            Task {
                await recoverHistory()
                flush()
            }
        }
    }

    func signIn() {
        guard let scrova else { return }
        guard ScrovaSetup.hasLastfmCredentials else {
            accountNotice = ScrovaSetup.missingCredentialsNotice
            return
        }
        approvalTask?.cancel()
        accountNotice = nil
        approvalTask = Task {
            do {
                let request = try await scrova.beginAuthorization()
                account = .awaitingApproval(url: request.url)
                openApprovalPage(request.url)
                let lastfm = try await LastfmApproval.waitForSession(scrova, token: request.token)
                try KeychainStore.saveSessionKey(lastfm.sessionKey)
                UserDefaults.standard.set(lastfm.username, forKey: ScrovaSetup.usernameDefaultsKey)
                account = .signedIn(username: lastfm.username)
                // Last.fm only says "you may close this window"; do it for the user.
                authSession?.cancel()
                flush()
            } catch is CancellationError {
                // cancelSignIn already reset the state.
            } catch {
                authSession?.cancel()
                account = .signedOut
                accountNotice = ScrovaSetup.describe(error)
            }
        }
    }

    func reopenApprovalPage() {
        guard case .awaitingApproval(let url) = account else { return }
        openApprovalPage(url)
    }

    func cancelSignIn() {
        approvalTask?.cancel()
        approvalTask = nil
        authSession?.cancel()
        account = .signedOut
        accountNotice = nil
    }

    func signOut() {
        try? KeychainStore.deleteSessionKey()
        UserDefaults.standard.removeObject(forKey: ScrovaSetup.usernameDefaultsKey)
        scrova?.setSessionKey(sessionKey: nil)
        account = .signedOut
    }

    func flush() {
        Task { await flushAwaiting() }
    }

    /// Queues plays that happened while Scrova was closed. Safe to repeat: Rust
    /// derives each recovered scrobble's ID from the track and start time.
    func recoverHistory() async {
        guard let scrova, MPMediaLibrary.authorizationStatus() == .authorized, !isRecovering else { return }
        // Launch and the first foreground transition both ask; one scan suffices.
        isRecovering = true
        defer { isRecovering = false }
        do {
            let baseline = try scrova.libraryScannedAt(source: NowPlayingObserver.source)
            // A large library takes a while to enumerate; keep it off the main actor.
            let items = await Task.detached(priority: .utility) {
                LibraryHistory.items(baselineAt: baseline)
            }.value
            let report = try scrova.recoverLibraryHistory(source: NowPlayingObserver.source, items: items)
            if report.queued > 0 { refreshQueue() }
        } catch {
            accountNotice = ScrovaSetup.describe(error)
        }
    }

    func flushAwaiting() async {
        guard let scrova, case .signedIn = account, !isSending else { return }
        isSending = true
        defer {
            isSending = false
            refreshQueue()
        }
        do {
            _ = try await scrova.flush()
            accountNotice = nil
        } catch ScrovaError.SessionInvalid {
            signOut()
            accountNotice = "Last.fm signed Scrova out. Sign in again; queued scrobbles are kept."
        } catch {
            accountNotice = ScrovaSetup.describe(error)
        }
    }

    private func openApprovalPage(_ urlString: String) {
        guard let url = URL(string: urlString), url.scheme == "https" || url.scheme == "http" else {
            accountNotice = "Last.fm did not return an approval page."
            return
        }
        // Last.fm's desktop flow never redirects back, so a nil scheme: the
        // sheet closes when the user cancels or when polling sees the approval.
        // Polling keeps going if the user closes it early, since they may
        // already have approved.
        let session = ASWebAuthenticationSession(url: url, callbackURLScheme: nil) { [weak self] _, _ in
            Task { @MainActor in self?.authSession = nil }
        }
        session.presentationContextProvider = authPresenter
        authSession = session
        if !session.start() {
            accountNotice = "Could not open the Last.fm page."
        }
    }

    private func ingest(_ event: PlaybackEvent) {
        playbackState = event.state
        currentTrack = event.state == .stopped ? nil : event.track
        guard let scrova else { return }
        let outputs: [EngineOutput]
        do {
            outputs = try scrova.handlePlaybackEvent(event: event)
        } catch {
            accountNotice = ScrovaSetup.describe(error)
            return
        }
        progress = scrova.sessionProgress(source: event.source)
        for output in outputs {
            switch output {
            case .nowPlaying(let track, _):
                guard case .signedIn = account else { continue }
                // Last.fm asks clients not to retry Now Playing; drop failures.
                Task { try? await scrova.updateNowPlaying(track: track) }
            case .scrobble:
                refreshQueue()
                flush()
            }
        }
    }

    private func restoreSession() {
        guard let scrova,
              let key = try? KeychainStore.loadSessionKey(),
              let username = UserDefaults.standard.string(forKey: ScrovaSetup.usernameDefaultsKey)
        else { return }
        scrova.setSessionKey(sessionKey: key)
        account = .signedIn(username: username)
    }

    private func refreshQueue() {
        guard let scrova else { return }
        recentScrobbles = (try? scrova.recentScrobbles(limit: 10)) ?? []
        pendingCount = (try? scrova.pendingCount()) ?? 0
        history.invalidate()
    }
}
