import SwiftUI

@main
struct ScrovaApp: App {
    @State private var model = AppModel()

    var body: some Scene {
        MenuBarExtra {
            MenuContentView(model: model)
        } label: {
            Image(systemName: model.menuImage)
                .accessibilityLabel("Scrova")
        }
        .menuBarExtraStyle(.window)

        Settings {
            SettingsView(model: model)
        }

        Window("History", id: "history") {
            HistoryWindowView(model: model)
        }
        .defaultSize(width: 560, height: 720)
    }
}
