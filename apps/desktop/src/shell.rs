//! The local page shown before the server's interface (which server to open),
//! and the few messages a page may send to the native side.
//!
//! Every page loaded in the window can post messages, the server's interface
//! and any site it links to included: only harmless requests are accepted from
//! them, and the ones that choose the server are accepted from the local page
//! alone.
use serde::{Deserialize, Serialize};

/// Custom protocol serving the local page.
pub const SCHEME: &str = "aether";
pub const PAGE: &str = include_str!("../assets/shell.html");

/// Runs in every page: window full screen follows the web player's, F11 toggles
/// it, and Ctrl+Shift+S returns to the choice of the server. It also makes the
/// interface behave like an application rather than a page of a browser: no
/// browser menu, no dragging of images and links, no selection of the
/// interface's own labels, single sign-on in the computer's browser
/// (`aetherDesktop.sso`, used by the sign-in page), and a session that stays
/// open while the application is used.
pub const PAGE_SCRIPT: &str = r#"(() => {
  if (window.top !== window || window.__aetherDesktop) return;
  window.__aetherDesktop = true;
  const post = message => window.ipc && window.ipc.postMessage(JSON.stringify(message));
  window.aetherDesktop = Object.freeze({ sso: () => post({ type: 'sso' }) });
  document.addEventListener('fullscreenchange', () => post({ type: 'fullscreen', on: Boolean(document.fullscreenElement) }));
  const editable = target => target instanceof Element && Boolean(target.closest('input, textarea, [contenteditable=""], [contenteditable="true"]'));
  window.addEventListener('keydown', event => {
    if (event.key === 'F11') { event.preventDefault(); post({ type: 'toggle-fullscreen' }); }
    else if (event.ctrlKey && event.shiftKey && event.code === 'KeyS') { event.preventDefault(); post({ type: 'change-server' }); }
    else if (event.key === 'F5') { event.preventDefault(); location.reload(); }
  }, true);
  window.addEventListener('contextmenu', event => { if (!editable(event.target)) event.preventDefault(); }, true);
  window.addEventListener('dragstart', event => { if (!editable(event.target)) event.preventDefault(); }, true);
  window.addEventListener('wheel', event => { if (event.ctrlKey) event.preventDefault(); }, { capture: true, passive: false });
  try {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync('html{-webkit-user-select:none;user-select:none}input,textarea,[contenteditable],p,pre,code,.selectable{-webkit-user-select:text;user-select:text}img,a{-webkit-user-drag:none}');
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
  } catch { /* an older engine keeps the page's own behaviour */ }
  // Once a day while the application is used: 30 more days for the session.
  const extend = async () => {
    if (!location.pathname.startsWith('/app')) return;
    try {
      const key = 'aether-desktop-session-extended';
      if (Date.now() - Number(localStorage.getItem(key) || 0) < 86400000) return;
      const me = await fetch('/api/v1/auth/me', { credentials: 'same-origin', cache: 'no-store' });
      if (!me.ok) return;
      const { csrfToken } = await me.json();
      const done = await fetch('/api/v1/auth/session/extend', { method: 'POST', credentials: 'same-origin', headers: { 'x-csrf-token': csrfToken } });
      if (done.ok) localStorage.setItem(key, String(Date.now()));
    } catch { /* offline: next launch */ }
  };
  window.addEventListener('load', extend, { once: true });
})();"#;

