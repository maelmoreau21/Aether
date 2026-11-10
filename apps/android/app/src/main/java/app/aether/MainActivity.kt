@file:androidx.annotation.OptIn(markerClass = [androidx.media3.common.util.UnstableApi::class])

package app.aether

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
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
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items as listItems
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
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
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.ui.PlayerView
import coil.ImageLoader
import coil.compose.AsyncImage
import kotlinx.coroutines.delay
import okhttp3.OkHttpClient
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.isActive
import kotlinx.coroutines.withContext
import uniffi.aether_ffi.ClientException
import uniffi.aether_ffi.Pairing
import uniffi.aether_ffi.Episode
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
                Screen.Downloads -> DownloadsScreen(model)
                is Screen.Series -> SeriesScreen(model, current.work)
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

/** Starts a download, asking once for the notification that shows its progress. */
@Composable
private fun rememberDownload(model: AetherViewModel): (Work) -> Unit {
    val context = LocalContext.current
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { }
    return { work ->
        if (Build.VERSION.SDK_INT >= 33 &&
            context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            ask.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
        model.download(work)
    }
}

/** Posters need the device token; it is only ever sent to the Aether server. */
@Composable
private fun rememberPosterLoader(model: AetherViewModel): ImageLoader {
    val context = LocalContext.current
    return remember(model) {
        val http = OkHttpClient.Builder()
            .addInterceptor { chain ->
                val request = chain.request()
                val token = model.client?.token()?.access
                val ours = token != null && request.url.host == model.serverHost()
                chain.proceed(if (ours) request.newBuilder().header("Authorization", "Bearer $token").build() else request)
            }
            .build()
        ImageLoader.Builder(context).okHttpClient(http).crossfade(true).build()
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
    val posters = rememberPosterLoader(model)
    val download = rememberDownload(model)
    Column(Modifier.fillMaxSize().padding(12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            FilterChip(kind == "film", { kind = "film" }, { Text(stringResource(R.string.films)) })
            FilterChip(kind == "series", { kind = "series" }, { Text(stringResource(R.string.series)) })
            Spacer(Modifier.weight(1f))
            TextButton(onClick = model::showDownloads) { Text(stringResource(R.string.downloads)) }
            TextButton(onClick = model::signOut) { Text(stringResource(R.string.sign_out)) }
        }
        OutlinedTextField(search, { search = it }, label = { Text(stringResource(R.string.search)) }, singleLine = true, modifier = Modifier.fillMaxWidth())
        Spacer(Modifier.height(8.dp))
        if (works.isEmpty()) {
            Text(stringResource(R.string.empty), modifier = Modifier.padding(16.dp))
        }
        LazyVerticalGrid(columns = GridCells.Adaptive(128.dp), verticalArrangement = Arrangement.spacedBy(8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            items(works, key = { it.id }) { work ->
                WorkCard(work, model.posterUrl(work), posters, onDownload = { download(work) }.takeIf { work.kind != "series" }) { model.open(work) }
            }
        }
    }
}

@Composable
private fun WorkCard(work: Work, poster: String?, loader: ImageLoader, onDownload: (() -> Unit)?, onClick: () -> Unit) {
    Card(Modifier.fillMaxWidth().clickable(onClick = onClick)) {
        AsyncImage(
            model = poster,
            imageLoader = loader,
            contentDescription = work.title,
            contentScale = ContentScale.Crop,
            modifier = Modifier.fillMaxWidth().aspectRatio(2f / 3f).background(Color.DarkGray),
        )
        Row(Modifier.padding(start = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f).padding(vertical = 8.dp)) {
                Text(work.title, style = MaterialTheme.typography.titleMedium, maxLines = 2)
                work.year?.let { Text(it.toString(), style = MaterialTheme.typography.bodySmall) }
            }
            onDownload?.let { DownloadButton(it) }
        }
    }
}

@Composable
private fun DownloadButton(onClick: () -> Unit) {
    IconButton(onClick = onClick) {
        Icon(Icons.Filled.KeyboardArrowDown, contentDescription = stringResource(R.string.download))
    }
}

@Composable
private fun DownloadsScreen(model: AetherViewModel) {
    val items by model.offline.collectAsState()
    BackHandler { model.closeSeries() }
    Column(Modifier.fillMaxSize().padding(12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = model::closeSeries) { Text(stringResource(R.string.back)) }
            Text(stringResource(R.string.downloads), style = MaterialTheme.typography.titleLarge)
        }
        if (items.isEmpty()) Text(stringResource(R.string.no_downloads), modifier = Modifier.padding(16.dp))
        LazyColumn(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            listItems(items, key = { it.id }) { item ->
                Card(Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(12.dp)) {
                        Text(item.title, style = MaterialTheme.typography.titleMedium)
                        if (!item.complete) {
                            LinearProgressIndicator(progress = { item.percent / 100f }, modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp))
                        }
                        Row {
                            if (item.complete) {
                                TextButton(onClick = { model.open(Work(item.id, "offline", item.title, null, "", null)) }) {
                                    Text(stringResource(R.string.play))
                                }
                            } else {
                                Text(stringResource(R.string.download_percent, item.percent.toInt()), modifier = Modifier.padding(8.dp))
                            }
                            Spacer(Modifier.weight(1f))
                            TextButton(onClick = { model.deleteDownload(item.id) }) { Text(stringResource(R.string.delete)) }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun SeriesScreen(model: AetherViewModel, series: Work) {
    val detail by model.detail.collectAsState()
    val download = rememberDownload(model)
    var season by remember(series.id) { mutableStateOf(0) }
    BackHandler { model.closeSeries() }
    Column(Modifier.fillMaxSize().padding(12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = model::closeSeries) { Text(stringResource(R.string.back)) }
            Text(series.title, style = MaterialTheme.typography.titleLarge, maxLines = 1)
        }
        val seasons = detail?.seasons.orEmpty()
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            seasons.forEachIndexed { index, item ->
                val label = item.title.ifBlank { stringResource(R.string.season, (item.number ?: (index + 1L)).toInt()) }
                FilterChip(season == index, { season = index }, { Text(label) })
            }
        }
        LazyColumn(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            listItems(seasons.getOrNull(season)?.episodes.orEmpty(), key = { it.id }) { episode ->
                val asWork = Work(episode.id, "episode", episode.title, null, episode.description, null)
                EpisodeRow(episode, onDownload = { download(asWork) }) { model.open(asWork) }
            }
        }
    }
}

@Composable
private fun EpisodeRow(episode: Episode, onDownload: () -> Unit, onClick: () -> Unit) {
    Card(Modifier.fillMaxWidth().clickable(onClick = onClick)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f).padding(12.dp)) {
            Text(episode.number?.let { "$it · ${episode.title}" } ?: episode.title, style = MaterialTheme.typography.titleMedium)
            if (episode.description.isNotBlank()) {
                Text(episode.description, style = MaterialTheme.typography.bodySmall, maxLines = 2)
            }
        }
        DownloadButton(onDownload)
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
    // Offline copy first (cache), network otherwise.
    val player = remember {
        ExoPlayer.Builder(context).setMediaSourceFactory(DefaultMediaSourceFactory(Downloads.playback(context))).build()
    }
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
        val offline = withContext(Dispatchers.IO) { model.completedDownload(work.id) }
        if (offline != null) {
            // No network needed; the position syncs when the Core is reachable.
            val progress = runCatching { model.client?.progress(work.id) }.getOrNull()
            progressId = work.id
            revision = progress?.revision ?: 0
            val start = if (progress == null || progress.watched) 0L else (progress.position * 1000).toLong()
            player.setMediaItem(offline.request.toMediaItem(), start)
            player.prepare()
            player.playWhenReady = true
            while (isActive) {
                delay(15_000)
                if (player.isPlaying) save()
            }
            return@LaunchedEffect
        }
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
