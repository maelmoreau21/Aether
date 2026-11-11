@file:androidx.annotation.OptIn(markerClass = [androidx.media3.common.util.UnstableApi::class])

package app.aether

import android.app.Application
import android.content.Intent
import android.net.Uri
import android.os.Build
import androidx.media3.common.MimeTypes
import androidx.media3.common.StreamKey
import androidx.media3.exoplayer.hls.playlist.HlsMultivariantPlaylist
import androidx.media3.exoplayer.hls.playlist.HlsPlaylistParser
import androidx.media3.exoplayer.offline.Download
import androidx.media3.exoplayer.offline.DownloadManager
import androidx.media3.exoplayer.offline.DownloadRequest
import androidx.media3.exoplayer.offline.DownloadService
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.aether_ffi.AetherClient
import uniffi.aether_ffi.ClientException
import uniffi.aether_ffi.Pairing
import uniffi.aether_ffi.releasesUrl
import uniffi.aether_ffi.ServerInfo
import uniffi.aether_ffi.Work
import uniffi.aether_ffi.WorkDetail

/** One offline copy, as shown in the downloads screen. */
data class OfflineItem(val id: String, val title: String, val state: Int, val percent: Float) {
    val complete get() = state == Download.STATE_COMPLETED
}

/**
 * A video variant of the master playlist an offline copy can keep: the
 * original, or a reduced H.264 encode when the server allows transcoding.
 * `keys` select that variant and every audio rendition.
 */
data class DownloadQuality(val original: Boolean, val height: Int, val bitrate: Int, val keys: List<StreamKey>)

/** A download waiting for its quality to be chosen. */
data class DownloadChoice(val work: Work, val assetId: String, val url: String, val qualities: List<DownloadQuality>)

/** What a pairing request in progress is for: SSO opens the browser, Quick Connect shows a QR code. */
enum class PairingKind { Sso, QuickConnect }

sealed interface Screen {
    data object Server : Screen
    data object SignIn : Screen
    data object Catalog : Screen
    data object Downloads : Screen
    data object Devices : Screen
    data class Series(val work: Work) : Screen
    /** `back` is where leaving the player returns; `startAt` overrides the saved position (seconds). */
    data class Player(val work: Work, val back: Screen, val startAt: Double? = null) : Screen
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
    /** Address of the Core in use, shown on the sign-in page. */
    val serverAddress: String? get() = store.server
    private val _serverInfo = MutableStateFlow<ServerInfo?>(null)
    /** How the server lets people sign in (SSO label, local account), once checked. */
    val serverInfo: StateFlow<ServerInfo?> = _serverInfo
    private val _checking = MutableStateFlow(false)
    val checking: StateFlow<Boolean> = _checking
    private val _pairingKind = MutableStateFlow<PairingKind?>(null)
    val pairingKind: StateFlow<PairingKind?> = _pairingKind
    private val _signingIn = MutableStateFlow(false)
    val signingIn: StateFlow<Boolean> = _signingIn
    /** Pending second step of a sign-in (TOTP or passkey account); null otherwise. */
    class MfaChallenge(val token: String, val totp: Boolean)
    private val _mfa = MutableStateFlow<MfaChallenge?>(null)
    val mfa: StateFlow<MfaChallenge?> = _mfa
    /** Newer version published by the server when this app is older; null otherwise. */
    private val _updateAvailable = MutableStateFlow<String?>(null)
    val updateAvailable: StateFlow<String?> = _updateAvailable
    private val _works = MutableStateFlow<List<Work>>(emptyList())
    val works: StateFlow<List<Work>> = _works
    private val _detail = MutableStateFlow<WorkDetail?>(null)
    val detail: StateFlow<WorkDetail?> = _detail