/// Windows only: the window has no title bar of its own, the interface draws
/// the minimize / maximize / close buttons in its top right corner and the top
/// of the page (the site header) moves the window.
pub const FRAME_SCRIPT: &str = r#"(() => {
  if (window.top !== window || window.__aetherFrame) return;
  window.__aetherFrame = true;
  const post = message => window.ipc && window.ipc.postMessage(JSON.stringify(message));
  const interactive = target => target instanceof Element && Boolean(target.closest('a, button, input, select, textarea, label, summary, [role=button], [role=menuitem], [role=slider], [contenteditable=""], [contenteditable="true"], video, #aether-frame'));
  const draggable = event => event.button === 0 && !interactive(event.target)
    && (event.clientY < 32 || (event.target instanceof Element && Boolean(event.target.closest('.site-header, [data-drag-region]'))));
  window.addEventListener('mousedown', event => { if (draggable(event) && event.detail === 1) post({ type: 'window', action: 'drag' }); }, true);
  window.addEventListener('dblclick', event => { if (draggable(event)) post({ type: 'window', action: 'toggle-maximize' }); }, true);
  const icons = {
    minimize: '<path d="M1 5.5h8" />',
    'toggle-maximize': '<rect x="1.5" y="1.5" width="7" height="7" />',
    close: '<path d="M1 1l8 8M9 1L1 9" />',
  };
  const install = () => {
    if (document.getElementById('aether-frame')) return;
    // A constructed style sheet: the Core's content security policy refuses <style> elements.
    const sheet = new CSSStyleSheet();
    sheet.replaceSync('#aether-frame{position:fixed;top:0;right:0;z-index:2147483647;display:flex;height:32px;-webkit-user-select:none;user-select:none}'
      + '#aether-frame button{all:unset;box-sizing:border-box;width:46px;height:32px;display:grid;place-items:center;color:#eef1f8;opacity:.8;cursor:default}'
      + '#aether-frame button:hover{background:rgba(255,255,255,.12);opacity:1}'
      + '#aether-frame button.close:hover{background:#c42b1c}'
      + '#aether-frame svg{width:10px;height:10px;fill:none;stroke:currentColor;stroke-width:1}'
      + '#aether-frame[hidden],html[data-native-fullscreen] #aether-frame{display:none}'
      + '.site-header .header-inner{padding-right:calc(var(--gutter,24px) + 140px)!important}');
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
    const bar = document.createElement('div');
    bar.id = 'aether-frame';
    for (const [action, icon] of Object.entries(icons)) {
      const button = document.createElement('button');
      button.type = 'button';
      button.tabIndex = -1;
      button.setAttribute('aria-label', action === 'close' ? 'Fermer' : action === 'minimize' ? 'Réduire' : 'Agrandir ou restaurer');
      if (action === 'close') button.className = 'close';
      button.innerHTML = '<svg viewBox="0 0 10 10" aria-hidden="true">' + icon + '</svg>';
      button.addEventListener('click', () => post({ type: 'window', action }));
      bar.appendChild(button);
    }
    document.body.appendChild(bar);
    // Full screen (the player's or F11) has no window buttons.
    document.addEventListener('fullscreenchange', () => { bar.hidden = Boolean(document.fullscreenElement); });
  };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', install, { once: true });
  else install();
})();"#;

/// Address of the local page. WebView2 maps a custom protocol to
/// `http://<scheme>.localhost`; the other engines keep `<scheme>://localhost`.
pub fn url(change: bool) -> String {
    let base = if cfg!(windows) {
        "http://aether.localhost/"
    } else {
        "aether://localhost/"
    };
    if change {
        format!("{base}#change")
    } else {
        base.to_owned()
    }
}

pub fn is_shell_url(url: &str) -> bool {
    ["http://aether.localhost/", "aether://localhost/"]
        .iter()
        .any(|origin| url.starts_with(origin))
}

