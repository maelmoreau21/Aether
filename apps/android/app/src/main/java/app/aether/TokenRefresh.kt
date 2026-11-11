package app.aether

import android.content.Context
import android.os.Build
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit
import uniffi.aether_ffi.AetherClient
import uniffi.aether_ffi.ClientException

/**
 * Daily, when a network is available: keeps the device token alive. The Core
 * ends a session after 7 idle days and 30 days at most; a TV switched on once
 * a fortnight would otherwise have to pair again.
 */
class TokenRefresh(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val store = TokenStore(applicationContext)
        val server = store.server ?: return Result.success()
        val device = "${Build.MANUFACTURER} ${Build.MODEL}"
        // The accounts set aside stay signed in too; one that was revoked leaves the list.
        store.setParked(store.parked().mapNotNull { account ->
            try {
                val other = AetherClient(server, device, account.token)
                other.me()
                other.refreshIfDue(MARGIN_SECONDS)
                account.copy(token = other.token() ?: account.token)
            } catch (e: ClientException.Unauthorized) {
                null
            } catch (e: ClientException) {
                account
            }
        })
        val token = store.load() ?: return Result.success()
        return try {
            val client = AetherClient(server, device, token)
            // `me` also counts as activity for the idle limit; refresh rotates before the absolute one.
            client.me()
            client.refreshIfDue(MARGIN_SECONDS)
            store.save(client.token())
            Result.success()
        } catch (e: ClientException.Unauthorized) {
            store.save(null) // revoked or expired: the app will ask to sign in
            Result.success()
        } catch (e: ClientException) {
            Result.retry()
        }
    }

    companion object {
        private const val NAME = "token-refresh"
        private const val MARGIN_SECONDS = 7L * 24 * 3600

        fun schedule(context: Context) {
            val request = PeriodicWorkRequestBuilder<TokenRefresh>(1, TimeUnit.DAYS)
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .build()
            WorkManager.getInstance(context).enqueueUniquePeriodicWork(NAME, ExistingPeriodicWorkPolicy.KEEP, request)
        }

        fun cancel(context: Context) {
            WorkManager.getInstance(context).cancelUniqueWork(NAME)
        }
    }
}
