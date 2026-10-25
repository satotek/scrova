package dev.satotek.scrova.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.satotek.scrova.MainViewModel
import dev.satotek.scrova.R
import dev.satotek.scrova.account.SignInState
import dev.satotek.scrova.playback.NowPlaying
import uniffi.scrova_core.PlaybackState

@Composable
fun HomeScreen(model: MainViewModel, onHistory: () -> Unit, onSettings: () -> Unit) {
    val nowPlaying by model.nowPlaying.collectAsState()
    val recent by model.recent.collectAsState()
    val signIn by model.signInState.collectAsState()
    val access by model.notificationAccess.collectAsState()

    // A fixed layout: this is the screen people glance at while listening, so
    // everything fits at once and the artwork gives way to the text instead of
    // pushing it off screen.
    Column(
        modifier = Modifier.fillMaxSize().padding(horizontal = 20.dp, vertical = 16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        if (!access) {
            // Nothing can be shown until access is on, so the card is the screen.
            AccessCard()
            Spacer(Modifier.weight(1f))
        } else {
            if (signIn !is SignInState.SignedIn) {
                TextButton(onClick = onSettings, contentPadding = PaddingValues(0.dp)) {
                    Text("Sign in to Last.fm in Settings to send scrobbles.")
                }
            }
            NowPlayingSection(nowPlaying, Modifier.weight(1f))
        }
        recent.firstOrNull()?.let { record -> LastScrobbled(record, onHistory) }
    }
}

@Composable
@OptIn(androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class)
private fun NowPlayingSection(nowPlaying: NowPlaying?, modifier: Modifier = Modifier) {
    val accent = nowPlaying?.tint?.let(::Color) ?: MaterialTheme.colorScheme.primary
    Column(modifier, verticalArrangement = Arrangement.spacedBy(16.dp)) {
        if (nowPlaying == null) {
            // An empty full-width square would be the loudest thing on screen
            // while saying nothing.
            Cover(null, 88, cornerDp = 12)
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text("Nothing playing", style = MaterialTheme.typography.headlineSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text(
                    "Play something in any music app and it shows up here.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.outline,
                )
            }
            return@Column
        }
        // Only the artwork flexes: it takes the height the text and timeline
        // leave, staying square and never wider than the screen.
        val paused = nowPlaying.progress?.state == PlaybackState.PAUSED
        Box(
            Modifier.weight(1f, fill = false).align(Alignment.CenterHorizontally)
                .aspectRatio(1f, matchHeightConstraintsFirst = true).clip(MaterialTheme.shapes.extraLargeIncreased)
                .background(MaterialTheme.colorScheme.surfaceVariant),
            contentAlignment = Alignment.Center,
        ) {
            val art = nowPlaying.artwork
            if (art != null) {
                Image(art.asImageBitmap(), contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
            } else {
                Icon(painterResource(R.drawable.ic_music_note), contentDescription = null, tint = MaterialTheme.colorScheme.outline, modifier = Modifier.size(48.dp))
            }
            // A dimmed cover reads as "paused" at a glance, without another label.
            if (paused) {
                Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.4f)))
                Icon(painterResource(R.drawable.ic_pause), contentDescription = "Paused", tint = Color.White, modifier = Modifier.size(44.dp))
            }
        }
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(nowPlaying.track.title, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (nowPlaying.track.artist.isNotEmpty()) {
                Text(nowPlaying.track.artist, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            nowPlaying.track.album?.takeIf { it.isNotEmpty() }?.let {
                Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.outline, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            // Several apps can play on Android, so say which one this is; set
            // apart from the album so the two are not read as one line of metadata.
            Text(
                nowPlaying.appLabel,
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier
                    .padding(top = 6.dp)
                    .background(MaterialTheme.colorScheme.surfaceContainerHigh, RoundedCornerShape(50))
                    .padding(horizontal = 10.dp, vertical = 4.dp),
            )
        }
        nowPlaying.progress?.let { ScrobbleTimeline(it, accent) }
    }
}

@Composable
internal fun AccessCard() {
    val context = LocalContext.current
    Card(shape = MaterialTheme.shapes.large, colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh)) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Scrova can’t see your music apps yet", style = MaterialTheme.typography.titleMedium)
            Text(
                "Turn on notification access for Scrova. Android grants it so Scrova can read what " +
                    "music apps are playing; Scrova does not read your notifications.",
                style = MaterialTheme.typography.bodyMedium,
            )
            Button(onClick = { openNotificationSettings(context) }) { Text("Open notification access settings") }
            // Sideloaded apps hit Android's restricted settings: the switch is
            // greyed out until the user allows it from App info.
            Text(
                "If the switch is greyed out, open App info, tap ⋮ and choose Allow restricted settings.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            TextButton(onClick = { openAppInfo(context) }) { Text("Open App info") }
        }
    }
}
