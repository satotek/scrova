package dev.satotek.scrova

import android.app.Activity
import android.app.Application
import androidx.core.app.NotificationManagerCompat
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.satotek.scrova.account.signIn
import dev.satotek.scrova.playback.PlaybackRepository
import dev.satotek.scrova.sync.FlushScheduler
import kotlinx.coroutines.delay
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.withContext
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import uniffi.scrova_ffi.ScrobbleRecord

class MainViewModel(app: Application) : AndroidViewModel(app) {
    private val scrova = app.scrovaApp.scrova
    private val signIn = app.signIn
    private var queueRefresh: Job? = null

    val nowPlaying = PlaybackRepository.nowPlaying
    val signInState = signIn.state
    val signInNotice = signIn.notices

    private val recentState = MutableStateFlow<List<ScrobbleRecord>>(emptyList())
    val recent: StateFlow<List<ScrobbleRecord>> = recentState.asStateFlow()

    private val pendingState = MutableStateFlow(0uL)
    val pending: StateFlow<ULong> = pendingState.asStateFlow()

    private val accessState = MutableStateFlow(false)
    /** Whether notification access is switched on for Scrova in system settings. */
    val notificationAccess: StateFlow<Boolean> = accessState.asStateFlow()

    val hasLastfmCredentials = app.scrovaApp.hasLastfmCredentials

    init {
        viewModelScope.launch {
            PlaybackRepository.queueChanged.collect { refreshQueue() }
        }
        viewModelScope.launch {
            // Delivery happens in a worker this model does not hear from;
            // re-reading the small queue is cheaper than plumbing callbacks.
            while (true) {
                refreshQueue()
                delay(QUEUE_REFRESH_MILLIS)
            }
        }
    }

    /** Called on every resume: the user may have just come back from settings. */
    fun onResume() {
        accessState.value = getApplication<Application>().let { app ->
            NotificationManagerCompat.getEnabledListenerPackages(app).contains(app.packageName)
        }
        FlushScheduler.flushSoon(getApplication())
        refreshQueue()
    }

    fun signIn(activity: Activity) = signIn.start(activity)
    fun reopenApproval(activity: Activity) = signIn.reopen(activity)
    fun cancelSignIn() = signIn.cancel()
    fun signOut() = viewModelScope.launch { signIn.signOut() }
    fun sendNow() = FlushScheduler.flushSoon(getApplication())

    private fun refreshQueue() {
        if (queueRefresh?.isActive == true) return
        queueRefresh = viewModelScope.launch {
            runCatching {
                // SQLite work must not block navigation or the progress animation.
                val (records, count) = withContext(Dispatchers.IO) {
                    scrova.recentScrobbles(RECENT_LIMIT) to scrova.pendingCount()
                }
                recentState.value = records
                pendingState.value = count
            }
        }
    }

    private companion object {
        const val RECENT_LIMIT = 20u
        const val QUEUE_REFRESH_MILLIS = 10_000L
    }
}
