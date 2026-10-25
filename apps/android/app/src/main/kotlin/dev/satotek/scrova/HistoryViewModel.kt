package dev.satotek.scrova

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.scrova_ffi.HistoryCursor
import uniffi.scrova_ffi.ScrobbleRecord

data class HistoryState(
    val items: List<ScrobbleRecord> = emptyList(),
    val loading: Boolean = false,
    val refreshing: Boolean = false,
    val hasMore: Boolean = true,
    val error: String? = null,
)

class HistoryViewModel(app: Application) : AndroidViewModel(app) {
    private val scrova = app.scrovaApp.scrova
    private val mutableState = MutableStateFlow(HistoryState())
    val state: StateFlow<HistoryState> = mutableState.asStateFlow()
    private var next: HistoryCursor? = null
    private var request: Job? = null

    fun refresh() {
        request?.cancel()
        load(reset = true)
    }

    fun loadMore() {
        if (request?.isActive == true || !mutableState.value.hasMore) return
        load(reset = false)
    }

    private fun load(reset: Boolean) {
        mutableState.value = mutableState.value.copy(loading = true, refreshing = reset, error = null)
        request = viewModelScope.launch {
            try {
                // Pass the opaque cursor back unchanged; a timestamp alone
                // would lose rows when several scrobbles started together.
                val page = withContext(Dispatchers.IO) {
                    scrova.scrobbleHistory(50u, if (reset) null else next)
                }
                next = page.next
                mutableState.value = HistoryState(
                    items = if (reset) page.items else mutableState.value.items + page.items,
                    hasMore = page.next != null,
                )
            } catch (error: CancellationException) {
                throw error
            } catch (_: Exception) {
                mutableState.value = mutableState.value.copy(
                    loading = false,
                    refreshing = false,
                    error = "Could not load history. Try again.",
                )
            }
        }
    }
}
