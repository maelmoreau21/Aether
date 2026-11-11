@file:androidx.annotation.OptIn(markerClass = [androidx.media3.common.util.UnstableApi::class])

package app.aether

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.fragment.app.FragmentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items as listItems
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.OutlinedTextFieldDefaults
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.sp
import androidx.compose.ui.focus.onFocusChanged
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
import androidx.mediarouter.app.MediaRouteButton
import com.google.android.gms.cast.HlsSegmentFormat
import com.google.android.gms.cast.HlsVideoSegmentFormat
import com.google.android.gms.cast.MediaInfo
import com.google.android.gms.cast.MediaLoadRequestData
import com.google.android.gms.cast.MediaMetadata
import com.google.android.gms.cast.framework.CastButtonFactory
import com.google.android.gms.cast.framework.CastContext
import com.google.android.gms.cast.framework.CastSession
import com.google.android.gms.cast.framework.SessionManagerListener
import coil.ImageLoader
import coil.compose.AsyncImage
import kotlinx.coroutines.delay
import okhttp3.OkHttpClient
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import java.util.concurrent.Executors
import kotlinx.coroutines.withContext
import uniffi.aether_ffi.ClientException
import uniffi.aether_ffi.Pairing
import uniffi.aether_ffi.Episode
import uniffi.aether_ffi.Work

/** The web's Halo dark palette (apps/web/src/lib/styles/halo.css). */
private val HaloColors = darkColorScheme(
    primary = Color(0xFFEEF1F8),
    onPrimary = Color(0xFF0B0E18),
    secondary = Color(0xFFFF7B47),
    background = Color(0xFF0B0E18),
    onBackground = Color(0xFFEEF1F8),
    surface = Color(0xFF11151F),
    onSurface = Color(0xFFEEF1F8),
    surfaceVariant = Color(0xFF171C28),
    onSurfaceVariant = Color(0xFFA2A9BA),
    error = Color(0xFFFF6B6B),
    outline = Color(0x17FFFFFF),
)

/** A FragmentActivity: the Cast route picker is shown as a dialog fragment. */
class MainActivity : FragmentActivity() {
    private val model: AetherViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme(colorScheme = HaloColors) { App(model) }
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
                Screen.SignIn -> if (model.mfa.collectAsState().value != null) SecondFactorScreen(model) else SignInScreen(model)
                Screen.Catalog -> CatalogScreen(model)
                Screen.Downloads -> DownloadsScreen(model)
                Screen.Devices -> DevicesScreen(model)
                is Screen.Series -> SeriesScreen(model, current.work)
                is Screen.Player -> PlayerScreen(model, current.work, current.startAt)
            }
        }
    }
    val choice by model.downloadChoice.collectAsState()
    choice?.let { DownloadQualityDialog(it, model::chooseDownload) }
}

/** Original or a reduced variant, with the storage each takes per hour of film. */
@Composable
private fun DownloadQualityDialog(choice: DownloadChoice, onChoose: (DownloadQuality?) -> Unit) {
    AlertDialog(
        onDismissRequest = { onChoose(null) },
        title = { Text(stringResource(R.string.download_quality)) },
        text = {
            Column {
                Text(choice.work.title, style = MaterialTheme.typography.bodyMedium)
                choice.qualities.forEach { quality ->
                    val parts = listOfNotNull(
                        if (quality.original) stringResource(R.string.download_original) else null,
                        if (quality.height > 0) stringResource(R.string.download_height, quality.height) else null,
                        if (quality.bitrate > 0) {
                            // BANDWIDTH is a peak: an upper bound of the copy's size.
                            val gigabytes = quality.bitrate.toDouble() * 3600 / 8 / 1e9
                            stringResource(R.string.download_size, String.format("%.1f", gigabytes))
                        } else {
                            null
                        },
                    )
                    TextButton(onClick = { onChoose(quality) }, modifier = Modifier.fillMaxWidth()) {
                        Text(parts.joinToString(" · "), modifier = Modifier.fillMaxWidth())
                    }
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = { onChoose(null) }) { Text(stringResource(R.string.cancel)) } },
    )
}

// ---- Sign-in: the same two pages and the same look as the web (Halo, dark) ------

private val Panel = Color(0x0EFFFFFF)
private val Panel2 = Color(0x17FFFFFF)
private val Line = Color(0x17FFFFFF)
private val Muted = Color(0xFFA2A9BA)
private val Accent = Color(0xFFFF7B47)

/** Brand, a card and a footer, centred and scrollable like the web's `login-viewport`. */
@Composable
private fun AuthFrame(name: String, content: @Composable ColumnScope.() -> Unit) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Image(painterResource(R.drawable.ic_aether_logo), contentDescription = null, modifier = Modifier.size(44.dp))
            Text(name, fontSize = 28.sp, fontWeight = FontWeight.ExtraBold)
        }
        Spacer(Modifier.height(8.dp))
        Text(stringResource(R.string.brand_tagline), color = Muted, fontSize = 12.sp, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(24.dp))
        val shape = RoundedCornerShape(28.dp)
        Column(
            Modifier.widthIn(max = 440.dp).fillMaxWidth().clip(shape)
                .background(MaterialTheme.colorScheme.surface).border(1.dp, Line, shape).padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(20.dp),
            content = content,
        )
        Spacer(Modifier.height(24.dp))
        Text(stringResource(R.string.brand_footer, name), color = Muted, fontSize = 13.sp)
    }
}

