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
/// it, and Ctrl+Shift+S returns to the choice of the server.
pub const PAGE_SCRIPT: &str = r#"(() => {
  if (window.top !== window || window.__aetherDesktop) return;
  window.__aetherDesktop = true;
  const post = message => window.ipc && window.ipc.postMessage(JSON.stringify(message));
  document.addEventListener('fullscreenchange', () => post({ type: 'fullscreen', on: Boolean(document.fullscreenElement) }));
  window.addEventListener('keydown', event => {
    if (event.key === 'F11') { event.preventDefault(); post({ type: 'toggle-fullscreen' }); }
    else if (event.ctrlKey && event.shiftKey && event.code === 'KeyS') { event.preventDefault(); post({ type: 'change-server' }); }
  }, true);
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
    }
}

/// What the local page shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct State {
    pub server: Option<String>,
    /// `empty`, `invalid`, `not-aether`, `unreachable` or `timeout`.
    pub error: Option<String>,
    pub checking: bool,
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
        assert_eq!(request(r#"{"type":"run","command":"calc"}"#, remote), None);
        assert_eq!(request("not json", remote), None);
    }

    #[test]
    fn the_state_is_passed_as_data() {
        let state = State {
            server: Some("https://a.example\"</script>".into()),
            error: Some("invalid".into()),
            checking: false,
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
