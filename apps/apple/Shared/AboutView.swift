import ScrovaKit
import SwiftUI

/// Shared About screen. Settings pushes it on iOS and wraps it in a
/// NavigationStack in the macOS Settings tab.
struct AboutView: View {
    @State private var coreVersionText = ""

    var body: some View {
        List {
            Section {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Scrova")
                        .font(.title2.weight(.semibold))
                    Text("Version \(version) (\(build))")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    Text(coreVersionText.isEmpty ? "Core unavailable" : "Core \(coreVersionText)")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 4)

                Text("Scrova keeps your Last.fm profile up to date with the music you play.")
                    .font(.body)
                    .foregroundStyle(.secondary)
            }

            Section {
                NavigationLink("Privacy policy") {
                    PrivacyPolicyView()
                }
                NavigationLink("Open-source licenses") {
                    LicensesView()
                }
            }

            Section {
                Text("Scrobbling uses the Last.fm API. Scrova is not affiliated with Last.fm.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .navigationTitle("About Scrova")
        .task {
            coreVersionText = coreVersion()
        }
    }

    private var version: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "—"
    }

    private var build: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "—"
    }
}
