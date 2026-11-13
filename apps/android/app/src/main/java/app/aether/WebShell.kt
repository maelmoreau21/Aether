package app.aether

import android.annotation.SuppressLint
import android.app.Activity
import android.content.Intent
import android.content.pm.ActivityInfo
import android.net.Uri
import android.view.View
import android.view.ViewGroup
import android.webkit.CookieManager
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import org.json.JSONObject
import uniffi.aether_ffi.Work

/** Session cookie of the Core (see `crates/core/src/auth/state.rs`). */
private const val SESSION_COOKIE = "aether_session"
private const val SESSION_MAX_AGE = 30L * 24 * 3600

/**
 * Runs before the server's own scripts, in its pages only (origin rule): offline downloads
 * and their list stay native (`window.aetherApp`), single sign-on opens the phone's browser
 * (`window.aetherDesktop.sso`, the same hook as the desktop application).
 */
private const val BRIDGE_SCRIPT = """(() => {
  if (window.top !== window || window.aetherApp || !window.AetherAndroid) return;
  const post = message => window.AetherAndroid.postMessage(JSON.stringify(message));
  window.aetherApp = Object.freeze({
    download: (id, kind, title) => post({ type: 'download', id: String(id), kind: String(kind), title: String(title) }),
    downloads: () => post({ type: 'downloads' }),
  });
  window.aetherDesktop = Object.freeze({ sso: () => post({ type: 'sso' }) });
})();"""

/**
 * The server's web interface, signed in with this device's session: the phone shows exactly
 * what the browser shows. Sign-in, downloads and their offline list stay native.
 */
