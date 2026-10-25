package dev.satotek.scrova.playback

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Color
import android.media.MediaMetadata
import android.net.Uri
import java.io.File
import java.security.MessageDigest
import uniffi.scrova_core.Track

/** Reads the cover a player attached to its media session. */
internal object SessionArtwork {
    fun load(context: Context, metadata: MediaMetadata?): Bitmap? {
        metadata ?: return null
        for (key in listOf(MediaMetadata.METADATA_KEY_ALBUM_ART, MediaMetadata.METADATA_KEY_ART)) {
            metadata.getBitmap(key)?.let { return it }
        }
        // Some players only publish a content URI they have granted to readers.
        for (key in listOf(MediaMetadata.METADATA_KEY_ALBUM_ART_URI, MediaMetadata.METADATA_KEY_ART_URI)) {
            val uri = metadata.getString(key)?.let(Uri::parse) ?: continue
            if (uri.scheme != "content" && uri.scheme != "file") continue
            runCatching {
                context.contentResolver.openInputStream(uri)?.use(BitmapFactory::decodeStream)
            }.getOrNull()?.let { return it }
        }
        return null
    }
}

/**
 * Small on-disk thumbnails so the scrobble history can show covers after the
 * player has moved on; neither the media session nor the Rust queue keeps them.
 * Keyed per album like the Apple cache, since tracks of an album share a cover.
 */
object ArtworkCache {
    private const val MAX_SIDE = 240

    fun store(context: Context, bitmap: Bitmap, track: Track) {
        val file = file(context, track)
        if (file.exists()) return
        val scale = minOf(1f, MAX_SIDE.toFloat() / maxOf(bitmap.width, bitmap.height))
        val thumbnail = Bitmap.createScaledBitmap(
            bitmap,
            (bitmap.width * scale).toInt().coerceAtLeast(1),
            (bitmap.height * scale).toInt().coerceAtLeast(1),
            true,
        )
        runCatching {
            file.parentFile?.mkdirs()
            file.outputStream().use { thumbnail.compress(Bitmap.CompressFormat.JPEG, 80, it) }
        }
    }

    fun load(context: Context, track: Track): Bitmap? {
        val file = file(context, track)
        return if (file.exists()) BitmapFactory.decodeFile(file.path) else null
    }

    private fun file(context: Context, track: Track): File {
        val artist = track.albumArtist ?: track.artist
        val key = track.album?.let { "album|$artist|$it" } ?: "track|${track.artist}|${track.title}"
        val digest = MessageDigest.getInstance("SHA-256").digest(key.lowercase().toByteArray())
        val name = digest.take(16).joinToString("") { "%02x".format(it) }
        return File(context.cacheDir, "artwork/$name.jpg")
    }
}

/**
 * Accent colour from the cover, so the scrobble mark belongs to the track that
 * is playing. A plain average turns most covers into mud, so this keeps the
 * most vivid pixel of a tiny thumbnail (same rule as the Apple apps).
 */
object ArtworkTint {
    fun color(bitmap: Bitmap): Int? {
        val small = Bitmap.createScaledBitmap(bitmap, 12, 12, true)
        val hsv = FloatArray(3)
        var best: FloatArray? = null
        var bestScore = 0f
        for (y in 0 until small.height) {
            for (x in 0 until small.width) {
                Color.colorToHSV(small.getPixel(x, y), hsv)
                // Near-black pixels have unstable hues.
                if (hsv[2] <= 0.25f) continue
                val score = hsv[1] * hsv[2]
                if (score > bestScore) {
                    bestScore = score
                    best = hsv.copyOf()
                }
            }
        }
        // Greyscale covers have no colour worth borrowing.
        val chosen = best?.takeIf { it[1] > 0.2f } ?: return null
        // Fixed brightness keeps the mark legible on light and dark surfaces.
        return Color.HSVToColor(floatArrayOf(chosen[0], minOf(chosen[1], 0.7f), 0.85f))
    }
}