@Composable
private fun CardIntro(title: String, description: String) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(title, fontSize = 30.sp, lineHeight = 34.sp, fontWeight = FontWeight.ExtraBold)
        Text(description, color = Muted, fontSize = 15.sp, lineHeight = 24.sp)
    }
}

@Composable
private fun LabeledField(
    label: String,
    value: String,
    onChange: (String) -> Unit,
    keyboardType: KeyboardType = KeyboardType.Text,
    secret: Boolean = false,
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(label, fontSize = 14.sp, fontWeight = FontWeight.Bold)
        OutlinedTextField(
            value = value,
            onValueChange = onChange,
            singleLine = true,
            shape = RoundedCornerShape(12.dp),
            visualTransformation = if (secret) PasswordVisualTransformation() else VisualTransformation.None,
            keyboardOptions = KeyboardOptions(keyboardType = keyboardType),
            colors = OutlinedTextFieldDefaults.colors(
                unfocusedContainerColor = Panel,
                focusedContainerColor = MaterialTheme.colorScheme.surfaceVariant,
                unfocusedBorderColor = Color.Transparent,
                focusedBorderColor = Accent,
            ),
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

/** The large full-width button of the web's `btn-submit` (light when primary). */
@Composable
private fun WideButton(text: String, primary: Boolean, enabled: Boolean = true, onClick: () -> Unit) {
    Button(
        onClick = onClick,
        enabled = enabled,
        shape = RoundedCornerShape(16.dp),
        colors = if (primary) {
            ButtonDefaults.buttonColors()
        } else {
            ButtonDefaults.buttonColors(containerColor = Panel2, contentColor = MaterialTheme.colorScheme.onSurface)
        },
        modifier = Modifier.fillMaxWidth().height(48.dp),
    ) {
        Text(text, fontWeight = FontWeight.Bold)
        if (primary) {
            Spacer(Modifier.size(8.dp))
            Text("→", fontWeight = FontWeight.Bold)
        }
    }
}

@Composable
private fun Notice(text: String) {
    Text(
        text,
        color = Accent,
        fontSize = 14.sp,
        fontWeight = FontWeight.SemiBold,
        modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp))
            .background(Color(0x29FF7B47)).padding(horizontal = 16.dp, vertical = 12.dp),
    )
}

/** Page 1: the address of the server, checked before anything else is offered. */
@Composable
private fun ServerScreen(model: AetherViewModel) {
    var address by remember { mutableStateOf("") }
    val checking by model.checking.collectAsState()
    AuthFrame("Aether") {
        CardIntro(stringResource(R.string.server_title), stringResource(R.string.server_intro))
        LabeledField(stringResource(R.string.server_hint), address, { address = it }, KeyboardType.Uri)
        WideButton(
            stringResource(if (checking) R.string.server_checking else R.string.continue_action),
            primary = true,
            enabled = address.isNotBlank() && !checking,
        ) { model.connect(address) }
    }
}

