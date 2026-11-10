@file:androidx.annotation.OptIn(markerClass = [androidx.media3.common.util.UnstableApi::class])

package app.aether

import android.content.Context
import androidx.media3.database.StandaloneDatabaseProvider
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.datasource.cache.CacheDataSource
import androidx.media3.datasource.cache.CacheKeyFactory
import androidx.media3.datasource.cache.NoOpCacheEvictor
import androidx.media3.datasource.cache.SimpleCache
import androidx.media3.exoplayer.offline.DefaultDownloadIndex
import androidx.media3.exoplayer.offline.DefaultDownloaderFactory
import androidx.media3.exoplayer.offline.DownloadManager
import java.io.File
import java.util.concurrent.Executors

/**
 * Offline copies: Media3 downloads the Core's HLS (packet-copied, so the copy
 * is the original quality) into one cache that the player reads first.
 *
 * HLS URLs carry a 12 h playback grant in their path. Cache keys drop it, so a
 * download resumed with a fresh URL keeps every segment it already has, and a
 * downloaded title plays with no network at all.
 */
object Downloads {
    private val grant = Regex("/hls/[0-9a-fA-F]{64}/")

    /** Same key for the same segment, whatever grant the URL carries. */
    val cacheKeys = CacheKeyFactory { spec -> spec.key ?: spec.uri.toString().replace(grant, "/hls/") }

    @Volatile private var manager: DownloadManager? = null
    @Volatile private var cache: SimpleCache? = null
    @Volatile private var database: StandaloneDatabaseProvider? = null

    private fun database(context: Context) =
        database ?: synchronized(this) { database ?: StandaloneDatabaseProvider(context.applicationContext).also { database = it } }

    private fun cache(context: Context): SimpleCache = cache ?: synchronized(this) {
        cache ?: run {
            val app = context.applicationContext
            // App-specific storage: no permission, removed with the app.
            val root = File(app.getExternalFilesDir(null) ?: app.filesDir, "downloads")
            SimpleCache(root, NoOpCacheEvictor(), database(app)).also { cache = it }
        }
    }

    private fun network() = DefaultHttpDataSource.Factory().setUserAgent("Aether-Android").setAllowCrossProtocolRedirects(false)

    fun manager(context: Context): DownloadManager = manager ?: synchronized(this) {
        manager ?: run {
            val app = context.applicationContext
            val writer = CacheDataSource.Factory()
                .setCache(cache(app))
                .setCacheKeyFactory(cacheKeys)
                .setUpstreamDataSourceFactory(network())
            DownloadManager(
                app,
                DefaultDownloadIndex(database(app)),
                DefaultDownloaderFactory(writer, Executors.newFixedThreadPool(2)),
            ).apply { maxParallelDownloads = 2 }.also { manager = it }
        }
    }

    /**
     * Player data source: offline copy first, network otherwise. Streaming never
     * writes to the cache, which holds downloads only and would otherwise grow forever.
     */
    fun playback(context: Context): CacheDataSource.Factory = CacheDataSource.Factory()
        .setCache(cache(context))
        .setCacheKeyFactory(cacheKeys)
        .setUpstreamDataSourceFactory(network())
        .setCacheWriteDataSinkFactory(null)
}
