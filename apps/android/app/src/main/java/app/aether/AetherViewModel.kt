package app.aether

import android.app.Application
import android.os.Build
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import uniffi.aether_ffi.AetherClient
import uniffi.aether_ffi.ClientException
import uniffi.aether_ffi.Pairing
import uniffi.aether_ffi.Work

sealed interface Screen {
    data object Server : Screen
    data object SignIn : Screen
    data object Catalog : Screen
    data class Player(val work: Work) : Screen
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

    fun open(work: Work) {
        _screen.value = Screen.Player(work)
    }

    fun closePlayer() {
        _screen.value = Screen.Catalog
    }

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

    private companion object {
        const val REFRESH_MARGIN_SECONDS = 7L * 24 * 3600
        const val POLL_MILLIS = 3_000L
        const val PAGE = 200u
    }
}
