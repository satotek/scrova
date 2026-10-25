import SwiftUI

/// Renders the Markdown policy bundled from docs/privacy-policy.md.
///
/// AttributedString handles inline Markdown but not block layout, so the file is
/// split into blocks and styled here. The policy file itself stays the single
/// source of truth shared with the repository docs.
struct PrivacyPolicyView: View {
    @State private var blocks: [Block] = []
    @State private var failed = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                if failed {
                    Text("The privacy policy is missing from this build.")
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(blocks) { block in
                        blockView(block)
                    }
                }
            }
            .frame(maxWidth: 680, alignment: .leading)
            .padding(20)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .textSelection(.enabled)
        .navigationTitle("Privacy policy")
        .task { load() }
    }

    @ViewBuilder
    private func blockView(_ block: Block) -> some View {
        switch block.kind {
        case .title:
            Text(attributed(block.text))
                .font(.title2.weight(.semibold))
        case .heading:
            Text(attributed(block.text))
                .font(.headline)
                .padding(.top, 4)
        case .paragraph:
            Text(attributed(block.text))
                .font(.body)
                .foregroundStyle(.primary)
        case .bullet:
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("•")
                Text(attributed(block.text))
            }
            .font(.body)
        }
    }

    private func attributed(_ text: String) -> AttributedString {
        (try? AttributedString(
            markdown: text,
            options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        )) ?? AttributedString(text)
    }

    private func load() {
        guard let url = Bundle.main.url(forResource: "privacy-policy", withExtension: "md"),
              let markdown = try? String(contentsOf: url, encoding: .utf8)
        else {
            failed = true
            return
        }
        blocks = Self.parse(markdown)
        failed = blocks.isEmpty
    }

    private static func parse(_ markdown: String) -> [Block] {
        var blocks: [Block] = []
        var nextID = 0
        let normalized = markdown.replacingOccurrences(of: "\r\n", with: "\n")

        for raw in normalized.components(separatedBy: "\n\n") {
            let lines = raw
                .split(separator: "\n", omittingEmptySubsequences: false)
                .map { $0.trimmingCharacters(in: .whitespaces) }
                .filter { !$0.isEmpty }
            guard !lines.isEmpty else { continue }

            if lines.first?.hasPrefix("- ") == true {
                // A Markdown list item can wrap onto indented lines. Join those
                // into the preceding bullet so the policy keeps its structure.
                var current = ""
                for line in lines {
                    if line.hasPrefix("- ") {
                        if !current.isEmpty {
                            blocks.append(Block(id: nextID, text: current, kind: .bullet))
                            nextID += 1
                        }
                        current = String(line.dropFirst(2))
                    } else if current.isEmpty {
                        current = line
                    } else {
                        current += " " + line
                    }
                }
                if !current.isEmpty {
                    blocks.append(Block(id: nextID, text: current, kind: .bullet))
                    nextID += 1
                }
            } else {
                let text = lines.joined(separator: " ")
                let kind: Block.Kind
                if text.hasPrefix("## ") {
                    kind = .heading
                } else if text.hasPrefix("# ") {
                    kind = .title
                } else {
                    kind = .paragraph
                }
                let stripped = text.hasPrefix("#") ? String(text.drop(while: { $0 == "#" })).trimmingCharacters(in: .whitespaces) : text
                blocks.append(Block(id: nextID, text: stripped, kind: kind))
                nextID += 1
            }
        }
        return blocks
    }

    private struct Block: Identifiable {
        enum Kind {
            case title
            case heading
            case paragraph
            case bullet
        }

        let id: Int
        let text: String
        let kind: Kind
    }
}
