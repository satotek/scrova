import Observation
import ScrovaKit

/// Keyset-paged local scrobble history. AppModel owns one of these so the list
/// survives tab switches and the macOS history window.
@MainActor
@Observable
final class HistoryPager {
    static let pageSize: UInt32 = 50

    private let scrova: Scrova?
    private var stale = true

    private(set) var items: [ScrobbleRecord] = []
    private(set) var next: HistoryCursor?
    private(set) var isLoading = false
    private(set) var hasLoaded = false
    private(set) var error: String?

    init(scrova: Scrova?) {
        self.scrova = scrova
    }

    var hasNext: Bool { next != nil }

    func loadInitialIfNeeded() async {
        guard !hasLoaded || stale else { return }
        await refresh()
    }

    func refresh() async {
        guard let scrova else {
            error = "Scrova could not start."
            hasLoaded = true
            stale = false
            return
        }
        guard !isLoading else { return }
        isLoading = true
        defer { isLoading = false }

        do {
            let page = try scrova.scrobbleHistory(limit: Self.pageSize, before: nil)
            items = page.items
            next = page.next
            hasLoaded = true
            stale = false
            error = nil
        } catch {
            self.error = ScrovaSetup.describe(error)
        }
    }

    func loadNextPage() async {
        guard let scrova, let cursor = next, !isLoading else { return }
        isLoading = true
        defer { isLoading = false }

        do {
            let page = try scrova.scrobbleHistory(limit: Self.pageSize, before: cursor)
            items.append(contentsOf: page.items)
            next = page.next
            error = nil
        } catch {
            self.error = ScrovaSetup.describe(error)
        }
    }

    /// Mark the loaded pages stale; the next History appearance refreshes.
    func invalidate() {
        stale = true
    }
}