/// What the native side does for a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// The local page is displayed; `change` when the person asked to pick
    /// another server (no automatic reconnection).
    Ready {
        change: bool,
    },
    Connect(String),
    ChangeServer,
    Fullscreen(bool),
    ToggleFullscreen,
    /// Single sign-on in the computer's browser, asked by the sign-in page.
    Sso,
    /// The server cannot be reached: open what was downloaded from it.
    Offline,
    /// A button or the top of the page, in the frameless Windows window.
    Window(WindowAction),
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WindowAction {
    Drag,
    Minimize,
    ToggleMaximize,
    Close,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum Message {
    Ready {
        #[serde(default)]
        change: bool,
    },
    Connect {
        server: String,
    },
    ChangeServer,
    Fullscreen {
        on: bool,
    },
    ToggleFullscreen,
    Sso,
    Offline,
    Window {
        action: WindowAction,
    },
}

/// Reads a message posted by the page at `page_url`; `None` for anything
/// malformed or not allowed from that page.
pub fn request(body: &str, page_url: &str) -> Option<Request> {
    let from_shell = is_shell_url(page_url);
    match serde_json::from_str(body).ok()? {
        Message::Ready { change } if from_shell => Some(Request::Ready { change }),
        Message::Connect { server } if from_shell && server.len() <= 2048 => {
            Some(Request::Connect(server))
        }
        Message::Ready { .. } | Message::Connect { .. } => None,
        Message::ChangeServer => Some(Request::ChangeServer),
        Message::Fullscreen { on } => Some(Request::Fullscreen(on)),
        Message::ToggleFullscreen => Some(Request::ToggleFullscreen),
        Message::Sso if !from_shell => Some(Request::Sso),
        Message::Sso => None,
        Message::Window { action } => Some(Request::Window(action)),
        Message::Offline if from_shell => Some(Request::Offline),
        Message::Offline => None,
    }
}

/// What the local page shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct State {
    pub server: Option<String>,
    /// `empty`, `invalid`, `not-aether`, `unreachable` or `timeout`.
    pub error: Option<String>,
    pub checking: bool,
    /// The server opened before cannot be reached: its downloads are offered.
    pub offline: bool,
}

/// The script, run in the sign-in page of the Core, that redeems the one-time
/// code of a single sign-on for the session cookie and opens the interface.
/// `None` unless both values are what the application itself produced.
pub fn sso_script(code: &str, verifier: &str) -> Option<String> {
    if !crate::sso::is_hex64(code) || !crate::sso::is_hex64(verifier) {
        return None;
    }
    Some(format!(
        "fetch('/api/v1/auth/oidc/app/token',{{method:'POST',credentials:'same-origin',cache:'no-store',         headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{code:'{code}',verifier:'{verifier}'}})}})         .then(r=>location.replace(r.ok?'/app/':'/app/login?error=oidc_invalid_response'),         ()=>location.replace('/app/login?error=oidc_provider_unavailable'));"
    ))
}

/// The sign-in page of the Core, with the reason a single sign-on was refused.
pub fn sso_refused_script(reason: &str) -> Option<String> {
    crate::sso::is_error_code(reason)
        .then(|| format!("location.replace('/app/login?error={reason}');"))
}

/// Whether `page` belongs to `server` (the interface of the Core in use).
pub fn is_page_of(page: &str, server: &str) -> bool {
    let server = server.trim_end_matches('/');
    page.len() > server.len()
        && page.is_char_boundary(server.len())
        && page[..server.len()].eq_ignore_ascii_case(server)
        && page[server.len()..].starts_with('/')
}

impl State {
    /// The script handing this state to the page. JSON is valid JavaScript.
    pub fn script(&self) -> String {
        let json = serde_json::to_string(self).unwrap_or_else(|_| "{}".to_owned());
        format!("window.aetherShell && window.aetherShell.state({json});")
    }
}

