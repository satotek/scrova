import SwiftUI

@main
struct ScrovaiOSApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        WindowGroup {
            ContentView(model: delegate.model)
        }
    }
}

/// Background-task registration must finish before this method returns.
/// A SwiftUI `App` body can run later, which is too late for a launch handler.
@MainActor
final class AppDelegate: NSObject, UIApplicationDelegate {
    let model = AppModel()

    nonisolated func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        MainActor.assumeIsolated {
            FlushScheduler.register(model: self.model)
            FlushScheduler.scheduleNext()
        }
        return true
    }
}
