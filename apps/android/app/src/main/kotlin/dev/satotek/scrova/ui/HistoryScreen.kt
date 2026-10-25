@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class, androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class)

package dev.satotek.scrova.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import dev.satotek.scrova.HistoryViewModel
import dev.satotek.scrova.MainViewModel
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

internal fun historyDayLabel(day: LocalDate, today: LocalDate = LocalDate.now()): String = when (day) {
    today -> "Today"
    today.minusDays(1) -> "Yesterday"
    else -> day.format(DateTimeFormatter.ofPattern(if (day.year == today.year) "EEE, MMM d" else "EEE, MMM d, yyyy", Locale.getDefault()))
}

@Composable
fun HistoryScreen(model: MainViewModel, historyModel: HistoryViewModel = viewModel()) {
    val state by historyModel.state.collectAsState()
    val pending by model.pending.collectAsState()
    val list = rememberLazyListState()
    val atEnd by remember {
        derivedStateOf { (list.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: -1) >= list.layoutInfo.totalItemsCount - 5 }
    }
    LaunchedEffect(atEnd, state.items.size, state.hasMore, state.loading, state.error) {
        if (atEnd && state.hasMore && !state.loading && state.error == null) historyModel.loadMore()
    }
    // Re-entering History refreshes statuses changed by a background worker.
    LaunchedEffect(Unit) { historyModel.refresh() }
    val zone = ZoneId.systemDefault()
    val groups = remember(state.items, zone) { state.items.groupBy { it.startedAt.atZone(zone).toLocalDate() } }
    PullToRefreshBox(isRefreshing = state.refreshing, onRefresh = historyModel::refresh, modifier = Modifier.fillMaxSize()) {
        LazyColumn(
            state = list,
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(horizontal = 20.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            if (pending > 0uL) {
                item(key = "sending") {
                    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                        Text("$pending waiting to send", modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
                        TextButton(onClick = model::sendNow) { Text("Send now") }
                    }
                }
            }
            if (state.items.isEmpty() && !state.loading) {
                item(key = "empty") {
                    Text("Tracks appear here once you've listened to half of them, or four minutes.", style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(vertical = 32.dp))
                }
            }
            groups.forEach { (day, records) ->
                stickyHeader(key = "day:$day") {
                    Text(historyDayLabel(day), style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.background).padding(vertical = 12.dp))
                }
                items(records) { record -> RecentScrobbleCard(record) }
            }
            if (state.loading && !state.refreshing) {
                item(key = "loading") { Box(Modifier.fillMaxWidth().padding(16.dp), contentAlignment = Alignment.Center) { LoadingIndicator() } }
            }
            state.error?.let { error ->
                item(key = "error") {
                    Column {
                        Text(error, color = MaterialTheme.colorScheme.error)
                        TextButton(onClick = { if (state.items.isEmpty()) historyModel.refresh() else historyModel.loadMore() }) { Text("Try again") }
                    }
                }
            }
        }
    }
}
