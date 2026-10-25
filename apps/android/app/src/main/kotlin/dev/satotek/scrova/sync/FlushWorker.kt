package dev.satotek.scrova.sync

import android.content.Context
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import dev.satotek.scrova.account.signIn
import dev.satotek.scrova.scrovaApp
import kotlinx.coroutines.CancellationException
import uniffi.scrova_ffi.ScrovaException

class FlushWorker(context: Context, parameters: WorkerParameters) : CoroutineWorker(context, parameters) {
    override suspend fun doWork(): Result {
        return try {
            // WorkManager may start a fresh process without ever opening the UI.
            applicationContext.signIn.restore()
            applicationContext.scrovaApp.scrova.flush()
            Result.success()
        } catch (_: ScrovaException.SessionInvalid) {
            applicationContext.signIn.signOut()
            Result.success()
        } catch (_: ScrovaException.Network) {
            Result.retry()
        } catch (error: CancellationException) {
            throw error
        } catch (_: Exception) {
            // Rust owns per-entry retry state; errors must not expose credentials.
            Result.success()
        }
    }
}
