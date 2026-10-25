import ScrovaKit
import SwiftUI

/// Shared paged, day-grouped history. Both the iOS History tab and the macOS
/// history window use this view; only their surrounding navigation differs.
struct HistoryListView: View {
    @Bindable var pager: HistoryPager
    let pendingCount: UInt64
    let isSending: Bool
    let onSendNow: () -> Void

    @Environment(\.openURL) private var openURL

    var body: some View {
        List {
            if pendingCount > 0 {
                Section {
                    HStack {
                        Text("\(pendingCount) waiting to send")
                            .monospacedDigit()
                        Spacer()
                        Button("Send now", action: onSendNow)
                            .disabled(isSending)
                    }
                }
            }

            ForEach(sections) { section in
                Section {
                    ForEach(Array(section.records.enumerated()), id: \.offset) { _, record in
                        RecentScrobbleRow(record: record)
                            .contextMenu {
                                if case .submitted = record.status,
                                   let url = LastfmLinks.track(
                                       artist: record.track.artist,
                                       title: record.track.title
                                   ) {
                                    Button("Open on Last.fm") {
                                        openURL(url)
                                    }
                                }
                            }
                    }
                } header: {
                    Text(dayTitle(section.day))
                        .textCase(nil)
                }
            }

            if pager.isLoading {
                HStack {
                    Spacer()
                    ProgressView()
                    Spacer()
                }
            } else if pager.hasNext {
                // Appearing at the bottom requests the next keyset page.
                Color.clear
                    .frame(height: 1)
                        .onAppear {
                        Task { await pager.loadNextPage() }
                    }
            }

            if let error = pager.error {
                Text(error)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        #if os(iOS)
        .listStyle(.plain)
        #else
        .listStyle(.inset)
        #endif
        .refreshable { await pager.refresh() }
        .overlay {
            if pager.hasLoaded, pager.items.isEmpty, !pager.isLoading {
                emptyState
                    .allowsHitTesting(false)
            }
        }
        .task { await pager.loadInitialIfNeeded() }
    }

    private var emptyState: some View {
        VStack(spacing: 8) {
            Image(systemName: "clock")
                .font(.system(size: 36))
                .foregroundStyle(.tertiary)
            Text("No history yet")
                .font(.headline)
            Text("Tracks appear here once you’ve listened to half of them, or four minutes.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
            if let error = pager.error {
                Text(error)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }
        }
        .padding(24)
    }

    private var sections: [DaySection] {
        let calendar = Calendar.current
        var sections: [DaySection] = []
        for record in pager.items {
            let day = calendar.startOfDay(for: record.startedAt)
            if sections.last?.day == day {
                sections[sections.count - 1].records.append(record)
            } else {
                sections.append(DaySection(day: day, records: [record]))
            }
        }
        return sections
    }

    private func dayTitle(_ day: Date) -> String {
        let calendar = Calendar.current
        if calendar.isDateInToday(day) { return "Today" }
        if calendar.isDateInYesterday(day) { return "Yesterday" }
        return day.formatted(.dateTime.weekday(.abbreviated).month(.abbreviated).day())
    }

    private struct DaySection: Identifiable {
        let day: Date
        var records: [ScrobbleRecord]
        var id: Date { day }
    }
}
