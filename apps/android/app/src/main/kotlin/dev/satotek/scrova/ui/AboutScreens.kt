package dev.satotek.scrova.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import dev.satotek.scrova.BuildConfig
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import uniffi.scrova_ffi.coreVersion

@Composable
fun AboutScreen(onPrivacy: () -> Unit, onLicenses: () -> Unit) {
    LazyColumn(contentPadding = PaddingValues(20.dp), verticalArrangement = Arrangement.spacedBy(20.dp)) {
        item {
            Text("Scrova", style = MaterialTheme.typography.headlineLarge)
            Text("${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})", style = MaterialTheme.typography.bodyLarge)
            Text("Core ${coreVersion()}", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        item { Text("Scrova keeps your Last.fm profile up to date with the music you play.", style = MaterialTheme.typography.bodyLarge) }
        item {
            Column {
                SettingsLink("Privacy policy", onClick = onPrivacy)
                SettingsLink("Open-source licenses", onClick = onLicenses)
            }
        }
        item { Text("Scrobbling uses the Last.fm API. Scrova is not affiliated with Last.fm.", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
    }
}

@Composable
fun PrivacyScreen() {
    val context = LocalContext.current
    val document by produceState<String?>(null) {
        value = withContext(Dispatchers.IO) { context.assets.open("privacy-policy.md").bufferedReader().use { it.readText() } }
    }
    val blocks = remember(document) { document?.let(::markdownBlocks).orEmpty() }
    LazyColumn(contentPadding = PaddingValues(20.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        items(blocks) { block ->
            val style = when (block.heading) {
                1 -> MaterialTheme.typography.headlineMedium
                2 -> MaterialTheme.typography.titleLarge
                0 -> MaterialTheme.typography.bodyLarge
                else -> MaterialTheme.typography.titleMedium
            }
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                if (block.bullet) Text("•", style = style)
                Text(markdownInline(block.text), style = style)
            }
        }
    }
}

@Composable
private fun markdownInline(text: String): AnnotatedString {
    val linkStyle = SpanStyle(color = MaterialTheme.colorScheme.primary)
    return buildAnnotatedString {
        val tokens = Regex("\\[([^]]+)\\]\\((https?://[^)]+)\\)|\\*\\*([^*]+)\\*\\*")
        var cursor = 0
        tokens.findAll(text).forEach { match ->
            append(text.substring(cursor, match.range.first))
            if (match.groupValues[1].isNotEmpty()) {
                withLink(LinkAnnotation.Url(match.groupValues[2], TextLinkStyles(style = linkStyle))) { append(match.groupValues[1]) }
            } else {
                withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(match.groupValues[3]) }
            }
            cursor = match.range.last + 1
        }
        append(text.substring(cursor))
    }
}

private data class LicenseEntry(val name: String, val version: String, val license: String, val repository: String)

@Composable
fun LicensesScreen() {
    val context = LocalContext.current
    val entries by produceState<List<LicenseEntry>?>(null) {
        value = withContext(Dispatchers.IO) {
            listOf("licenses-rust.json", "licenses-android.json").flatMap { asset ->
                val array = JSONArray(context.assets.open(asset).bufferedReader().use { it.readText() })
                List(array.length()) { index ->
                    array.getJSONObject(index).let {
                        LicenseEntry(it.getString("name"), it.getString("version"), it.getString("license"), it.optString("repository"))
                    }
                }
            }.sortedBy { it.name.lowercase() }
        }
    }
    LazyColumn(contentPadding = PaddingValues(20.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        item { Text("Scrova is built with these open-source libraries.", style = MaterialTheme.typography.bodyLarge) }
        items(entries.orEmpty()) { entry ->
            Column(Modifier.fillMaxWidth().padding(vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text("${entry.name} ${entry.version}", style = MaterialTheme.typography.titleMedium)
                Text(entry.license, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                if (entry.repository.startsWith("https://")) {
                    TextButton(onClick = { openExternal(context, entry.repository) }, contentPadding = PaddingValues(0.dp)) { Text("Project website") }
                }
            }
            HorizontalDivider()
        }
    }
}
