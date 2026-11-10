@file:androidx.annotation.OptIn(markerClass = [androidx.media3.common.util.UnstableApi::class])

package app.aether

import android.app.Application
import android.net.Uri
import android.os.Build
import androidx.media3.common.MimeTypes
import androidx.media3.exoplayer.offline.Download
import androidx.media3.exoplayer.offline.DownloadManager
import androidx.media3.exoplayer.offline.DownloadRequest
import androidx.media3.exoplayer.offline.DownloadService
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.aether_ffi.AetherClient
import uniffi.aether_ffi.ClientException
import uniffi.aether_ffi.Pairing
import uniffi.aether_ffi.Work
import uniffi.aether_ffi.WorkDetail

/** One offline copy, as shown in the downloads screen. */
data class OfflineItem(val id: String, val title: String, val state: Int, val percent: Float) {
    val complete get() = state == Download.STATE_COMPLETED
}

sealed interface Screen {
    data object Server : Screen
    data object SignIn : Screen
    data object Catalog : Screen
    data object Downloads : Screen
    data class Series(val work: Work) : Screen
    /** `back` is where leaving the player returns. */
    data class Player(val work: Work, val back: Screen) : Screen
}

/** Navigation and session state; every network call goes through the Rust core. */
class AetherViewModel(app: Application) : AndroidViewModel(app) {
    private val store = TokenStore(app)
    private val device = "${Build.MANUFACTURER} ${Build.MODEL}"

    var client: AetherClient? = null
        private set

    private val _screen = MutableStateFlow<Screen>(Screen.Server)
    val screen: StateFlow<Screen> = _screen
    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error
    private val _pairing = MutableStateFlow<Pairing?>(null)
    val pairing: StateFlow<Pairing?> = _pairing
    private val _works = MutableStateFlow<List<Work>>(emptyList())
    val works: StateFlow<List<Work>> = _works
    private val _detail = MutableStateFlow<WorkDetail?>(null)
    val detail: StateFlow<WorkDetail?> = _detail

    private var pairingJob: Job? = null

    init {
        store.server?.let { connect(it) }
    }

    fun clearError() {
        _error.value = null
    }

    /** Opens a Core; a saved token is refreshed if due, otherwise sign-in is shown. */
    fun connect(server: String) {
        val address = server.trim().let { if ("://" in it) it else "https://$it" }
        val saved = if (address == store.server) store.load() else null
        val created = runCatching { AetherClient(address, device, saved) }.getOrElse {
            _error.value = it.message
            _screen.value = Screen.Server
            return
        }
        client = created
        store.server = address
        if (saved == null) {
            showSignIn()
            return
        }
        viewModelScope.launch {
            try {
                created.refreshIfDue(REFRESH_MARGIN_SECONDS)
                store.save(created.token())
                showCatalog()
            } catch (e: ClientException.Unauthorized) {
                store.save(null)
                showSignIn()
            } catch (e: ClientException) {
                // Offline or Core restarting: keep the session, show what loads.
                _error.value = e.message
                showCatalog()
            }
        }
    }

    fun changeServer() {
        pairingJob?.cancel()
        client = null
        _screen.value = Screen.Server
    }

    fun login(login: String, password: String) {
        val client = client ?: return
        viewModelScope.launch {
            try {
                client.login(login, password)
                signedIn()
            } catch (e: ClientException.Unauthorized) {
                _error.value = getApplication<Application>().getString(R.string.error_credentials)
            } catch (e: ClientException) {
                _error.value = e.message
            }
        }
    }

    private fun showSignIn() {
        _screen.value = Screen.SignIn
        startPairing()
    }

    /** Shows a QR code and polls until a signed-in device approves it. */
    private fun startPairing() {
        val client = client ?: return
        pairingJob?.cancel()
        pairingJob = viewModelScope.launch {
            while (isActive && _screen.value == Screen.SignIn) {
                val pairing = try {
                    client.startPairing()
                } catch (e: ClientException) {
                    _error.value = e.message
                    delay(10_000)
                    continue
                }
                _pairing.value = pairing
                val deadline = System.currentTimeMillis() + pairing.expiresIn * 1000
                try {
                    while (isActive && System.currentTimeMillis() < deadline) {
                        delay(POLL_MILLIS)
                        if (client.pollPairing(pairing) != null) {
                            signedIn()
                            return@launch
                        }
                    }
                } catch (e: ClientException.PairingExpired) {
                    // A fresh code is requested on the next turn.
                } catch (e: ClientException) {
                    _error.value = e.message
                }
            }
        }
    }

    private fun signedIn() {
        pairingJob?.cancel()
        _pairing.value = null
        store.save(client?.token())
        showCatalog()
    }

    private fun showCatalog() {
        _screen.value = Screen.Catalog
        loadCatalog(kind = "film", search = null)
        renewIncompleteDownloads()
    }

    fun loadCatalog(kind: String?, search: String?) {
        val client = client ?: return
        viewModelScope.launch {
            try {
                _works.value = client.catalog(kind, search, 0u, PAGE).items
            } catch (e: ClientException.Unauthorized) {
                store.save(null)
                showSignIn()
            } catch (e: ClientException) {
                _error.value = e.message
            }
        }
    }

    /** A series opens on its seasons; anything else plays. */
    fun open(work: Work) {
        if (work.kind != "series") {
            _screen.value = Screen.Player(work, _screen.value)
            return
        }
        _detail.value = null
        _screen.value = Screen.Series(work)
        val client = client ?: return
        viewModelScope.launch {
            try {
                _detail.value = client.work(work.id)
            } catch (e: ClientException) {
                _error.value = e.message
            }
        }
    }

