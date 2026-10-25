package dev.satotek.scrova.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import dev.satotek.scrova.R
import java.time.Duration
import java.time.Instant
import kotlinx.coroutines.delay
import uniffi.scrova_core.PlaybackState
import uniffi.scrova_core.SessionProgress

/**
 * The track's timeline with a mark where it will scrobble. The mark comes from
 * the engine's listening time, not a fixed fraction: seeks and pauses are not
 * listening, so after a forward seek the mark moves ahead with the playhead.
 */
@Composable
fun ScrobbleTimeline(progress: SessionProgress, accent: Color, modifier: Modifier = Modifier) {
    // Observers report every few seconds; carrying the reading forward keeps
    // the playhead moving at the speed of the music.
    var now by remember { mutableStateOf(Instant.now()) }
    LaunchedEffect(progress) {
        while (true) {
            now = Instant.now()
            delay(1_000)
        }
    }
    val reading = Reading(progress, now)
    val duration = progress.track.duration?.toMillis()?.takeIf { it > 0 }
    val track = MaterialTheme.colorScheme.surfaceVariant
    val played = if (progress.scrobbled) accent else MaterialTheme.colorScheme.onSurfaceVariant

    Column(
        modifier = modifier.semantics { contentDescription = reading.status(progress) },
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        if (duration != null) {
            Canvas(Modifier.fillMaxWidth().height(12.dp)) {
                val barHeight = 4.dp.toPx()
                val top = (size.height - barHeight) / 2
                val radius = CornerRadius(barHeight / 2)
                fun x(millis: Long) = size.width * (millis.toFloat() / duration).coerceIn(0f, 1f)
                drawRoundRect(track, Offset(0f, top), Size(size.width, barHeight), radius)
                drawRoundRect(played, Offset(0f, top), Size(x(reading.positionMillis ?: 0), barHeight), radius)
                val mark = reading.scrobbleAtMillis
                if (!progress.scrobbled && mark != null && mark <= duration) {
                    val width = 3.dp.toPx()
                    drawRoundRect(accent, Offset(x(mark) - width / 2, 0f), Size(width, size.height), CornerRadius(1.dp.toPx()))
                }
            }
        }
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            val style = MaterialTheme.typography.labelMedium
            val muted = MaterialTheme.colorScheme.onSurfaceVariant
            Text(clock(reading.positionMillis ?: 0), style = style, color = muted)
            Row(Modifier.weight(1f), horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
                if (progress.scrobbled) {
                    Icon(painterResource(R.drawable.ic_check), contentDescription = null, tint = accent, modifier = Modifier.height(14.dp))
                    Text("Scrobbled", style = style, color = accent)
                } else {
                    Text(reading.status(progress), style = style, color = muted)
                }
            }
            Text(duration?.let(::clock) ?: "", style = style, color = muted)
        }
    }
}

private class Reading(progress: SessionProgress, now: Instant) {
    val positionMillis: Long?
    val remainingMillis: Long?
    /** Where the playhead will be when it scrobbles, if playback continues. */
    val scrobbleAtMillis: Long?

    init {
        val elapsed = if (progress.state == PlaybackState.PLAYING) {
            Duration.between(progress.observedAt, now).toMillis().coerceAtLeast(0)
        } else {
            0
        }
        val duration = progress.track.duration?.toMillis() ?: Long.MAX_VALUE
        positionMillis = progress.position?.toMillis()?.let { minOf(it + elapsed, duration) }
        val threshold = progress.threshold
        if (threshold != null && !progress.scrobbled) {
            remainingMillis = (threshold.toMillis() - progress.played.toMillis() - elapsed).coerceAtLeast(0)
            scrobbleAtMillis = positionMillis?.plus(remainingMillis)
        } else {
            remainingMillis = null
            scrobbleAtMillis = null
        }
    }

    fun status(progress: SessionProgress): String = when {
        progress.scrobbled -> "Scrobbled"
        remainingMillis != null -> "Scrobbles in ${clock(remainingMillis)}"
        (progress.track.duration?.seconds ?: Long.MAX_VALUE) <= 30 -> "Too short to scrobble"
        else -> "Can’t scrobble without a title and artist"
    }
}

internal fun clock(millis: Long): String {
    val seconds = (millis / 1000).coerceAtLeast(0)
    return "%d:%02d".format(seconds / 60, seconds % 60)
}