/** Page 3: authenticator or backup code, when the account has a second factor. */
@Composable
private fun SecondFactorScreen(model: AetherViewModel) {
    var code by remember { mutableStateOf("") }
    var recovery by remember { mutableStateOf(false) }
    val challenge by model.mfa.collectAsState()
    val signingIn by model.signingIn.collectAsState()
    val totp = challenge?.totp == true
    AuthFrame("Aether") {
        CardIntro(
            stringResource(R.string.mfa_title),
            stringResource(
                when {
                    !totp -> R.string.mfa_passkey_only
                    recovery -> R.string.mfa_recovery_desc
                    else -> R.string.mfa_totp_desc
                },
            ),
        )
        if (totp) {
            LabeledField(
                stringResource(if (recovery) R.string.mfa_recovery_label else R.string.mfa_code_label),
                code,
                { code = it },
                if (recovery) KeyboardType.Text else KeyboardType.NumberPassword,
            )
            WideButton(
                stringResource(if (signingIn) R.string.mfa_checking else R.string.mfa_submit),
                primary = true,
                enabled = code.isNotBlank() && !signingIn,
            ) { model.submitSecondFactor(code, recovery) }
            TextButton(onClick = { recovery = !recovery; code = "" }) {
                Text(stringResource(if (recovery) R.string.mfa_use_app else R.string.mfa_use_recovery))
            }
        }
        TextButton(onClick = model::cancelSecondFactor) { Text(stringResource(R.string.back)) }
    }
}

/** Page 2, in the order of the web login page: SSO button, local account, Quick Connect. */
@Composable
private fun SignInScreen(model: AetherViewModel) {
    var login by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    val info by model.serverInfo.collectAsState()
    val pairing by model.pairing.collectAsState()
    val kind by model.pairingKind.collectAsState()
    val signingIn by model.signingIn.collectAsState()
    val ssoEnabled = info?.ssoEnabled == true
    val localEnabled = info?.localLoginEnabled != false
    AuthFrame(info?.name ?: "Aether") {
        CardIntro(stringResource(R.string.welcome_title), stringResource(R.string.welcome_desc))
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween, modifier = Modifier.fillMaxWidth()) {
            Text(model.serverAddress.orEmpty(), color = Muted, fontSize = 13.sp, modifier = Modifier.weight(1f))
            TextButton(onClick = model::changeServer) { Text(stringResource(R.string.change_server)) }
        }
        if (info?.needsBootstrap == true) {
            Notice(stringResource(R.string.bootstrap_needed))
            return@AuthFrame
        }

        if (ssoEnabled) {
            if (kind == PairingKind.Sso) {
                SsoWait(pairing, model)
            } else {
                SsoButton(info?.ssoLabel.orEmpty(), info?.ssoProvider.orEmpty(), model::startSso)
            }
            if (localEnabled) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    HorizontalDivider(Modifier.weight(1f), color = Line)
                    Text(stringResource(R.string.or_local), color = Muted, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                    HorizontalDivider(Modifier.weight(1f), color = Line)
                }
            }
        }

        if (localEnabled) {
            LabeledField(stringResource(R.string.login), login, { login = it })
            LabeledField(stringResource(R.string.password), password, { password = it }, KeyboardType.Password, secret = true)
            WideButton(
                stringResource(if (signingIn) R.string.signing_in else R.string.sign_in),
                primary = true,
                enabled = login.isNotBlank() && password.isNotEmpty() && !signingIn,
            ) { model.login(login.trim(), password) }
        }

        QuickConnect(pairing, kind, model)

        HorizontalDivider(color = Line)
        Text(
            stringResource(
                when {
                    !localEnabled -> R.string.help_local_disabled
                    ssoEnabled -> R.string.help_sso
                    else -> R.string.help_local
                },
            ),
            color = Muted,
            fontSize = 13.sp,
            lineHeight = 21.sp,
        )
    }
}

