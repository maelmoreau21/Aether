//! Aether for the desktop (Windows, macOS, Linux): the Core's own web
//! interface in a native window, drawn by the system's web engine (WebView2,
//! WKWebView, WebKitGTK). Nothing of the interface is rewritten here: the
//! application asks which server to open, remembers it and its window, follows
//! the web player into full screen, and signs in through the computer's own
//! browser. Playback, direct or transcoded, is decided by the web player and
//! the Core exactly as in a browser.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod shell;
mod sso;
mod store;

use aether_client::{Client, Error};
use std::{
    borrow::Cow,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use store::Settings;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    window::{Fullscreen, Theme, Window, WindowBuilder},
};
use wry::{
    NewWindowResponse, WebContext, WebView, WebViewBuilder,
    http::{Response, header::CONTENT_TYPE},
};

/// How long the server may take to identify itself before the address is refused.
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
/// Halo background, shown while a page loads instead of a white flash.
const BACKGROUND: (u8, u8, u8, u8) = (11, 14, 24, 255);

enum UserEvent {
    Page(shell::Request),
    /// The address answered (or not) like an Aether server; the error is a
    /// code worded by the local page.
    Checked {
        server: String,
        result: Result<(), String>,
    },
    /// The browser came back from a single sign-on.
    SignedIn(sso::Outcome),
}

fn main() {
    if let Err(error) = run() {
        eprintln!("aether-desktop: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let env = |name: &str| std::env::var(name).ok();
    let config_dir = store::config_dir(env);
    let settings = config_dir
        .as_deref()
        .map(Settings::load)
        .unwrap_or_default();

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = window_builder(settings.window.unwrap_or_default()).build(&event_loop)?;

    // Cookies (the session), cache and site data live with the settings, never
    // next to the executable.
    let mut context = WebContext::new(config_dir.as_ref().map(|dir| dir.join("webview")));
    let ipc = proxy.clone();
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_background_color(BACKGROUND)
        .with_custom_protocol(shell::SCHEME.to_owned(), |_, _| {
            Response::builder()
                .header(CONTENT_TYPE, "text/html; charset=utf-8")
                .body(Cow::Borrowed(shell::PAGE.as_bytes()))
                .unwrap_or_else(|_| Response::new(Cow::Borrowed(&[])))
        })
        .with_initialization_script(shell::PAGE_SCRIPT)
        .with_ipc_handler(move |message| {
            let page = message.uri().to_string();
            if let Some(request) = shell::request(message.body(), &page) {
                let _ = ipc.send_event(UserEvent::Page(request));
            }
        })
        // A portal in front of the server (forward authentication) is followed
        // in the window: web addresses only, nothing else (files, other
        // schemes). Single sign-on itself goes through the browser (`sso`).
        .with_navigation_handler(|url| {
            shell::is_web_url(&url) || shell::is_shell_url(&url) || url == "about:blank"
        })
        // One window: what the interface opens elsewhere goes to the browser.
        .with_new_window_req_handler(|url, _| {
            open_in_browser(&url);
            NewWindowResponse::Deny
        })
        .with_autoplay(true)
        // An application has one size of text: no Ctrl + wheel or pinch zoom.
        .with_hotkeys_zoom(false)
        .with_devtools(cfg!(debug_assertions))
        .with_url(shell::url(false));
    let webview = build_webview(platform_options(builder), &window)?;

    let mut app = App {
        window,
        webview,
        proxy,
        config_dir,
        device: store::device_name(env),
        settings,
        resting: false,
        sign_in: Arc::new(AtomicU64::new(0)),
    };
    event_loop.run(move |event, _, control_flow| {
        // Keeps the web context alive as long as the window.
        let _ = &context;
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                app.remember_window();
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(_) | WindowEvent::Focused(_),
                ..
            } => app.rest_when_minimized(),
            Event::UserEvent(event) => app.handle(event),
            _ => {}
        }
    });
}

fn window_builder(state: store::WindowState) -> WindowBuilder {
    let (width, height) = state.size();
    let builder = WindowBuilder::new()
        .with_title("Aether")
        .with_inner_size(LogicalSize::new(width, height))
        .with_min_inner_size(LogicalSize::new(480.0, 480.0))
        .with_maximized(state.maximized)
        .with_theme(Some(Theme::Dark));
    #[cfg(windows)]
    let builder = {
        use tao::{
            platform::windows::{IconExtWindows, WindowBuilderExtWindows},
            window::Icon,
        };
        // Resource 1 is the icon embedded by build.rs.
        builder
            .with_window_icon(Icon::from_resource(1, None).ok())
            .with_taskbar_icon(Icon::from_resource(1, None).ok())
    };
    builder
}

