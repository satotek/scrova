package dev.satotek.scrova.account

import android.app.Activity
import android.content.Context
import android.net.Uri
import android.os.SystemClock
import android.util.Log
import androidx.browser.customtabs.CustomTabsIntent
import dev.satotek.scrova.scrovaApp
import dev.satotek.scrova.sync.FlushScheduler
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import uniffi.scrova_ffi.Scrova
import uniffi.scrova_ffi.ScrovaException

sealed interface SignInState {
    data object SignedOut : SignInState
    data class AwaitingApproval(val url: String) : SignInState
    data class SignedIn(val username: String) : SignInState
}

class LastfmSignIn(context: Context, private val scrova: Scrova, private val store: SessionStore) {
    private val context = context.applicationContext
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val gate = Mutex()
    private var authorization: Job? = null
    private val mutableState = MutableStateFlow<SignInState>(SignInState.SignedOut)
    val state: StateFlow<SignInState> = mutableState.asStateFlow()
    private val mutableNotice = MutableStateFlow<String?>(null)
    val notice: String? get() = mutableNotice.value
    val notices: StateFlow<String?> = mutableNotice.asStateFlow()

    fun start(activity: Activity) {
        if (!context.scrovaApp.hasLastfmCredentials) {
            mutableNotice.value = "This build has no Last.fm API key, so it cannot sign in."
            return
        }
        if (authorization?.isActive == true || mutableState.value is SignInState.SignedIn) return
        mutableNotice.value = null
        // Do not retain the activity for the hour-long polling job.
        val activityRef = java.lang.ref.WeakReference(activity)
        authorization = scope.launch {
            try {
                val request = scrova.beginAuthorization()
                val deadline = SystemClock.elapsedRealtime() + 60 * 60 * 1000L
                mutableState.value = SignInState.AwaitingApproval(request.url)
                activityRef.get()?.let { open(it, request.url) }
                while (SystemClock.elapsedRealtime() < deadline) {
                    delay(3_000)
                    try {
                        val remaining = deadline - SystemClock.elapsedRealtime()
                        if (remaining <= 0) break
                        val account = withTimeout(remaining) { scrova.completeAuthorization(request.token) }
                        val caller = currentCoroutineContext()
                        // DataStore may finish its atomic write after cancellation.
                        // Complete or undo that write before sign-out can clear it.
                        withContext(NonCancellable) {
                            gate.withLock {
                                if (!caller.isActive) return@withLock
                                store.save(LastfmSession(account.sessionKey, account.username))
                                if (!caller.isActive) {
                                    store.clear()
                                } else {
                                    scrova.setSessionKey(account.sessionKey)
                                    mutableState.value = SignInState.SignedIn(account.username)
                                }
                            }
                        }
                        if (caller.isActive) FlushScheduler.flushSoon(context)
                        return@launch
                    } catch (_: ScrovaException.AuthorizationPending) {
                        // The approval remains usable after a transient network failure.
                    } catch (_: ScrovaException.Network) {
                    }
                }
                mutableNotice.value = "The Last.fm sign-in timed out. Try again."
                mutableState.value = SignInState.SignedOut
            } catch (_: TimeoutCancellationException) {
                mutableNotice.value = "The Last.fm sign-in timed out. Try again."
                mutableState.value = SignInState.SignedOut
            } catch (error: CancellationException) {
                throw error
            } catch (error: Exception) {
                // ScrovaException messages carry API error text, never credentials.
                Log.w(TAG, "Last.fm sign-in failed: ${error.javaClass.simpleName}: ${error.message}")
                mutableNotice.value = "Could not sign in to Last.fm. Try again."
                mutableState.value = SignInState.SignedOut
            }
        }
    }

    fun reopen(activity: Activity) {
        (mutableState.value as? SignInState.AwaitingApproval)?.let { open(activity, it.url) }
    }

    private fun open(activity: Activity, url: String) {
        try {
            CustomTabsIntent.Builder().build().launchUrl(activity, Uri.parse(url))
        } catch (_: Exception) {
            mutableNotice.value = "Could not open a browser. Install a browser and try again."
        }
    }

    fun cancel() {
        authorization?.cancel()
        authorization = null
        if (mutableState.value is SignInState.AwaitingApproval) mutableState.value = SignInState.SignedOut
        mutableNotice.value = null
    }

    suspend fun signOut() {
        cancel()
        gate.withLock {
            scrova.setSessionKey(null)
            mutableState.value = SignInState.SignedOut
            store.clear()
        }
    }

    suspend fun restore() {
        gate.withLock {
            if (authorization?.isActive == true) return
            val session = store.load()
            scrova.setSessionKey(session?.sessionKey)
            mutableState.value = session?.let { SignInState.SignedIn(it.username) } ?: SignInState.SignedOut
        }
    }
}

private const val TAG = "ScrovaSignIn"
