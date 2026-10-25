package dev.satotek.scrova.ui

import android.graphics.Bitmap
import android.text.format.DateUtils
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import dev.satotek.scrova.R
import dev.satotek.scrova.playback.ArtworkCache
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.scrova_core.TimestampAccuracy
import uniffi.scrova_ffi.DeliveryStatus
import uniffi.scrova_ffi.ScrobbleRecord

/**
 * One entry of the local scrobble log, shown in full: long titles wrap instead
 * of truncating, since this is where users check what counted. Delivery status
 * only appears when it needs attention.
 */
@Composable
fun RecentScrobbleCard(record: ScrobbleRecord, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val artwork by produceState<Bitmap?>(null, record.track) {
        value = withContext(Dispatchers.IO) { ArtworkCache.load(context, record.track) }
    }
    val when_ = DateUtils.getRelativeTimeSpanString(
        record.startedAt.toEpochMilli(), System.currentTimeMillis(), DateUtils.MINUTE_IN_MILLIS,
        DateUtils.FORMAT_ABBREV_RELATIVE,
    ).toString()
    val status = status(record.status)

    Card(
        modifier = modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
    ) {
        Row(Modifier.padding(12.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Cover(artwork, 56)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(record.track.title, style = MaterialTheme.typography.titleSmall)
                if (record.track.artist.isNotEmpty()) {
                    Text(record.track.artist, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                record.track.album?.takeIf { it.isNotEmpty() }?.let {
                    Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.outline)
                }
                Row(Modifier.padding(top = 2.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    val small = MaterialTheme.typography.labelSmall
                    Text(when_, style = small, color = MaterialTheme.colorScheme.outline)
                    if (record.timestampAccuracy == TimestampAccuracy.ESTIMATED) {
                        Text("Time estimated", style = small, color = MaterialTheme.colorScheme.outline)
                    }
                    status?.let { (text, color) -> Text(text, style = small, color = color ?: MaterialTheme.colorScheme.onSurfaceVariant) }
                }
            }
        }
    }
}

@Composable
fun Cover(bitmap: Bitmap?, sizeDp: Int, modifier: Modifier = Modifier, cornerDp: Int = 8) {
    Box(
        modifier
            .size(sizeDp.dp)
            .clip(RoundedCornerShape(cornerDp.dp))
            .background(MaterialTheme.colorScheme.surfaceVariant)
            .clearAndSetSemantics { },
        contentAlignment = Alignment.Center,
    ) {
        if (bitmap != null) {
            Image(bitmap.asImageBitmap(), contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.size(sizeDp.dp))
        } else {
            Icon(painterResource(R.drawable.ic_music_note), contentDescription = null, tint = MaterialTheme.colorScheme.outline)
        }
    }
}

private fun status(status: DeliveryStatus): Pair<String, Color?>? = when (status) {
    is DeliveryStatus.Pending, is DeliveryStatus.Sending -> "Waiting to send" to null
    is DeliveryStatus.Submitted -> null
    is DeliveryStatus.Ignored -> "Ignored by Last.fm" to Color(0xFFE08A00)
    is DeliveryStatus.Failed -> "Not sent" to Color(0xFFD93025)
}
