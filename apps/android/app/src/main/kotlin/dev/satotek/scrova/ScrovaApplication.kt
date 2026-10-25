package dev.satotek.scrova

import android.app.Application
import dev.satotek.scrova.account.signIn
import dev.satotek.scrova.sync.FlushScheduler
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.launch
import uniffi.scrova_ffi.Scrova
import uniffi.scrova_ffi.ScrovaConfig

/**
 * Owns the one Rust core for the process. The notification listener, the
 * activity and background workers all run in this process and must share it:
 * two cores would run two engines and race on the same queue.
 */
class ScrovaApplication : Application() {
    lateinit var scrova: Scrova
        private set

    override fun onCreate() {
        super.onCreate()
        scrova = Scrova(
            ScrovaConfig(
                databasePath = getDatabasePath("scrova.sqlite").apply { parentFile?.mkdirs() }.path,
                lastfmApiKey = BuildConfig.LASTFM_API_KEY,
                lastfmSharedSecret = BuildConfig.LASTFM_SHARED_SECRET,
                lastfmBaseUrl = null,
            ),
        )
        // The process is often started by the system binding the notification
        // listener, with no activity; the session must be restored either way
        // or scrobbles queue up unsent.
        MainScope().launch { signIn.restore() }
        FlushScheduler.ensurePeriodic(this)
    }

    val hasLastfmCredentials: Boolean
        get() = BuildConfig.LASTFM_API_KEY.isNotEmpty() && BuildConfig.LASTFM_SHARED_SECRET.isNotEmpty()
}

val android.content.Context.scrovaApp: ScrovaApplication
    get() = applicationContext as ScrovaApplication
