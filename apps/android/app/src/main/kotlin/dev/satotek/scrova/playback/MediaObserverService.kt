package dev.satotek.scrova.playback

import android.content.ComponentName
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.media.MediaMetadata
import android.media.session.MediaController
import android.media.session.MediaSession
import android.media.session.MediaSessionManager
import android.media.session.PlaybackState
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.service.notification.NotificationListenerService
import android.util.Log
import dev.satotek.scrova.scrovaApp
import dev.satotek.scrova.sync.FlushScheduler
import java.time.Instant
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import uniffi.scrova_core.EngineOutput
import uniffi.scrova_core.PlaybackEvent
import uniffi.scrova_core.PlaybackState as CoreState

/**
 * Observes every app's media session. Notification access is the documented
 * way to call `getActiveSessions` for other apps; notifications themselves
 * are never read. The system keeps this service bound (and the process alive)
 * while access is granted, which is what makes background scrobbling work.
 *
 * Everything runs on the main thread: callbacks arrive there, and keeping the
 * engine calls on one thread preserves event order per source.
 */
class MediaObserverService : NotificationListenerService() {
    private val handler = Handler(Looper.getMainLooper())
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val sessions = mutableMapOf<MediaSession.Token, Observed>()
    private var sessionManager: MediaSessionManager? = null

    /** The session shown in the UI: the one that most recently started playing. */
    private var primary: MediaSession.Token? = null

    private class Observed(
        val controller: MediaController,
        val callback: MediaController.Callback,
        var artworkKey: String? = null,
        var artwork: Bitmap? = null,
        var tint: Int? = null,
    )

    private val sessionsChanged = MediaSessionManager.OnActiveSessionsChangedListener { controllers ->
        sync(controllers.orEmpty())
    }

    /**
     * Players only call back on changes, but the engine credits listening time
     * between observations and scrobbles once a threshold is crossed. Ticking
     * while something plays makes that happen on time, not at the next skip.
     */
    private var ticking = false
    private val ticker = Runnable {
        ticking = false
        sessions.values
            .filter { it.controller.playbackState?.state == PlaybackState.STATE_PLAYING }
            .forEach { report(it) }
    }

    /** report() reschedules this while anything plays, so it stops by itself. */
    private fun ensureTicking() {
        if (ticking) return
        ticking = true
        handler.postDelayed(ticker, TICK_MILLIS)
    }

    override fun onListenerConnected() {
        PlaybackRepository.setListenerConnected(true)
        val manager = getSystemService(MediaSessionManager::class.java)
        val component = ComponentName(this, MediaObserverService::class.java)
        sessionManager = manager
        // Throws SecurityException if access was revoked between bind and here.
        runCatching {
            manager.addOnActiveSessionsChangedListener(sessionsChanged, component, handler)
            sync(manager.getActiveSessions(component))
        }.onFailure { Log.w(TAG, "Cannot read media sessions", it) }
    }

    override fun onListenerDisconnected() {
        sessionManager?.removeOnActiveSessionsChangedListener(sessionsChanged)
        sessionManager = null
        sync(emptyList())
        PlaybackRepository.setListenerConnected(false)
    }

    override fun onDestroy() {
        handler.removeCallbacks(ticker)
        scope.cancel()
        super.onDestroy()
    }

    private fun sync(controllers: List<MediaController>) {
        val current = controllers.associateBy { it.sessionToken }
        for (token in sessions.keys - current.keys) {
            forget(token)
        }
        for ((token, controller) in current) {
            if (token in sessions) continue
            val observed = Observed(controller, callbackFor(token))
            sessions[token] = observed
            controller.registerCallback(observed.callback, handler)
            report(observed)
        }
        publish()
    }

    private fun callbackFor(token: MediaSession.Token) = object : MediaController.Callback() {
        override fun onPlaybackStateChanged(state: PlaybackState?) {
            sessions[token]?.let(::report)
        }

        override fun onMetadataChanged(metadata: MediaMetadata?) {
            sessions[token]?.let(::report)
        }

        override fun onSessionDestroyed() {
            forget(token)
            publish()
        }
    }

    private fun forget(token: MediaSession.Token) {
        val observed = sessions.remove(token) ?: return
        observed.controller.unregisterCallback(observed.callback)
        // Close the engine session so the time after the player went away is
        // not credited to its last track.
        feed(
            PlaybackEvent(
                track = null,
                state = CoreState.STOPPED,
                position = null,
                source = observed.controller.packageName,
                observedAt = Instant.now(),
            ),
        )
        if (primary == token) primary = null
    }

    private fun report(observed: Observed) {
        val event = PlaybackSnapshot.event(observed.controller)
        updateArtwork(observed, event)
        if (event.state == CoreState.PLAYING) {
            primary = observed.controller.sessionToken
            ensureTicking()
        }
        feed(event)
        publish()
    }

    private fun feed(event: PlaybackEvent) {
        val scrova = scrovaApp.scrova
        val outputs = try {
            scrova.handlePlaybackEvent(event)
        } catch (error: Exception) {
            Log.w(TAG, "Engine rejected an event", error)
            return
        }
        for (output in outputs) {
            when (output) {
                is EngineOutput.NowPlaying -> if (scrova.isSignedIn()) {
                    // Last.fm asks clients not to retry Now Playing; drop failures.
                    scope.launch { runCatching { scrova.updateNowPlaying(output.track) } }
                }
                is EngineOutput.Scrobble -> {
                    PlaybackRepository.notifyQueueChanged()
                    FlushScheduler.flushSoon(this)
                }
            }
        }
    }

    /** Covers can be large bitmaps; decode and tint once per track. */
    private fun updateArtwork(observed: Observed, event: PlaybackEvent) {
        val track = event.track
        val key = track?.let { it.sourceTrackId ?: "${it.artist}\u001f${it.title}" }
        if (key == observed.artworkKey) return
        observed.artworkKey = key
        val bitmap = track?.let { SessionArtwork.load(this, observed.controller.metadata) }
        observed.artwork = bitmap
        observed.tint = bitmap?.let(ArtworkTint::color)
        if (bitmap != null && track != null) ArtworkCache.store(this, bitmap, track)
    }

    private fun publish() {
        val token = primary ?: sessions.keys.firstOrNull()
        val observed = token?.let(sessions::get)
        val event = observed?.let { PlaybackSnapshot.event(it.controller) }
        val track = event?.track
        if (observed == null || track == null) {
            PlaybackRepository.publish(null)
            return
        }
        val source = observed.controller.packageName
        PlaybackRepository.publish(
            NowPlaying(
                source = source,
                appLabel = appLabel(source),
                track = track,
                progress = runCatching { scrovaApp.scrova.sessionProgress(source) }.getOrNull(),
                artwork = observed.artwork,
                tint = observed.tint,
            ),
        )
    }

    private fun appLabel(packageName: String): String = runCatching {
        val info = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            packageManager.getApplicationInfo(packageName, PackageManager.ApplicationInfoFlags.of(0))
        } else {
            @Suppress("DEPRECATION")
            packageManager.getApplicationInfo(packageName, 0)
        }
        packageManager.getApplicationLabel(info).toString()
    }.getOrDefault(packageName)

    private companion object {
        const val TAG = "ScrovaObserver"
        /** Well under the engine's 10s repeat window, like the Apple observers. */
        const val TICK_MILLIS = 3_000L
    }
}
