package dev.satotek.scrova.playback

import android.media.MediaMetadata
import android.media.session.MediaController
import android.media.session.PlaybackState
import android.os.SystemClock
import java.time.Duration
import java.time.Instant
import uniffi.scrova_core.PlaybackEvent
import uniffi.scrova_core.Track
import uniffi.scrova_core.PlaybackState as CoreState

/** Converts a media session's current state into the engine's snapshot type. */
internal object PlaybackSnapshot {
    fun event(controller: MediaController, now: Instant = Instant.now()): PlaybackEvent {
        val playback = controller.playbackState
        val state = state(playback)
        val track = if (state == CoreState.STOPPED) null else track(controller.metadata)
        return PlaybackEvent(
            track = track,
            state = if (track == null) CoreState.STOPPED else state,
            position = track?.let { position(playback) },
            source = controller.packageName,
            observedAt = now,
        )
    }

    /**
     * Only an explicit PLAYING counts as listening. Buffering, connecting and
     * skipping are transitions the player reports while no audio is heard, the
     * same reasoning as treating scrubbing as paused on Apple platforms.
     */
    private fun state(playback: PlaybackState?): CoreState = when (playback?.state) {
        PlaybackState.STATE_PLAYING -> CoreState.PLAYING
        null, PlaybackState.STATE_NONE, PlaybackState.STATE_STOPPED, PlaybackState.STATE_ERROR -> CoreState.STOPPED
        else -> CoreState.PAUSED
    }

    /**
     * `position` is only valid at `lastPositionUpdateTime` (elapsedRealtime);
     * players do not refresh it continuously, so carry it forward to now.
     */
    private fun position(playback: PlaybackState?): Duration? {
        playback ?: return null
        val reported = playback.position
        if (reported < 0) return null
        var millis = reported
        if (playback.state == PlaybackState.STATE_PLAYING && playback.lastPositionUpdateTime > 0) {
            val elapsed = SystemClock.elapsedRealtime() - playback.lastPositionUpdateTime
            millis += (elapsed * playback.playbackSpeed).toLong().coerceAtLeast(0)
        }
        return Duration.ofMillis(millis)
    }

    fun track(metadata: MediaMetadata?): Track? {
        metadata ?: return null
        val title = metadata.text(MediaMetadata.METADATA_KEY_TITLE)
            ?: metadata.text(MediaMetadata.METADATA_KEY_DISPLAY_TITLE)
            ?: return null
        val artist = metadata.text(MediaMetadata.METADATA_KEY_ARTIST)
            ?: metadata.text(MediaMetadata.METADATA_KEY_ALBUM_ARTIST)
            ?: metadata.text(MediaMetadata.METADATA_KEY_DISPLAY_SUBTITLE)
            ?: ""
        val duration = metadata.getLong(MediaMetadata.METADATA_KEY_DURATION)
        return Track(
            title = title,
            artist = artist,
            album = metadata.text(MediaMetadata.METADATA_KEY_ALBUM),
            albumArtist = metadata.text(MediaMetadata.METADATA_KEY_ALBUM_ARTIST),
            // Players report 0 or a negative value when the length is unknown.
            duration = if (duration > 0) Duration.ofMillis(duration) else null,
            sourceTrackId = metadata.text(MediaMetadata.METADATA_KEY_MEDIA_ID),
        )
    }

    private fun MediaMetadata.text(key: String): String? =
        getString(key)?.trim()?.takeIf { it.isNotEmpty() }
}
