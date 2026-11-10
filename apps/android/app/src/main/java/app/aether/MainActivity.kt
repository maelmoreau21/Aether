package app.aether

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.ui.PlayerView
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import uniffi.aether_ffi.ClientException
import uniffi.aether_ffi.Pairing
import uniffi.aether_ffi.Work

class MainActivity : ComponentActivity() {
    private val model: AetherViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme(colorScheme = darkColorScheme()) { App(model) }
        }
    }
}

@Composable
private fun App(model: AetherViewModel) {
    val screen by model.screen.collectAsState()
    val error by model.error.collectAsState()
    val snackbar = remember { SnackbarHostState() }
    LaunchedEffect(error) {
        error?.let {
            snackbar.showSnackbar(it)
            model.clearError()
        }
    }
    Scaffold(snackbarHost = { SnackbarHost(snackbar) }) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            when (val current = screen) {
                Screen.Server -> ServerScreen(model)
                Screen.SignIn -> SignInScreen(model)
                Screen.Catalog -> CatalogScreen(model)
                is Screen.Player -> PlayerScreen(model, current.work)
            }
        }
    }
}

@Composable
private fun ServerScreen(model: AetherViewModel) {
    var address by remember { mutableStateOf("") }
    Column(Modifier.fillMaxSize().padding(24.dp), verticalArrangement = Arrangement.Center) {
        Text(stringResource(R.string.server_title), style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.height(16.dp))
        OutlinedTextField(
            value = address,
            onValueChange = { address = it },
            label = { Text(stringResource(R.string.server_hint)) },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(16.dp))
        Button(onClick = { model.connect(address) }, enabled = address.isNotBlank()) {
            Text(stringResource(R.string.continue_action))
        }
    }
}

@Composable
private fun SignInScreen(model: AetherViewModel) {
    var login by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    val pairing by model.pairing.collectAsState()
    Column(Modifier.fillMaxSize().padding(24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(stringResource(R.string.sign_in_title), style = MaterialTheme.typography.headlineSmall)
        OutlinedTextField(login, { login = it }, label = { Text(stringResource(R.string.login)) }, singleLine = true, modifier = Modifier.fillMaxWidth())
        OutlinedTextField(
            password,
            { password = it },
            label = { Text(stringResource(R.string.password)) },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
            modifier = Modifier.fillMaxWidth(),
        )
        Button(onClick = { model.login(login.trim(), password) }, enabled = login.isNotBlank() && password.isNotEmpty()) {
            Text(stringResource(R.string.sign_in))
        }
        pairing?.let { PairingCode(it) }
        TextButton(onClick = model::changeServer) { Text(stringResource(R.string.change_server)) }
    }
}

/** The QR modules come from the Core; drawn here without an image library. */
@Composable
private fun PairingCode(pairing: Pairing) {
    Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.fillMaxWidth()) {
        Text(stringResource(R.string.pair_title))
        Spacer(Modifier.height(8.dp))
        val width = pairing.qrWidth.toInt()
        Canvas(Modifier.size(220.dp).background(Color.White).padding(12.dp)) {
            val cell = size.minDimension / width
            pairing.qrModules.forEachIndexed { index, dark ->
                if (dark) {
                    drawRect(Color.Black, Offset((index % width) * cell, (index / width) * cell), Size(cell, cell))
                }
            }
        }
        Spacer(Modifier.height(8.dp))
        Text(stringResource(R.string.pair_code, pairing.code), style = MaterialTheme.typography.titleLarge)
    }
}

@Composable
private fun CatalogScreen(model: AetherViewModel) {
    val works by model.works.collectAsState()
    var search by remember { mutableStateOf("") }
    var kind by remember { mutableStateOf("film") }
    LaunchedEffect(search, kind) {
        delay(300) // debounce typing
        model.loadCatalog(kind, search.ifBlank { null })
    }
    Column(Modifier.fillMaxSize().padding(12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            FilterChip(kind == "film", { kind = "film" }, { Text(stringResource(R.string.films)) })
            FilterChip(kind == "series", { kind = "series" }, { Text(stringResource(R.string.series)) })
            Spacer(Modifier.weight(1f))
            TextButton(onClick = model::signOut) { Text(stringResource(R.string.sign_out)) }
        }
        OutlinedTextField(search, { search = it }, label = { Text(stringResource(R.string.search)) }, singleLine = true, modifier = Modifier.fillMaxWidth())
        Spacer(Modifier.height(8.dp))
        if (works.isEmpty()) {
            Text(stringResource(R.string.empty), modifier = Modifier.padding(16.dp))
        }
        LazyVerticalGrid(columns = GridCells.Adaptive(160.dp), verticalArrangement = Arrangement.spacedBy(8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            items(works, key = { it.id }) { work -> WorkCard(work) { model.open(work) } }
        }
    }
}

@Composable
private fun WorkCard(work: Work, onClick: () -> Unit) {
    Card(Modifier.fillMaxWidth().clickable(onClick = onClick)) {
        Column(Modifier.padding(12.dp)) {
            Text(work.title, style = MaterialTheme.typography.titleMedium, maxLines = 2)
            work.year?.let { Text(it.toString(), style = MaterialTheme.typography.bodySmall) }
        }
    }
}

/**
 * Hardware-decoded HLS from the Core. Resumes at the saved position, saves it
 * every 15 s and when leaving; the Core applies its own "watched" threshold.
 */
@Composable
private fun PlayerScreen(model: AetherViewModel, work: Work) {
    val context = LocalContext.current
    val player = remember { ExoPlayer.Builder(context).build() }
    val unavailable = stringResource(R.string.error_unavailable, "%s")
    var progressId by remember { mutableStateOf<String?>(null) }
    var revision by remember { mutableStateOf(0L) }
    var errorText by remember { mutableStateOf<String?>(null) }

    fun save() {
        val id = progressId ?: return
        val duration = player.duration
        if (duration <= 0) return
        model.saveProgress(id, player.currentPosition / 1000.0, duration / 1000.0, revision) { revision = it }
    }

    LaunchedEffect(work.id) {
        val client = model.client ?: return@LaunchedEffect
        try {
            val media = client.media(work.id)
            val id = media.workId ?: work.id
            val progress = client.progress(id)
            val url = client.playbackUrl(media.id)
            progressId = id
            revision = progress.revision
            val start = if (progress.watched) 0L else (progress.position * 1000).toLong()
            player.setMediaItem(MediaItem.Builder().setUri(url.url).setMimeType(MimeTypes.APPLICATION_M3U8).build(), start)
            player.prepare()
            player.playWhenReady = true
            while (isActive) {
                delay(15_000)
                if (player.isPlaying) save()
            }
        } catch (e: ClientException) {
            errorText = unavailable.format(e.message)
        }
    }
    DisposableEffect(Unit) {
        onDispose {
            save()
            player.release()
        }
    }
    BackHandler { model.closePlayer() }
    Column(Modifier.fillMaxSize().background(Color.Black)) {
        errorText?.let { Text(it, color = Color.White, modifier = Modifier.padding(16.dp)) }
        AndroidView(factory = { PlayerView(it).apply { this.player = player } }, modifier = Modifier.fillMaxSize())
    }
}
