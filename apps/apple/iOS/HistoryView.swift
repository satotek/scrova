import SwiftUI

struct HistoryView: View {
    @Bindable var model: AppModel

    var body: some View {
        NavigationStack {
            HistoryListView(
                pager: model.history,
                pendingCount: model.pendingCount,
                isSending: model.isSending,
                onSendNow: model.flush
            )
            .navigationTitle("History")
        }
        .onAppear {
            model.history.invalidate()
            Task { await model.history.loadInitialIfNeeded() }
        }
    }
}
