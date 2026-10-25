package dev.satotek.scrova.sync

import android.content.Context
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import java.util.concurrent.TimeUnit

object FlushScheduler {
    private val network = Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()

    fun flushSoon(context: Context) {
        // KEEP preserves a running flush and its network backoff instead of
        // cancelling it every time the observer emits another event.
        WorkManager.getInstance(context).enqueueUniqueWork(
            "scrova.flush.soon",
            ExistingWorkPolicy.KEEP,
            OneTimeWorkRequestBuilder<FlushWorker>().setConstraints(network).build(),
        )
    }

    fun ensurePeriodic(context: Context) {
        WorkManager.getInstance(context).enqueueUniquePeriodicWork(
            "scrova.flush.periodic",
            ExistingPeriodicWorkPolicy.KEEP,
            PeriodicWorkRequestBuilder<FlushWorker>(15, TimeUnit.MINUTES).setConstraints(network).build(),
        )
    }
}
