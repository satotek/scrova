import ScrovaKit
import SwiftUI

struct ContentView: View {
    @Bindable var model: AppModel
    @Environment(\.scenePhase) private var scenePhase
    @State private var tab: Tab = .nowPlaying

    var body: some View {
        TabView(selection: $tab) {
            NowPlayingView(
                model: model,
                openSettings: { tab = .settings },
                openHistory: { tab = .history }
            )
            .tabItem { Label("Now playing", systemImage: "music.note") }
            .tag(Tab.nowPlaying)

            HistoryView(model: model)
                .tabItem { Label("History", systemImage: "clock") }
                .tag(Tab.history)

            SettingsView(model: model)
                .tabItem { Label("Settings", systemImage: "gearshape") }
                .tag(Tab.settings)
        }
        .onAppear { model.activate() }
        .onChange(of: scenePhase) { _, phase in
            // `.inactive` is still in the foreground (control center, app switcher).
            // Only a real background transition stops the engine clock.
            model.setForeground(phase != .background)
            if phase == .active {
                model.refreshPermissions()
            }
        }
        .onChange(of: tab) { _, tab in
            guard tab == .history else { return }
            model.history.invalidate()
            Task { await model.history.loadInitialIfNeeded() }
        }
    }

    private enum Tab: Hashable {
        case nowPlaying
        case history
        case settings
    }
}