@SuppressLint("SetJavaScriptEnabled")
@Composable
fun WebScreen(model: AetherViewModel) {
    val address = model.webAddress() ?: return
    val access by model.access.collectAsState()
    val context = LocalContext.current
    val activity = context as? Activity
    val download = rememberDownload(model)
    val origin = remember(address) { Uri.parse(address).let { "${it.scheme}://${it.authority}" } }
    var failed by remember { mutableStateOf(false) }
    var canGoBack by remember { mutableStateOf(false) }
    var fullscreen by remember { mutableStateOf<Pair<View, WebChromeClient.CustomViewCallback>?>(null) }

    fun sameOrigin(url: Uri?): Boolean = url != null && "${url.scheme}://${url.authority}" == origin

    /** The interface signed in, switched account or signed out: the device follows it. */
    fun followSession(url: String?) {
        val page = url?.let(Uri::parse)
        if (!sameOrigin(page)) return
        val token = sessionCookie(origin)
        if (token != null) model.adoptWebSession(token)
        else if (page?.path.orEmpty().startsWith("/app/login")) model.webSignedOut()
    }

    val webView = remember(address) {
        WebView(context).apply {
            setBackgroundColor(0xFF0B0E18.toInt())
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            settings.mediaPlaybackRequiresUserGesture = false
            settings.allowFileAccess = false
            settings.allowContentAccess = false
            CookieManager.getInstance().setAcceptCookie(true)
            if (WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER) &&
                WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)
            ) {
                WebViewCompat.addWebMessageListener(this, "AetherAndroid", setOf(origin)) { _, message, source, isMainFrame, _ ->
                    if (!isMainFrame || source.toString().trimEnd('/') != origin) return@addWebMessageListener
                    val data = runCatching { JSONObject(message.data ?: "") }.getOrNull() ?: return@addWebMessageListener
                    when (data.optString("type")) {
                        "sso" -> model.startSso(context)
                        "downloads" -> model.showDownloads()
                        "download" -> {
                            val id = data.optString("id").takeIf { it.isNotBlank() && it.length <= 256 } ?: return@addWebMessageListener
                            val kind = data.optString("kind").takeIf { it == "film" || it == "episode" } ?: return@addWebMessageListener
                            download(Work(id, kind, data.optString("title").take(300), null, "", null))
                        }
                    }
                }
                WebViewCompat.addDocumentStartJavaScript(this, BRIDGE_SCRIPT, setOf(origin))
            }
            webViewClient = object : WebViewClient() {
                override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                    if (sameOrigin(request.url)) return false
                    // Links to other sites (trailers, providers, help) open in the browser.
                    runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, request.url)) }
                    return true
                }

                override fun onPageFinished(view: WebView, url: String?) {
                    canGoBack = view.canGoBack()
                    followSession(url)
                }

                override fun doUpdateVisitedHistory(view: WebView, url: String?, isReload: Boolean) {
                    canGoBack = view.canGoBack()
                    if (sameOrigin(url?.let(Uri::parse))) model.webPage = url
                    followSession(url)
                }

                override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
                    if (request.isForMainFrame) failed = true
                }
            }
            webChromeClient = object : WebChromeClient() {
                // The web player's full screen: the video over everything, in landscape.
                override fun onShowCustomView(view: View, callback: CustomViewCallback) {
                    fullscreen = view to callback
                }

                override fun onHideCustomView() {
                    fullscreen = null
                }
            }
        }
    }

    // Opens (or reopens, after a sign-in or a change of account) with the device's session.
    LaunchedEffect(webView, access) {
        val token = access ?: return@LaunchedEffect
        if (sessionCookie(origin) == token && webView.url != null) return@LaunchedEffect
        setSessionCookie(origin, token)
        failed = false
        // Back from the downloads: the page that was open, not the home page.
        webView.loadUrl(model.webPage?.takeIf { webView.url == null } ?: address)
    }

    // Back from the background: the daily token renewal may have replaced the session.
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    DisposableEffect(lifecycle, webView) {
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_RESUME -> {
                    webView.onResume()
                    model.currentAccess()?.let { token ->
                        if (sessionCookie(origin) != token) {
                            setSessionCookie(origin, token)
                            webView.reload()
                        }
                    }
                }
                Lifecycle.Event.ON_PAUSE -> webView.onPause()
                else -> Unit
            }
        }
        lifecycle.addObserver(observer)
        onDispose {
            lifecycle.removeObserver(observer)
            webView.destroy()
        }
    }

    // Full screen video: system bars hidden, landscape; back leaves full screen first.
    DisposableEffect(fullscreen) {
        val window = activity?.window
        val shown = fullscreen
        val decor = window?.decorView as? ViewGroup
        if (shown != null && decor != null) {
            decor.addView(shown.first, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
            WindowCompat.getInsetsController(window, decor).apply {
                hide(WindowInsetsCompat.Type.systemBars())
                systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            }
            activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        }
        onDispose {
            if (shown != null && decor != null) {
                decor.removeView(shown.first)
                WindowCompat.getInsetsController(window, decor).show(WindowInsetsCompat.Type.systemBars())
                activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED
            }
        }
    }

    BackHandler(enabled = fullscreen != null || canGoBack) {
        val shown = fullscreen
        if (shown != null) {
            shown.second.onCustomViewHidden()
            fullscreen = null
        } else {
            webView.goBack()
        }
    }

    Box(Modifier.fillMaxSize().background(Color(0xFF0B0E18))) {
        AndroidView(factory = { webView }, modifier = Modifier.fillMaxSize())
        if (failed) Offline(onRetry = { failed = false; webView.reload() }, onDownloads = model::showDownloads)
    }
}

/** No answer from the server (offline, Core restarting): what was downloaded still plays. */
@Composable
private fun Offline(onRetry: () -> Unit, onDownloads: () -> Unit) {
    Column(
        Modifier.fillMaxSize().background(Color(0xFF0B0E18)).padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(stringResource(R.string.web_offline), textAlign = TextAlign.Center)
        WideButton(stringResource(R.string.web_retry), primary = true, onClick = onRetry)
        WideButton(stringResource(R.string.downloads), primary = false, onClick = onDownloads)
    }
}

private fun sessionCookie(origin: String): String? =
    CookieManager.getInstance().getCookie(origin)
        ?.split(';')
        ?.map { it.trim() }
        ?.firstOrNull { it.startsWith("$SESSION_COOKIE=") }
        ?.substringAfter('=')
        ?.takeIf { it.isNotBlank() }

private fun setSessionCookie(origin: String, token: String) {
    val secure = if (origin.startsWith("https://")) "; Secure" else ""
    CookieManager.getInstance().apply {
        setCookie(origin, "$SESSION_COOKIE=$token; Path=/; HttpOnly; SameSite=Strict; Max-Age=$SESSION_MAX_AGE$secure")
        flush()
    }
}
