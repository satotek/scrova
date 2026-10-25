package dev.satotek.scrova.ui

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.widget.Toast
import dev.satotek.scrova.playback.MediaObserverService

internal fun lastfmTrackUrl(artist: String, title: String): String =
    "https://www.last.fm/music/${encodePathSegment(artist)}/_/${encodePathSegment(title)}"

internal fun lastfmProfileUrl(username: String): String = "https://www.last.fm/user/${encodePathSegment(username)}"

private fun encodePathSegment(value: String): String = java.net.URLEncoder.encode(value, "UTF-8").replace("+", "%20")

internal fun openExternal(context: Context, url: String) {
    launch(context, Intent(Intent.ACTION_VIEW, Uri.parse(url)))
}

internal fun openAppInfo(context: Context) {
    launch(context, Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", context.packageName, null)))
}

internal fun openNotificationSettings(context: Context) {
    val detail = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
        Intent(Settings.ACTION_NOTIFICATION_LISTENER_DETAIL_SETTINGS).putExtra(
            Settings.EXTRA_NOTIFICATION_LISTENER_COMPONENT_NAME,
            ComponentName(context, MediaObserverService::class.java).flattenToString(),
        )
    } else null
    if (detail == null || !launch(context, detail, notify = false)) {
        launch(context, Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS))
    }
}

private fun launch(context: Context, intent: Intent, notify: Boolean = true): Boolean = try {
    context.startActivity(intent)
    true
} catch (_: android.content.ActivityNotFoundException) {
    if (notify) Toast.makeText(context, "No app can open this link or setting.", Toast.LENGTH_SHORT).show()
    false
}
