import BackgroundTasks
import Foundation

/// One `BGAppRefreshTask` that recovers missed library and recently played
/// streaming history, calls `flush()`, and schedules the next request.
/// Registration has to finish before `applicationDidFinishLaunching` returns,
/// and a second registration of the same identifier kills the app.
enum FlushScheduler {
    static let identifier = "dev.satotek.scrova.ios.flush"

    private static let modelBox = ModelBox()

    static func register(model: AppModel) {
        modelBox.model = model
        // Called once, from applicationDidFinishLaunching. A second registration
        // of the same identifier kills the process, so this is not retried.
        BGTaskScheduler.shared.register(forTaskWithIdentifier: identifier, using: nil) { task in
            handle(task)
        }
    }

    static func scheduleNext() {
        let request = BGAppRefreshTaskRequest(identifier: identifier)
        // nil means "no extra delay"; the system still chooses when to launch.
        request.earliestBeginDate = nil
        // Replacing an already queued request is the documented way to update it.
        try? BGTaskScheduler.shared.submit(request)
    }

    private static func handle(_ task: BGTask) {
        scheduleNext()
        guard let refresh = task as? BGAppRefreshTask else {
            task.setTaskCompleted(success: false)
            return
        }

        // The task object must not be captured by both the expiration handler
        // and a concurrent Task; the gate is the only shared completion path.
        let gate = CompletionGate(refresh)
        let work = Task { @MainActor in
            await modelBox.model?.recoverHistory()
            await modelBox.model?.flushAwaiting()
        }
        refresh.expirationHandler = {
            work.cancel()
            gate.complete(success: false)
        }
        Task {
            await work.value
            gate.complete(success: !work.isCancelled)
        }
    }
}

/// The launch handler is not on the main actor. The app delegate owns the model.
/// The launch handler is not isolated to the main actor. The model is only
/// messaged from a `Task { @MainActor in }`, so the reference itself is unsafe
/// to touch from that handler's queue.
private final class ModelBox: @unchecked Sendable {
    nonisolated(unsafe) weak var model: AppModel?
}

/// `setTaskCompleted` must be called once. Expiration and the work finishing can race.
private final class CompletionGate: @unchecked Sendable {
    private let lock = NSLock()
    private var didComplete = false
    private let task: BGTask

    init(_ task: BGTask) {
        self.task = task
    }

    func complete(success: Bool) {
        lock.lock()
        defer { lock.unlock() }
        guard !didComplete else { return }
        didComplete = true
        task.setTaskCompleted(success: success)
    }
}