/** The SSO button, labelled by the administrator of the server (`sso_label`). */
@Composable
private fun SsoButton(label: String, provider: String, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 48.dp).clip(RoundedCornerShape(16.dp)).background(Panel2)
            .clickable(onClick = onClick).padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(label, fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f))
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            if (provider.isNotEmpty()) {
                Text(
                    provider,
                    color = Muted,
                    fontSize = 12.sp,
                    fontWeight = FontWeight.Bold,
                    modifier = Modifier.clip(RoundedCornerShape(8.dp)).background(Panel2).padding(horizontal = 9.dp, vertical = 4.dp),
                )
            }
            Text("→", fontWeight = FontWeight.Bold)
        }
    }
}

/** Shown in place of the SSO button once the browser has been opened. */
@Composable
private fun SsoWait(pairing: Pairing?, model: AetherViewModel) {
    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(Panel).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(stringResource(if (pairing == null) R.string.sso_opening else R.string.sso_wait), color = Muted, fontSize = 14.sp, lineHeight = 22.sp)
        if (pairing != null) {
            Text(codeText(pairing.code), fontSize = 26.sp, fontWeight = FontWeight.Bold)
            Text(stringResource(R.string.waiting_confirmation), color = Muted, fontSize = 13.sp)
            WideButton(stringResource(R.string.sso_reopen), primary = false) { model.reopenBrowser() }
        }
        TextButton(onClick = model::cancelPairing) { Text(stringResource(R.string.cancel)) }
    }
}

/** Quick Connect, as the web's section: title, text, QR code and code. */
@Composable
private fun QuickConnect(pairing: Pairing?, kind: PairingKind?, model: AetherViewModel) {
    val showing = kind == PairingKind.QuickConnect
    Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp), modifier = Modifier.fillMaxWidth()) {
        HorizontalDivider(color = Line)
        Text(stringResource(R.string.qc_title), fontSize = 16.sp, fontWeight = FontWeight.Bold)
        Text(stringResource(R.string.qc_desc), color = Muted, fontSize = 13.sp, lineHeight = 21.sp, textAlign = TextAlign.Center)
        if (showing && pairing != null) {
            PairingCode(pairing)
            Text(stringResource(R.string.qc_hint), color = Muted, fontSize = 13.sp, lineHeight = 21.sp, textAlign = TextAlign.Center)
            Text(stringResource(R.string.waiting_confirmation), color = Muted, fontSize = 13.sp)
        }
        WideButton(
            stringResource(
                when {
                    showing && pairing == null -> R.string.qc_creating
                    showing -> R.string.qc_new
                    else -> R.string.qc_show
                },
            ),
            primary = false,
            enabled = !(showing && pairing == null),
        ) { model.startQuickConnect() }
    }
}

/** `ABCD–EFGH`, as the web shows a pairing code. */
private fun codeText(code: String) = if (code.length > 4) "${code.take(4)}–${code.drop(4)}" else code

