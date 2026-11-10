package app.aether

import android.app.UiModeManager
import android.content.Context
import android.content.res.Configuration
import java.util.UUID
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import org.json.JSONObject

data class Playing(val workId: String, val title: String?, val position: Double, val duration: Double?, val paused: Boolean)
data class RemoteDevice(val id: String, val name: String, val kind: String, val playing: Playing?)
data class RemoteCommand(val fromName: String, val action: String, val position: Double?, val workId: String?)

/**
 * This device on the account's remote-control channel (`/api/v1/remote/ws`):
 * it lists the other devices and what they play, reports its own playback, and
 * receives commands (play, pause, seek, stop, open). Reconnects on its own.
 */
class RemoteControl(context: Context, private val scope: CoroutineScope) {
    private val app = context.applicationContext
    private val http = OkHttpClient.Builder().pingInterval(30, TimeUnit.SECONDS).build()
    private val deviceId: String = app.getSharedPreferences("aether", Context.MODE_PRIVATE).let { prefs ->
        prefs.getString("device_id", null) ?: UUID.randomUUID().toString().also {
            prefs.edit().putString("device_id", it).apply()
        }
    }
    private val kind = if ((app.getSystemService(Context.UI_MODE_SERVICE) as UiModeManager).currentModeType ==
        Configuration.UI_MODE_TYPE_TELEVISION
    ) "tv" else "phone"

    private val _devices = MutableStateFlow<List<RemoteDevice>>(emptyList())
    val devices: StateFlow<List<RemoteDevice>> = _devices
    private val _commands = MutableSharedFlow<RemoteCommand>(extraBufferCapacity = 16)
    val commands: SharedFlow<RemoteCommand> = _commands

    private var socket: WebSocket? = null
    private var target: Pair<String, () -> String?>? = null
    private var name = "Aether"
    private var state: JSONObject = JSONObject().put("type", "state").put("playing", JSONObject.NULL)
    private var retry: Job? = null
    private var backoff = 2_000L

    /** `token` is read at every (re)connection, so a refreshed token is picked up. */
    fun connect(server: String, device: String, token: () -> String?) {
        target = server to token
        name = device
        open()
    }

    fun disconnect() {
        target = null
        retry?.cancel()
        socket?.close(1000, null)
        socket = null
        _devices.value = emptyList()
    }

    private fun open() {
        val (server, token) = target ?: return
        val access = token() ?: return
        socket?.cancel()
        val url = server.trimEnd('/').replaceFirst("http", "ws") + "/api/v1/remote/ws"
        val request = Request.Builder().url(url).header("Authorization", "Bearer $access").build()
        socket = http.newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                backoff = 2_000L
                webSocket.send(JSONObject().put("type", "hello").put("deviceId", deviceId).put("name", name).put("kind", kind).toString())
                webSocket.send(state.toString())
            }

            override fun onMessage(webSocket: WebSocket, text: String) {
                val message = runCatching { JSONObject(text) }.getOrNull() ?: return
                when (message.optString("type")) {
                    "devices" -> _devices.value = parseDevices(message)
                    "command" -> _commands.tryEmit(
                        RemoteCommand(
                            message.optString("fromName"),
                            message.optString("action"),
                            message.optDouble("position").takeUnless { it.isNaN() },
                            message.optString("workId").takeIf { it.isNotEmpty() && it != "null" },
                        ),
                    )
                }
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) = reconnect(webSocket)
            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) = reconnect(webSocket)
        })
    }

    private fun reconnect(closed: WebSocket) {
        if (closed != socket || target == null) return
        retry?.cancel()
        retry = scope.launch {
            delay(backoff)
            backoff = (backoff * 2).coerceAtMost(30_000L)
            open()
        }
    }

    private fun parseDevices(message: JSONObject): List<RemoteDevice> {
        val list = message.optJSONArray("devices") ?: return emptyList()
        return (0 until list.length()).mapNotNull { i ->
            val d = list.optJSONObject(i) ?: return@mapNotNull null
            val p = d.optJSONObject("playing")
            RemoteDevice(
                d.optString("deviceId"),
                d.optString("name"),
                d.optString("kind"),
                p?.let {
                    Playing(
                        it.optString("workId"),
                        it.optString("title").takeIf { t -> t.isNotEmpty() && t != "null" },
                        it.optDouble("position", 0.0),
                        it.optDouble("duration").takeUnless { v -> v.isNaN() },
                        it.optBoolean("paused"),
                    )
                },
            )
        }
    }

    /** What this device plays now (`null` when nothing). */
    fun report(playing: Playing?) {
        val now = playing?.let {
            JSONObject().put("workId", it.workId).put("title", it.title ?: JSONObject.NULL)
                .put("position", it.position).put("duration", it.duration ?: JSONObject.NULL).put("paused", it.paused)
        } ?: JSONObject.NULL
        state = JSONObject().put("type", "state").put("playing", now)
        socket?.send(state.toString())
    }

    fun command(to: RemoteDevice, action: String, position: Double? = null, workId: String? = null) {
        val message = JSONObject().put("type", "command").put("to", to.id).put("action", action)
        position?.let { message.put("position", it.coerceAtLeast(0.0)) }
        workId?.let { message.put("workId", it) }
        socket?.send(message.toString())
    }
}
