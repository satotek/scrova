import AppKit
import Observation
import ScrovaKit
import ServiceManagement
import SwiftUI

/// Owns the Rust core and the Music.app observer. The menu bar window is not
/// created until the icon is clicked, so observation starts here.
@MainActor
@Observable
final class AppModel {
    enum Account: Equatable {
        case signedOut
        /// The browser is showing Last.fm's approval page; Scrova polls until
        /// the user approves it.
        case awaitingApproval(url: URL)
        case signedIn(username: String)
    }

    private let scrova: Scrova?
    private let observer = MusicObserver()
    private var flushTimer: Timer?
    private var approvalTask: Task<Void, Never>?

    private(set) var currentTrack: Track?
    private(set) var artwork: NSImage?
    private(set) var tint: Color?
    private(set) var progress: SessionProgress?
    private(set) var playbackState: PlaybackState = .stopped
    private(set) var recentScrobbles: [ScrobbleRecord] = []
    private(set) var pendingCount: UInt64 = 0
    private(set) var account: Account = .signedOut
    private(set) var accountNotice: String?
    private(set) var isSending = false
    private(set) var automationDenied = false
    private(set) var readFailure: String?
    private(set) var setupFailure: String?
    private(set) var launchesAtLogin = false
    private(set) var loginItemNotice: String?
    /// Login item problems are only worth mentioning once the user asks for it.
    private(set) var loginItemRequested = false

    /// Catches scrobbles whose retry time has come and recovers after the
    /// network returns, without depending on reachability callbacks.
    private static let flushInterval: TimeInterval = 5 * 60

    var menuImage: String {
        automationDenied || setupFailure != nil ? "music.note.slash" : "music.note"
    }

    init() {
        do {
            scrova = try Scrova(config: ScrovaSetup.makeConfig())
        } catch {
            scrova = nil
            setupFailure = "Scrova could not start: \(error.localizedDescription)"
        }

        restoreSession()
        refreshQueue()

        observer.onEvent = { [weak self] event in self?.ingest(event) }
        observer.onFailure = { [weak self] failure in self?.note(failure) }
        observer.onArtwork = { [weak self] image in
            guard let self else { return }
            artwork = image
            let cgImage = image?.cgImage(forProposedRect: nil, context: nil, hints: nil)
            tint = cgImage.flatMap(ArtworkTint.color)
            if let cgImage, let track = currentTrack {
                ArtworkCache.store(cgImage, for: track)
            }
        }
        observer.start()

        flushTimer = Timer.scheduledTimer(withTimeInterval: Self.flushInterval, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.flush() }
        }
        flush()
        refreshLoginItem()
    }

    // MARK: Last.fm account

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
                guard let url = URL(string: request.url) else {
                    accountNotice = "Last.fm did not return an approval page."
                    return
                }
                account = .awaitingApproval(url: url)
                NSWorkspace.shared.open(url)
                let lastfm = try await LastfmApproval.waitForSession(scrova, token: request.token)
                try KeychainStore.saveSessionKey(lastfm.sessionKey)
                UserDefaults.standard.set(lastfm.username, forKey: ScrovaSetup.usernameDefaultsKey)
                account = .signedIn(username: lastfm.username)
                flush()
            } catch is CancellationError {
                // cancelSignIn already reset the state.
            } catch {
                account = .signedOut
                accountNotice = ScrovaSetup.describe(error)
            }
        }
    }

    func reopenApprovalPage() {
        guard case .awaitingApproval(let url) = account else { return }
        NSWorkspace.shared.open(url)
    }

    func cancelSignIn() {
        approvalTask?.cancel()
        approvalTask = nil
        account = .signedOut
        accountNotice = nil
    }

    func signOut() {
        try? KeychainStore.deleteSessionKey()
        UserDefaults.standard.removeObject(forKey: ScrovaSetup.usernameDefaultsKey)
        scrova?.setSessionKey(sessionKey: nil)
        account = .signedOut
    }

    // MARK: Queue

    func flush() {
        guard let scrova, case .signedIn = account, !isSending else { return }
        isSending = true
        Task {
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
    }

    // MARK: Login item

    func refreshLoginItem() {
        launchesAtLogin = LoginItem.isEnabled
        loginItemNotice = Self.notice(for: LoginItem.status)
    }

    func setLaunchesAtLogin(_ enabled: Bool) {
        loginItemRequested = true
        do {
            try LoginItem.setEnabled(enabled)
            loginItemNotice = nil
        } catch {
            // register() throws when approval is still required. That is the
            // settings handoff, not a broken toggle.
            if LoginItem.status != .requiresApproval {
                loginItemNotice = "Login item could not be updated."
            }
        }
        if LoginItem.status == .requiresApproval {
            LoginItem.openSystemSettingsIfApprovalRequired()
        }
        refreshLoginItem()
    }

    func openAutomationSettings() {
        // No public API opens the Automation pane. This is the conventional
        // System Settings deep link, not an Apple-documented URL.
        guard let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation") else {
            return
        }
        NSWorkspace.shared.open(url)
    }

    func quit() {
        NSApp.terminate(nil)
    }

    // MARK: Private

    private func ingest(_ event: PlaybackEvent) {
        automationDenied = false
        readFailure = nil
        playbackState = event.state
        currentTrack = event.state == .stopped ? nil : event.track

        guard let scrova else { return }
        let outputs: [EngineOutput]
        do {
            outputs = try scrova.handlePlaybackEvent(event: event)
        } catch {
            readFailure = ScrovaSetup.describe(error)
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

    private func note(_ failure: MusicObserver.Failure) {
        switch failure {
        case .notPermitted:
            automationDenied = true
            readFailure = nil
        case .scriptError(let code, _):
            automationDenied = false
            readFailure = "Music could not be read (Apple Event \(code))."
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
    }

    private static func notice(for status: SMAppService.Status) -> String? {
        switch status {
        case .requiresApproval:
            "Confirm Scrova in System Settings → General → Login Items."
        case .notFound:
            "Launch at login works after Scrova is in the Applications folder."
        case .enabled, .notRegistered:
            nil
        @unknown default:
            nil
        }
    }
}