/** The QR modules come from the Core; drawn here without an image library. */
@Composable
private fun PairingCode(pairing: Pairing) {
    Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.fillMaxWidth()) {
        val width = pairing.qrWidth.toInt()
        Canvas(Modifier.size(220.dp).clip(RoundedCornerShape(12.dp)).background(Color.White).padding(12.dp)) {
            val cell = size.minDimension / width
            pairing.qrModules.forEachIndexed { index, dark ->
                if (dark) {
                    drawRect(Color.Black, Offset((index % width) * cell, (index / width) * cell), Size(cell, cell))
                }
            }
        }
        Spacer(Modifier.height(8.dp))
        Text(codeText(pairing.code), fontSize = 26.sp, fontWeight = FontWeight.Bold)
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
    val update by model.updateAvailable.collectAsState()
    Column(Modifier.fillMaxSize().padding(12.dp)) {
        update?.let { latest ->
            Row(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(Panel2).padding(start = 16.dp, end = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(stringResource(R.string.update_available, latest), fontSize = 14.sp, modifier = Modifier.weight(1f).padding(vertical = 10.dp))
                TextButton(onClick = model::openReleases) { Text(stringResource(R.string.update_download)) }
            }
            Spacer(Modifier.height(8.dp))
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            FilterChip(kind == "film", { kind = "film" }, { Text(stringResource(R.string.films)) })
            FilterChip(kind == "series", { kind = "series" }, { Text(stringResource(R.string.series)) })
            Spacer(Modifier.weight(1f))
            TextButton(onClick = model::showDevices) { Text(stringResource(R.string.devices)) }
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

/** A visible ring on the item the TV remote (D-pad) has selected. */
@Composable
private fun Modifier.focusRing(): Modifier {
    var focused by remember { mutableStateOf(false) }
    return onFocusChanged { focused = it.isFocused }
        .border(3.dp, if (focused) Color.White else Color.Transparent, RoundedCornerShape(12.dp))
}

@Composable
private fun WorkCard(work: Work, poster: String?, loader: ImageLoader, onDownload: (() -> Unit)?, onClick: () -> Unit) {
    Card(Modifier.fillMaxWidth().focusRing().clickable(onClick = onClick)) {
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

private fun clock(seconds: Double): String {
    val total = seconds.toLong()
    return if (total >= 3600) "%d:%02d:%02d".format(total / 3600, total / 60 % 60, total % 60)
    else "%d:%02d".format(total / 60, total % 60)
}

/** The account's other devices: drive what they play, or take it over here. */
@Composable
private fun DevicesScreen(model: AetherViewModel) {
    val devices by model.devices.collectAsState()
    BackHandler { model.closeSeries() }
    Column(Modifier.fillMaxSize().padding(12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = model::closeSeries) { Text(stringResource(R.string.back)) }
            Text(stringResource(R.string.devices), style = MaterialTheme.typography.titleLarge)
        }
        if (devices.isEmpty()) Text(stringResource(R.string.no_devices), modifier = Modifier.padding(16.dp))
        LazyColumn(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            listItems(devices, key = { it.id }) { device ->
                Card(Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(12.dp)) {
                        Text(device.name, style = MaterialTheme.typography.titleMedium)
                        val playing = device.playing
                        if (playing == null) {
                            Text(stringResource(R.string.idle), style = MaterialTheme.typography.bodySmall)
                            return@Column
                        }
                        Text("${playing.title ?: ""} · ${clock(playing.position)}", style = MaterialTheme.typography.bodySmall)
                        Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                            TextButton(onClick = { model.remoteCommand(device, if (playing.paused) "play" else "pause") }) {
                                Text(stringResource(if (playing.paused) R.string.play else R.string.pause))
                            }
                            TextButton(onClick = { model.remoteCommand(device, "seek", playing.position - 10) }) { Text("−10 s") }
                            TextButton(onClick = { model.remoteCommand(device, "seek", playing.position + 30) }) { Text("+30 s") }
                            TextButton(onClick = { model.remoteCommand(device, "stop") }) { Text(stringResource(R.string.stop)) }
                        }
                        TextButton(onClick = { model.resumeHere(device) }) { Text(stringResource(R.string.resume_here)) }
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
    Card(Modifier.fillMaxWidth().focusRing().clickable(onClick = onClick)) {
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
private fun PlayerScreen(model: AetherViewModel, work: Work, startAt: Double?) {
    val context = LocalContext.current
    // Offline copy first (cache), network otherwise.
    val player = remember {
        ExoPlayer.Builder(context).setMediaSourceFactory(DefaultMediaSourceFactory(Downloads.playback(context))).build()
    }
    val unavailable = stringResource(R.string.error_unavailable, "%s")
    var progressId by remember { mutableStateOf<String?>(null) }
    var revision by remember { mutableStateOf(0L) }
    var errorText by remember { mutableStateOf<String?>(null) }
    var streamUrl by remember { mutableStateOf<String?>(null) }
    var castingTo by remember { mutableStateOf<String?>(null) }
    val scope = androidx.compose.runtime.rememberCoroutineScope()

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
            player.setMediaItem(offline.request.toMediaItem(), startAt?.let { (it * 1000).toLong() } ?: start)
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
            streamUrl = url.url
            progressId = id
            revision = progress.revision
            val saved = if (progress.watched) 0L else (progress.position * 1000).toLong()
            val start = startAt?.let { (it * 1000).toLong() } ?: saved
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
    // Chromecast: the receiver opens the same HLS capability URL at this position.
    var cast by remember { mutableStateOf<CastContext?>(null) }
    LaunchedEffect(Unit) {
        runCatching {
            CastContext.getSharedInstance(context, Executors.newSingleThreadExecutor()).addOnSuccessListener { cast = it }
        }
    }
    fun castHere(session: CastSession) {
        val client = session.remoteMediaClient ?: return
        val position = player.currentPosition
        scope.launch {
            val url = streamUrl ?: model.streamUrl(work.id) ?: return@launch
            val metadata = MediaMetadata(MediaMetadata.MEDIA_TYPE_MOVIE).apply {
                putString(MediaMetadata.KEY_TITLE, work.title)
            }
            val info = MediaInfo.Builder(url)
                .setStreamType(MediaInfo.STREAM_TYPE_BUFFERED)
                .setContentType("application/x-mpegURL")
                // Without these the Default Media Receiver expects MPEG-TS segments.
                .setHlsSegmentFormat(HlsSegmentFormat.FMP4)
                .setHlsVideoSegmentFormat(HlsVideoSegmentFormat.FMP4)
                .setMetadata(metadata)
                .build()
            client.load(MediaLoadRequestData.Builder().setMediaInfo(info).setAutoplay(true).setCurrentTime(position).build())
            player.pause()
            castingTo = session.castDevice?.friendlyName ?: ""
        }
    }
    DisposableEffect(cast) {
        val context = cast ?: return@DisposableEffect onDispose { }
        val listener = object : SessionManagerListener<CastSession> {
            override fun onSessionStarted(session: CastSession, sessionId: String) = castHere(session)
            override fun onSessionResumed(session: CastSession, wasSuspended: Boolean) = castHere(session)
            override fun onSessionEnded(session: CastSession, error: Int) { castingTo = null }
            override fun onSessionStarting(session: CastSession) {}
            override fun onSessionStartFailed(session: CastSession, error: Int) {}
            override fun onSessionEnding(session: CastSession) {}
            override fun onSessionResuming(session: CastSession, sessionId: String) {}
            override fun onSessionResumeFailed(session: CastSession, error: Int) {}
            override fun onSessionSuspended(session: CastSession, reason: Int) {}
        }
        context.sessionManager.addSessionManagerListener(listener, CastSession::class.java)
        onDispose { context.sessionManager.removeSessionManagerListener(listener, CastSession::class.java) }
    }

    // Remote control: commands from another device, and what plays here.
    LaunchedEffect(Unit) {
        model.playerCommands.collect { command ->
            when (command.action) {
                "play" -> player.play()
                "pause" -> player.pause()
                "seek" -> command.position?.let { player.seekTo((it * 1000).toLong()) }
                "stop" -> model.closePlayer()
            }
        }
    }
    LaunchedEffect(work.id) {
        while (isActive) {
            val id = progressId
            if (id != null && player.duration > 0) {
                model.reportPlaying(Playing(id, work.title.ifBlank { null }, player.currentPosition / 1000.0, player.duration / 1000.0, !player.isPlaying))
            }
            delay(3_000)
        }
    }
    DisposableEffect(Unit) {
        onDispose {
            save()
            model.reportPlaying(null)
            player.release()
        }
    }
    BackHandler { model.closePlayer() }
    val others by model.devices.collectAsState()
    Column(Modifier.fillMaxSize().background(Color.Black)) {
        errorText?.let { Text(it, color = Color.White, modifier = Modifier.padding(16.dp)) }
        Row(Modifier.padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            if (cast != null) {
                AndroidView(factory = { MediaRouteButton(it).also { button -> CastButtonFactory.setUpMediaRouteButton(it.applicationContext, button) } })
            }
            castingTo?.let { Text(stringResource(R.string.casting_to, it), color = Color.White) }
        }
        // "Send to…": hand this playback over to another device at this position.
        if (others.isNotEmpty() && progressId != null) {
            Row(Modifier.padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.send_to), color = Color.White)
                others.forEach { device ->
                    TextButton(onClick = { model.sendTo(device, progressId!!, player.currentPosition / 1000.0) }) { Text(device.name) }
                }
            }
        }
        AndroidView(factory = { PlayerView(it).apply {
            this.player = player
            // The Core's HLS carries WebVTT renditions; forced ones are picked on their own.
            setShowSubtitleButton(true)
        } }, modifier = Modifier.fillMaxSize())
    }
}
