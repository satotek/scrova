import SwiftUI

/// Lists the Rust crates linked into scrova-ffi from the generated JSON, so the
/// app does not need a hand-maintained copy of the dependency graph.
struct LicensesView: View {
    @State private var entries: [LicenseEntry] = []
    @State private var failed = false

    var body: some View {
        Group {
            if failed {
                Text("Licenses are missing from this build.")
                    .foregroundStyle(.secondary)
            } else {
                List(entries) { entry in
                    VStack(alignment: .leading, spacing: 3) {
                        Text(entry.name)
                            .font(.body.weight(.medium))
                        Text("Version \(entry.version) — \(entry.license)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        if let repository = entry.repository, let url = URL(string: repository) {
                            Link(repository, destination: url)
                                .font(.caption)
                        }
                    }
                    .padding(.vertical, 3)
                }
            }
        }
        .navigationTitle("Open-source licenses")
        .task { load() }
    }

    private func load() {
        guard let url = Bundle.main.url(forResource: "licenses-rust", withExtension: "json"),
              let data = try? Data(contentsOf: url)
        else {
            failed = true
            return
        }
        do {
            entries = try JSONDecoder().decode([LicenseEntry].self, from: data)
            failed = entries.isEmpty
        } catch {
            failed = true
        }
    }
}

private struct LicenseEntry: Decodable, Identifiable {
    var name: String
    var version: String
    var license: String
    var repository: String?

    var id: String { "\(name)-\(version)" }
}
