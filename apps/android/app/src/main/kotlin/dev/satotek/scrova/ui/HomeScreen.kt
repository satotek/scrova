package dev.satotek.scrova.ui

import android.app.Activity
import android.content.ComponentName
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
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
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledTonalButton
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
import androidx.compose.ui.unit.dp
import dev.satotek.scrova.MainViewModel
import dev.satotek.scrova.R
import dev.satotek.scrova.account.SignInState
import dev.satotek.scrova.playback.MediaObserverService
import dev.satotek.scrova.playback.NowPlaying
import uniffi.scrova_core.PlaybackState

@Composable
fun HomeScreen(model: MainViewModel) {
    val nowPlaying by model.nowPlaying.collectAsState()
    val recent by model.recent.collectAsState()
    val pending by model.pending.collectAsState()
    val access by model.notificationAccess.collectAsState()

    LazyColumn(
        modifier = Modifier.fillMaxSize().safeDrawingPadding(),
        contentPadding = PaddingValues(horizontal = 20.dp, vertical = 16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (!access) {
            item { AccessCard() }
        }
        item { NowPlayingSection(nowPlaying, Modifier.padding(bottom = 18.dp)) }
        item {
            Row(Modifier.fillMaxWidth().padding(top = 8.dp), verticalAlignment = Alignment.Bottom) {
                Text("Recent scrobbles", style = MaterialTheme.typography.titleSmall, modifier = Modifier.weight(1f))
                if (pending > 0u) {
                    Text("$pending waiting to send", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
        if (recent.isEmpty()) {
            item {
                Text(
                    "Tracks appear here once you’ve listened to half of them, or four minutes.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else {
            items(recent) { RecentScrobbleCard(it) }
        }
        item { AccountSection(model, Modifier.padding(top = 18.dp)) }
    }
}

@Composable
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
        val paused = nowPlaying.progress?.state == PlaybackState.PAUSED
        Box(
            Modifier.fillMaxWidth().aspectRatio(1f).clip(RoundedCornerShape(16.dp))
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
            Text(nowPlaying.track.title, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
            if (nowPlaying.track.artist.isNotEmpty()) {
                Text(nowPlaying.track.artist, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            nowPlaying.track.album?.takeIf { it.isNotEmpty() }?.let {
                Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.outline)
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
private fun AccessCard() {
    val context = LocalContext.current
    Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.secondaryContainer)) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Scrova can’t see your music apps yet", style = MaterialTheme.typography.titleMedium)
            Text(
                "Turn on notification access for Scrova. Android grants it so Scrova can read what " +
                    "music apps are playing; Scrova does not read your notifications.",
                style = MaterialTheme.typography.bodyMedium,
            )
            Button(onClick = {
                val component = ComponentName(context, MediaObserverService::class.java)
                val detail = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                    Intent(Settings.ACTION_NOTIFICATION_LISTENER_DETAIL_SETTINGS)
                        .putExtra(Settings.EXTRA_NOTIFICATION_LISTENER_COMPONENT_NAME, component.flattenToString())
                } else {
                    null
                }
                val fallback = Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS)
                runCatching { context.startActivity(detail ?: fallback) }
                    .onFailure { context.startActivity(fallback) }
            }) { Text("Open notification access settings") }
            // Sideloaded apps hit Android's restricted settings: the switch is
            // greyed out until the user allows it from App info.
            Text(
                "If the switch is greyed out, open App info, tap ⋮ and choose Allow restricted settings.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSecondaryContainer,
            )
            TextButton(onClick = {
                context.startActivity(
                    Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", context.packageName, null)),
                )
            }) { Text("Open App info") }
        }
    }
}

@Composable
private fun AccountSection(model: MainViewModel, modifier: Modifier = Modifier) {
    val state by model.signInState.collectAsState()
    val notice by model.signInNotice.collectAsState()
    val activity = LocalContext.current as Activity
    Column(modifier, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text("Last.fm", style = MaterialTheme.typography.titleSmall)
        when (val current = state) {
            SignInState.SignedOut -> {
                Button(onClick = { model.signIn(activity) }, enabled = model.hasLastfmCredentials) { Text("Sign in to Last.fm") }
                if (!model.hasLastfmCredentials) {
                    Text("This build has no Last.fm API key, so it cannot sign in.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            is SignInState.AwaitingApproval -> {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                    Text("Approve Scrova on Last.fm", style = MaterialTheme.typography.bodyLarge)
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    FilledTonalButton(onClick = { model.reopenApproval(activity) }) { Text("Open approval page again") }
                    TextButton(onClick = model::cancelSignIn) { Text("Cancel") }
                }
            }
            is SignInState.SignedIn -> {
                Text("Scrobbling as ${current.username}", style = MaterialTheme.typography.bodyLarge)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    FilledTonalButton(onClick = model::sendNow) { Text("Send now") }
                    TextButton(onClick = { model.signOut() }) { Text("Sign out") }
                }
            }
        }
        notice?.let {
            Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}
