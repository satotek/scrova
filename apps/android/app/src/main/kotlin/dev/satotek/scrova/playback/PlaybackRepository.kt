package dev.satotek.scrova.playback

import android.graphics.Bitmap
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import uniffi.scrova_core.SessionProgress
import uniffi.scrova_core.Track

/** What the UI shows as "now playing": one session, even if several apps have one. */
data class NowPlaying(
    val source: String,
    val appLabel: String,
    val track: Track,
    val progress: SessionProgress?,
    val artwork: Bitmap?,
    /** ARGB accent picked from the artwork, or null for greyscale covers. */
    val tint: Int?,
)

/**
 * Hand-off between the notification listener (which observes) and the
 * activity (which displays). Both live in the app process; the listener runs
 * whether or not the activity exists, so it publishes here instead of the
 * activity polling it.
 */
object PlaybackRepository {
    private val nowPlayingState = MutableStateFlow<NowPlaying?>(null)
    val nowPlaying: StateFlow<NowPlaying?> = nowPlayingState.asStateFlow()

    private val listenerConnectedState = MutableStateFlow(false)
    /** True while the system has the listener bound, i.e. access is granted and working. */
    val listenerConnected: StateFlow<Boolean> = listenerConnectedState.asStateFlow()

    private val queueChangedEvents = MutableSharedFlow<Unit>(extraBufferCapacity = 1)
    /** Fires when a scrobble was queued, so the history can refresh. */
    val queueChanged: SharedFlow<Unit> = queueChangedEvents.asSharedFlow()

    internal fun publish(nowPlaying: NowPlaying?) {
        nowPlayingState.value = nowPlaying
    }

    internal fun setListenerConnected(connected: Boolean) {
        listenerConnectedState.value = connected
    }

    internal fun notifyQueueChanged() {
        queueChangedEvents.tryEmit(Unit)
    }
}