    fun closeSeries() {
        _screen.value = Screen.Catalog
    }

    fun closePlayer() {
        val current = _screen.value
        _screen.value = if (current is Screen.Player) current.back else Screen.Catalog
    }

    /** Absolute poster URL on this server, or `null` (no poster, foreign path). */
    fun posterUrl(work: Work): String? =
        work.posterUrl?.let { path -> runCatching { client?.url(path) }.getOrNull() }

    /** Host the token may be sent to: never to another site. */
    fun serverHost(): String? = store.server?.let { android.net.Uri.parse(it).host }

    fun signOut() {
        val client = client ?: return
        viewModelScope.launch {
            runCatching { client.logout() }
            store.save(null)
            showSignIn()
        }
    }

    /** Saves the position without blocking the screen that is closing. */
    fun saveProgress(workId: String, positionSeconds: Double, durationSeconds: Double, revision: Long, done: (Long) -> Unit = {}) {
        val client = client ?: return
        if (durationSeconds <= 0) return
        viewModelScope.launch {
            runCatching { client.saveProgress(workId, positionSeconds, durationSeconds, false, revision) }
                .onSuccess { done(it.revision) }
        }
    }

    // ---- Offline copies ---------------------------------------------------

    private val downloads = Downloads.manager(app)
    private val _offline = MutableStateFlow<List<OfflineItem>>(emptyList())
    val offline: StateFlow<List<OfflineItem>> = _offline
    /** Grants renewed after a failure this session: one retry each, no loop. */
    private val renewed = mutableSetOf<String>()

    init {
        downloads.addListener(object : DownloadManager.Listener {
            override fun onDownloadChanged(manager: DownloadManager, download: Download, finalException: Exception?) {
                publishDownloads()
                // Most failures are an expired 12 h grant: resume with a fresh URL.
                if (download.state == Download.STATE_FAILED && renewed.add(download.request.id)) {
                    renew(download)
                }
            }

            override fun onDownloadRemoved(manager: DownloadManager, download: Download) = publishDownloads()
        })
        // Progress is not reported by callbacks: sample it while something downloads.
        viewModelScope.launch {
            while (isActive) {
                if (_offline.value.isEmpty() || downloads.currentDownloads.isNotEmpty()) publishDownloads()
                delay(1_000)
            }
        }
    }

    fun showDownloads() {
        _screen.value = Screen.Downloads
        publishDownloads()
    }

    private fun publishDownloads() {
        // The manager is used on the main thread, its index is read off it.
        val live = downloads.currentDownloads.associateBy { it.request.id }
        viewModelScope.launch {
            val stored = withContext(Dispatchers.IO) {
                buildList {
                    downloads.downloadIndex.getDownloads().use { cursor ->
                        while (cursor.moveToNext()) add(cursor.download)
                    }
                }
            }
            _offline.value = stored.map { live[it.request.id] ?: it }.map {
                OfflineItem(it.request.id, title(it), it.state, it.percentDownloaded.coerceAtLeast(0f))
            }
        }
    }

    private fun title(download: Download) = String(download.request.data).substringAfter('\n')
    private fun assetId(download: Download) = String(download.request.data).substringBefore('\n')

    private fun request(id: String, assetId: String, title: String, url: String) {
        val request = DownloadRequest.Builder(id, Uri.parse(url))
            .setMimeType(MimeTypes.APPLICATION_M3U8)
            .setData("$assetId\n$title".toByteArray())
            .build()
        // Same id: Media3 merges the request, so a renewal keeps every segment.
        DownloadService.sendAddDownload(getApplication(), AetherDownloadService::class.java, request, false)
    }

    /** Copies the film or episode for offline viewing (original quality, every audio track). */
    fun download(work: Work) {
        val client = client ?: return
        viewModelScope.launch {
            try {
                val media = client.media(work.id)
                request(work.id, media.id, work.title, client.playbackUrl(media.id).url)
            } catch (e: ClientException) {
                _error.value = e.message
            }
        }
    }

    private fun renew(download: Download) {
        val client = client ?: return
        viewModelScope.launch {
            runCatching { client.playbackUrl(assetId(download)) }
                .onSuccess { request(download.request.id, assetId(download), title(download), it.url) }
        }
    }

    /** At start-up, unfinished downloads resume with fresh grants. */
    private fun renewIncompleteDownloads() {
        viewModelScope.launch {
            val pending = withContext(Dispatchers.IO) {
                buildList {
                    downloads.downloadIndex.getDownloads().use { cursor ->
                        while (cursor.moveToNext()) {
                            if (cursor.download.state != Download.STATE_COMPLETED) add(cursor.download)
                        }
                    }
                }
            }
            pending.forEach(::renew)
        }
    }

    fun deleteDownload(id: String) {
        DownloadService.sendRemoveDownload(getApplication(), AetherDownloadService::class.java, id, false)
    }

    /** The finished offline copy of this work, if any (reads the index: call off the main thread). */
    fun completedDownload(id: String): Download? =
        downloads.downloadIndex.getDownload(id)?.takeIf { it.state == Download.STATE_COMPLETED }

    private companion object {
        const val REFRESH_MARGIN_SECONDS = 7L * 24 * 3600
        const val POLL_MILLIS = 3_000L
        const val PAGE = 200u
    }
}