    private var pairingJob: Job? = null

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
        if (saved == null) {
            checkServer(created, address)
            return
        }
        client = created
        store.server = address
        viewModelScope.launch {
            try {
                created.refreshIfDue(REFRESH_MARGIN_SECONDS)
                store.save(created.token())
                TokenRefresh.schedule(getApplication())
                showCatalog()
                connectRemote()
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

    /** First page: the address must be an Aether server before any sign-in is offered. */
    private fun checkServer(created: AetherClient, address: String) {
        _checking.value = true
        viewModelScope.launch {
            try {
                _serverInfo.value = created.discover()
                client = created
                store.server = address
                _screen.value = Screen.SignIn
            } catch (e: ClientException.NotAether) {
                _error.value = getApplication<Application>().getString(R.string.error_not_aether)
                _screen.value = Screen.Server
            } catch (e: ClientException) {
                _error.value = e.message
                _screen.value = Screen.Server
            } finally {
                _checking.value = false
            }
        }
    }

    fun changeServer() {
        cancelPairing()
        client = null
        _mfa.value = null
        _serverInfo.value = null
        _screen.value = Screen.Server
    }

    fun login(login: String, password: String) {
        val client = client ?: return
        _signingIn.value = true
        viewModelScope.launch {
            try {
                val step = client.loginStep(login, password)
                val token = step.mfaToken
                if (step.user == null && token != null) {
                    _error.value = null
                    _mfa.value = MfaChallenge(token, step.totp)
                } else {
                    signedIn()
                }
            } catch (e: ClientException.Unauthorized) {
                _error.value = getApplication<Application>().getString(R.string.error_credentials)
            } catch (e: ClientException) {
                _error.value = e.message
            } finally {
                _signingIn.value = false
            }
        }
    }

    /** Second step: an authenticator code, or a backup code when [recovery]. */
    fun submitSecondFactor(code: String, recovery: Boolean) {
        val client = client ?: return
        val challenge = _mfa.value ?: return
        _signingIn.value = true
        viewModelScope.launch {
            try {
                client.loginSecondFactor(challenge.token, code.trim(), recovery)
                signedIn()
            } catch (e: ClientException.Unauthorized) {
                _error.value = getApplication<Application>().getString(R.string.error_code)
            } catch (e: ClientException.Server) {
                _error.value = if (e.status.toInt() == 401) {
                    getApplication<Application>().getString(R.string.error_code)
                } else {
                    e.message
                }
            } catch (e: ClientException) {
                _error.value = e.message
            } finally {
                _signingIn.value = false
            }
        }
    }

    /** Back from the code page to the identifiers. */
    fun cancelSecondFactor() {
        _mfa.value = null
        _error.value = null
    }

    private fun showSignIn() {
        _screen.value = Screen.SignIn
        // A saved session that expired: the page still needs the server's sign-in methods.
        if (_serverInfo.value == null) {
            val current = client ?: return
            viewModelScope.launch {
                runCatching { current.discover() }.getOrNull()?.let { _serverInfo.value = it }
            }
        }
    }

    /** SSO: opens the browser on the server's approval page, then polls like Quick Connect. */
    fun startSso() = startPairing(PairingKind.Sso)

    fun startQuickConnect() = startPairing(PairingKind.QuickConnect)

    fun cancelPairing() {
        pairingJob?.cancel()
        _pairing.value = null
        _pairingKind.value = null
    }

    fun reopenBrowser() {
        _pairing.value?.let { openBrowser(it.url) }
    }

    /** Best effort: a failure never shows anything. */
    private fun checkUpdate() {
        val client = client ?: return
        viewModelScope.launch {
            _updateAvailable.value = runCatching { client.checkUpdate() }.getOrNull()
        }
    }

    fun openReleases() = openBrowser(releasesUrl())

    /** Only web addresses are ever handed to the system. */
    private fun openBrowser(url: String) {
        if (!url.startsWith("https://") && !url.startsWith("http://")) return
        val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        runCatching { getApplication<Application>().startActivity(intent) }
    }

    /** Requests a code, then polls until a signed-in device (or the browser, for SSO) approves it. */
    private fun startPairing(kind: PairingKind) {
        val client = client ?: return
        pairingJob?.cancel()
        _pairing.value = null
        _pairingKind.value = kind
        pairingJob = viewModelScope.launch {
            val expired = getApplication<Application>().getString(R.string.code_expired)
            val pairing = try {
                client.startPairing()
            } catch (e: ClientException) {
                _error.value = e.message
                _pairingKind.value = null
                return@launch
            }
            _pairing.value = pairing
            if (kind == PairingKind.Sso) openBrowser(pairing.url)
            val deadline = System.currentTimeMillis() + pairing.expiresIn * 1000
            try {
                while (isActive && System.currentTimeMillis() < deadline) {
                    delay(POLL_MILLIS)
                    if (client.pollPairing(pairing) != null) {
                        signedIn()
                        return@launch
                    }
                }
                _error.value = expired
            } catch (e: ClientException.PairingExpired) {
                _error.value = expired
            } catch (e: ClientException) {
                _error.value = e.message
            }
            _pairing.value = null
            _pairingKind.value = null
        }
    }

    private fun signedIn() {
        _mfa.value = null
        pairingJob?.cancel()
        _pairing.value = null
        _pairingKind.value = null
        store.save(client?.token())
        TokenRefresh.schedule(getApplication())
        showCatalog()
        connectRemote()
    }

    private fun showCatalog() {
        _screen.value = Screen.Catalog
        checkUpdate()
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
            remote.disconnect()
            runCatching { client.logout() }
            store.save(null)
            TokenRefresh.cancel(getApplication())
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

    // ---- Remote control ---------------------------------------------------

    private val remote = RemoteControl(app, viewModelScope)
    val devices: StateFlow<List<RemoteDevice>> = remote.devices
    private val _playerCommands = MutableSharedFlow<RemoteCommand>(extraBufferCapacity = 16)
    /** play, pause, seek and stop received for the player on screen. */
    val playerCommands: SharedFlow<RemoteCommand> = _playerCommands

    init {
        viewModelScope.launch {
            remote.commands.collect { command ->
                when (command.action) {
                    "open" -> command.workId?.let { id ->
                        val back = (_screen.value as? Screen.Player)?.back ?: _screen.value
                        _screen.value = Screen.Player(Work(id, "remote", "", null, "", null), back, command.position)
                    }
                    else -> _playerCommands.tryEmit(command)
                }
            }
        }
    }

    private fun connectRemote() {
        val server = store.server ?: return
        remote.connect(server, device) { client?.token()?.access }
    }

    fun showDevices() {
        _screen.value = Screen.Devices
    }

    fun reportPlaying(playing: Playing?) = remote.report(playing)

    fun remoteCommand(device: RemoteDevice, action: String, position: Double? = null) =
        remote.command(device, action, position)

    /** "Resume here": continue locally where the other device is, and stop it there. */
    fun resumeHere(device: RemoteDevice) {
        val playing = device.playing ?: return
        remote.command(device, "stop")
        _screen.value = Screen.Player(Work(playing.workId, "remote", playing.title ?: "", null, "", null), _screen.value, playing.position)
    }

    /** A fresh HLS URL for a work (casting an offline copy needs the online one). */
    suspend fun streamUrl(workId: String): String? {
        val client = client ?: return null
        return runCatching { client.playbackUrl(client.media(workId).id).url }.getOrNull()
    }

    /** "Send to…": the other device opens the work at this position; this one stops. */
    fun sendTo(device: RemoteDevice, workId: String, position: Double) {
        remote.command(device, "open", position, workId)
        closePlayer()
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

    /** `keys` empty keeps every variant of the master playlist. */
    private fun request(id: String, assetId: String, title: String, url: String, keys: List<StreamKey>) {
        val request = DownloadRequest.Builder(id, Uri.parse(url))
            .setMimeType(MimeTypes.APPLICATION_M3U8)
            .setStreamKeys(keys)
            .setData("$assetId\n$title".toByteArray())
            .build()
        // Same id: Media3 merges the request, so a renewal keeps every segment.
        DownloadService.sendAddDownload(getApplication(), AetherDownloadService::class.java, request, false)
    }

    private val _downloadChoice = MutableStateFlow<DownloadChoice?>(null)
    /** Set while the quality of a new offline copy is being chosen. */
    val downloadChoice: StateFlow<DownloadChoice?> = _downloadChoice

    /**
     * Copies the film or episode for offline viewing, every audio track. When
     * the server offers reduced variants (a 4K original is ~50 GB), the user
     * picks one first.
     */
    fun download(work: Work) {
        val client = client ?: return
        viewModelScope.launch {
            try {
                val media = client.media(work.id)
                val url = client.playbackUrl(media.id).url
                val qualities = withContext(Dispatchers.IO) { runCatching { qualities(url) }.getOrDefault(emptyList()) }
                if (qualities.size > 1) {
                    _downloadChoice.value = DownloadChoice(work, media.id, url, qualities)
                } else {
                    request(work.id, media.id, work.title, url, qualities.firstOrNull()?.keys.orEmpty())
                }
            } catch (e: ClientException) {
                _error.value = e.message
            }
        }
    }

    /** `null` cancels the pending download. */
    fun chooseDownload(quality: DownloadQuality?) {
        val choice = _downloadChoice.value ?: return
        _downloadChoice.value = null
        quality ?: return
        request(choice.work.id, choice.assetId, choice.work.title, choice.url, quality.keys)
    }

    /** The variants of a master playlist, in its order (the original first). */
    private fun qualities(url: String): List<DownloadQuality> {
        val uri = Uri.parse(url)
        val connection = java.net.URL(url).openConnection() as java.net.HttpURLConnection
        connection.connectTimeout = 10_000
        connection.readTimeout = 10_000
        connection.instanceFollowRedirects = false
        val playlist = try {
            connection.inputStream.use { HlsPlaylistParser().parse(uri, it) }
        } finally {
            connection.disconnect()
        }
        if (playlist !is HlsMultivariantPlaylist) return emptyList()
        val audio = playlist.audios.indices.map { StreamKey(HlsMultivariantPlaylist.GROUP_INDEX_AUDIO, it) }
        return playlist.variants.mapIndexed { index, variant ->
            DownloadQuality(
                original = index == 0,
                height = variant.format.height,
                bitrate = variant.format.bitrate,
                keys = listOf(StreamKey(HlsMultivariantPlaylist.GROUP_INDEX_VARIANT, index)) + audio,
            )
        }
    }

    private fun renew(download: Download) {
        val client = client ?: return
        viewModelScope.launch {
            // The same keys: a renewal must not widen the copy to every variant.
            runCatching { client.playbackUrl(assetId(download)) }
                .onSuccess {
                    request(download.request.id, assetId(download), title(download), it.url, download.request.streamKeys)
                }
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

    // Last on purpose: Kotlin initialises properties in source order, and a saved
    // session reaches the remote control and the downloads without suspending
    // (a token that is not due for refresh answers at once).
    init {
        store.server?.let { connect(it) }
    }

    private companion object {
        const val REFRESH_MARGIN_SECONDS = 7L * 24 * 3600
        const val POLL_MILLIS = 3_000L
        const val PAGE = 200u
    }
}
