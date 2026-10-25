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
    }
}