#[cfg(windows)]
fn platform_options(builder: WebViewBuilder<'_>) -> WebViewBuilder<'_> {
    use wry::WebViewBuilderExtWindows;
    // wry's own defaults (no Edge mini menu, no SmartScreen lookups, autoplay
    // for "resume") plus what a single-site media window never uses: every one
    // of these is a background service or a process less to keep in memory.
    // The browser's own shortcuts (find, print, reload, downloads…) are off:
    // the keys belong to the interface.
    builder
        .with_browser_accelerator_keys(false)
        .with_additional_browser_args(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,Translate,OptimizationHints,MediaRouter \
         --autoplay-policy=no-user-gesture-required \
         --disable-background-networking --disable-component-update --disable-sync \
         --disable-domain-reliability --no-default-browser-check --no-first-run",
    )
}

#[cfg(not(windows))]
fn platform_options(builder: WebViewBuilder<'_>) -> WebViewBuilder<'_> {
    builder
}

#[cfg(not(target_os = "linux"))]
fn build_webview(builder: WebViewBuilder<'_>, window: &Window) -> wry::Result<WebView> {
    builder.build(window)
}

/// GTK draws the web view inside the window's own box (X11 and Wayland).
#[cfg(target_os = "linux")]
fn build_webview(builder: WebViewBuilder<'_>, window: &Window) -> wry::Result<WebView> {
    use tao::platform::unix::WindowExtUnix;
    use wry::WebViewBuilderExtUnix;
    match window.default_vbox() {
        Some(container) => builder.build_gtk(container),
        None => builder.build(window),
    }
}

struct App {
    window: Window,
    webview: WebView,
    proxy: EventLoopProxy<UserEvent>,
    config_dir: Option<PathBuf>,
    device: String,
    settings: Settings,
    /// The web engine was told to release memory (window minimized).
    resting: bool,
    /// Number of the single sign-on in progress: an older one stops waiting.
    sign_in: Arc<AtomicU64>,
}

impl App {
    fn handle(&mut self, event: UserEvent) {
        match event {
            UserEvent::Page(shell::Request::Ready { change }) => {
                match self.settings.server.clone() {
                    Some(server) if !change => self.check(server),
                    server => self.show(shell::State {
                        server,
                        ..Default::default()
                    }),
                }
            }
            UserEvent::Page(shell::Request::Connect(typed)) => {
                if typed.trim().is_empty() {
                    self.show(shell::State {
                        error: Some("empty".to_owned()),
                        ..Default::default()
                    });
                } else {
                    self.check(store::normalize_server(&typed));
                }
            }
            UserEvent::Page(shell::Request::ChangeServer) => {
                self.window.set_fullscreen(None);
                self.open(&shell::url(true));
            }
            UserEvent::Page(shell::Request::Fullscreen(on)) => self.set_fullscreen(on),
            UserEvent::Page(shell::Request::ToggleFullscreen) => {
                self.set_fullscreen(self.window.fullscreen().is_none())
            }
            UserEvent::Page(shell::Request::Sso) => self.sign_in(),
            UserEvent::Page(shell::Request::Offline) => {
                if let Some(server) = self.settings.server.clone() {
                    self.open(&store::downloads_url(&server));
                }
            }
            UserEvent::SignedIn(outcome) => self.signed_in(outcome),
            UserEvent::Checked { server, result } => match result {
                Ok(()) => {
                    self.settings.server = Some(server.clone());
                    self.save_settings();
                    self.open(&store::app_url(&server));
                }
                Err(error) => {
                    // A server opened before that no longer answers: its
                    // downloads can still be played.
                    let offline = matches!(error.as_str(), "unreachable" | "timeout")
                        && self.settings.server.as_deref() == Some(server.as_str());
                    self.show(shell::State {
                        server: Some(server),
                        error: Some(error),
                        checking: false,
                        offline,
                    })
                }
            },
        }
    }

    fn save_settings(&self) {
        if let Some(dir) = &self.config_dir
            && let Err(error) = self.settings.save(dir)
        {
            eprintln!("aether-desktop: cannot save the settings: {error}");
        }
    }

    /// The size to reopen with; a maximized or full-screen window keeps the
    /// size it had before.
    fn remember_window(&mut self) {
        let mut state = self.settings.window.unwrap_or_default();
        state.maximized = self.window.is_maximized();
        if !state.maximized && self.window.fullscreen().is_none() && !self.window.is_minimized() {
            let size = self
                .window
                .inner_size()
                .to_logical::<f64>(self.window.scale_factor());
            state.width = size.width.round() as u32;
            state.height = size.height.round() as u32;
        }
        if self.settings.window != Some(state) {
            self.settings.window = Some(state);
            self.save_settings();
        }
    }

    /// The address of the Core, when the window shows one of its pages.
    fn server_on_screen(&self) -> Option<String> {
        let server = self.settings.server.clone()?;
        let page = self.webview.url().ok()?;
        shell::is_page_of(&page, &server).then_some(server)
    }

    /// Single sign-on: the provider's page opens in the computer's browser,
    /// which hands a one-time code back to this application.
    fn sign_in(&self) {
        let Some(server) = self.server_on_screen() else {
            return;
        };
        let number = self.sign_in.fetch_add(1, Ordering::SeqCst) + 1;
        let (proxy, device, latest) = (
            self.proxy.clone(),
            self.device.clone(),
            self.sign_in.clone(),
        );
        std::thread::spawn(move || {
            let outcome = match sso::prepare(&server, &device) {
                Some(attempt) => {
                    open_in_browser(&attempt.url);
                    attempt.wait(|| latest.load(Ordering::SeqCst) == number)
                }
                None => Some(sso::Outcome::Refused(
                    "oidc_provider_unavailable".to_owned(),
                )),
            };
            if let Some(outcome) = outcome {
                let _ = proxy.send_event(UserEvent::SignedIn(outcome));
            }
        });
    }

    /// Back from the browser: the sign-in page of the Core redeems the code
    /// for its session, and the window comes back to the front.
    fn signed_in(&self, outcome: sso::Outcome) {
        if self.server_on_screen().is_none() {
            return;
        }
        let script = match &outcome {
            sso::Outcome::Code { code, verifier } => shell::sso_script(code, verifier),
            sso::Outcome::Refused(reason) => shell::sso_refused_script(reason),
        };
        if let Some(script) = script {
            let _ = self.webview.evaluate_script(&script);
        }
        self.window.set_minimized(false);
        self.window.set_focus();
    }

    fn open(&self, url: &str) {
        if let Err(error) = self.webview.load_url(url) {
            eprintln!("aether-desktop: cannot open {url}: {error}");
        }
    }

    fn show(&self, state: shell::State) {
        let _ = self.webview.evaluate_script(&state.script());
    }

    fn set_fullscreen(&self, on: bool) {
        self.window
            .set_fullscreen(on.then_some(Fullscreen::Borderless(None)));
    }

    /// Asks, off the interface thread, whether `server` is an Aether server.
    fn check(&self, server: String) {
        self.show(shell::State {
            server: Some(server.clone()),
            error: None,
            checking: true,
            offline: false,
        });
        let (proxy, device) = (self.proxy.clone(), self.device.clone());
        std::thread::spawn(move || {
            let result = verify(&server, &device);
            let _ = proxy.send_event(UserEvent::Checked { server, result });
        });
    }

    /// A minimized window keeps playing but lets the web engine give memory
    /// back to the system; it takes it again when the window returns.
    #[cfg(windows)]
    fn rest_when_minimized(&mut self) {
        use wry::{MemoryUsageLevel, WebViewExtWindows};
        let rest = self.window.is_minimized();
        if rest != self.resting {
            self.resting = rest;
            let level = if rest {
                MemoryUsageLevel::Low
            } else {
                MemoryUsageLevel::Normal
            };
            let _ = self.webview.set_memory_usage_level(level);
        }
    }

    #[cfg(not(windows))]
    fn rest_when_minimized(&mut self) {
        self.resting = self.window.is_minimized();
    }
}

fn verify(server: &str, device: &str) -> Result<(), String> {
    let reason = |error: &Error| reason(error).to_owned();
    let client = Client::new(server, device).map_err(|error| reason(&error))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "unreachable".to_owned())?;
    runtime.block_on(async {
        match tokio::time::timeout(CHECK_TIMEOUT, client.discover()).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(error)) => Err(reason(&error)),
            Err(_) => Err("timeout".to_owned()),
        }
    })
}

/// Why an address was refused, as a code the local page words in the
/// person's language.
fn reason(error: &Error) -> &'static str {
    match error {
        Error::InvalidUrl => "invalid",
        Error::NotAether | Error::Server { .. } => "not-aether",
        _ => "unreachable",
    }
}

/// Hands a web address to the default browser, without a shell in between.
fn open_in_browser(url: &str) {
    if !shell::is_web_url(url) {
        return;
    }
    let mut command = if cfg!(windows) {
        let mut command = std::process::Command::new("rundll32");
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    } else {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let mut command = std::process::Command::new(opener);
        command.arg(url);
        command
    };
    if let Err(error) = command.spawn() {
        eprintln!("aether-desktop: cannot open the browser: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_are_reported_as_codes() {
        assert_eq!(reason(&Error::InvalidUrl), "invalid");
        assert_eq!(reason(&Error::NotAether), "not-aether");
        let other_service = Error::Server {
            status: 404,
            code: String::new(),
        };
        assert_eq!(reason(&other_service), "not-aether");
        assert_eq!(
            verify("ftp://media.example", "test"),
            Err("invalid".to_owned())
        );
    }
}