/// Only safe web addresses are followed in the window or handed to the browser.
/// Forbids command injection characters, whitespace, control characters, and malformed URLs.
pub fn is_web_url(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    if rest.is_empty()
        || rest.starts_with('/')
        || rest.starts_with(':')
        || rest.starts_with('?')
        || rest.starts_with('#')
    {
        return false;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() {
        return false;
    }
    !url.chars().any(|c| {
        c.is_control()
            || c.is_whitespace()
            || matches!(c, '"' | '\'' | '<' | '>' | '\\' | '`' | '|' | '^')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_local_page_chooses_the_server() {
        let shell = url(false);
        assert!(is_shell_url(&shell));
        assert!(is_shell_url(&url(true)));
        assert!(!is_shell_url("https://media.example/app/"));
        assert!(!is_shell_url("https://aether.localhost.evil.example/"));
        assert_eq!(
            request(r#"{"type":"connect","server":"media.example"}"#, &shell),
            Some(Request::Connect("media.example".into()))
        );
        assert_eq!(
            request(r#"{"type":"ready","change":true}"#, &shell),
            Some(Request::Ready { change: true })
        );
        assert_eq!(
            request(r#"{"type":"ready"}"#, &shell),
            Some(Request::Ready { change: false })
        );
        let remote = "https://media.example/app/";
        assert_eq!(
            request(
                r#"{"type":"connect","server":"https://evil.example"}"#,
                remote
            ),
            None
        );
        assert_eq!(request(r#"{"type":"ready"}"#, remote), None);
    }

    #[test]
    fn any_page_may_ask_for_harmless_things_only() {
        let remote = "https://media.example/app/";
        assert_eq!(
            request(r#"{"type":"fullscreen","on":true}"#, remote),
            Some(Request::Fullscreen(true))
        );
        assert_eq!(
            request(r#"{"type":"toggle-fullscreen"}"#, remote),
            Some(Request::ToggleFullscreen)
        );
        assert_eq!(
            request(r#"{"type":"change-server"}"#, remote),
            Some(Request::ChangeServer)
        );
        assert_eq!(request(r#"{"type":"sso"}"#, remote), Some(Request::Sso));
        assert_eq!(request(r#"{"type":"sso"}"#, &url(false)), None);
        assert_eq!(request(r#"{"type":"offline"}"#, remote), None);
        assert_eq!(
            request(r#"{"type":"offline"}"#, &url(false)),
            Some(Request::Offline)
        );
        assert_eq!(
            request(r#"{"type":"window","action":"minimize"}"#, remote),
            Some(Request::Window(WindowAction::Minimize))
        );
        assert_eq!(
            request(r#"{"type":"window","action":"format"}"#, remote),
            None
        );
        assert_eq!(request(r#"{"type":"run","command":"calc"}"#, remote), None);
        assert_eq!(request("not json", remote), None);
    }

    #[test]
    fn single_sign_on_only_runs_what_the_application_produced() {
        let (code, verifier) = ("0a".repeat(32), "b1".repeat(32));
        let script = sso_script(&code, &verifier).unwrap();
        assert!(script.contains(&format!("code:'{code}',verifier:'{verifier}'")));
        assert_eq!(sso_script("');alert(1);//", &verifier), None);
        assert_eq!(sso_script(&code, "short"), None);
        assert_eq!(
            sso_refused_script("oidc_group_denied").as_deref(),
            Some("location.replace('/app/login?error=oidc_group_denied');")
        );
        assert_eq!(sso_refused_script("');alert(1);//"), None);
        assert!(is_page_of(
            "https://media.example/app/login",
            "https://media.example"
        ));
        assert!(is_page_of(
            "https://media.example/app/",
            "https://Media.Example/"
        ));
        assert!(!is_page_of(
            "https://media.example.evil.test/app/",
            "https://media.example"
        ));
        assert!(!is_page_of(
            "http://aether.localhost/",
            "https://media.example"
        ));
    }

    #[test]
    fn the_state_is_passed_as_data() {
        let state = State {
            server: Some("https://a.example\"</script>".into()),
            error: Some("invalid".into()),
            checking: false,
            offline: false,
        };
        let script = state.script();
        assert!(script.contains(r#"\"</script>"#));
        assert!(script.starts_with("window.aetherShell && window.aetherShell.state({"));
        assert!(is_web_url("https://a.example"));
        assert!(is_web_url("http://localhost:8080/app"));
        assert!(!is_web_url("https://"));
        assert!(!is_web_url("http://"));
        assert!(!is_web_url("https:///path"));
        assert!(!is_web_url("https://a.example\" calc"));
        assert!(!is_web_url("https://a.example\ncalc"));
        assert!(!is_web_url("https://a.example\\foo"));
        assert!(!is_web_url("https://a.example|calc"));
        assert!(!is_web_url("https://a.example^calc"));
        assert!(!is_web_url("file:///etc/passwd"));
        assert!(!is_web_url("javascript:alert(1)"));
    }
}
