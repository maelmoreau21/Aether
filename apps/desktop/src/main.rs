//! Aether for the desktop (Windows, macOS, Linux): GPUI for the interface,
//! libmpv for playback, the shared `aether-client` core for the Core's API.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod mpv;
mod net;
mod playback;
mod remote;
mod state;
mod store;
mod ui;

use gpui::{
    App, AppContext, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use std::rc::Rc;

fn main() {
    let net = match net::Net::new() {
        Ok(net) => Rc::new(net),
        Err(error) => {
            eprintln!("aether-desktop: cannot start the network runtime: {error}");
            std::process::exit(1);
        }
    };
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Aether".into()),
                ..Default::default()
            }),
            app_id: Some("app.aether.desktop".into()),
            window_min_size: Some(size(px(720.), px(480.))),
            ..Default::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            cx.new(|cx| ui::app::AetherApp::new(net.clone(), window, cx))
        });
        if let Err(error) = opened {
            eprintln!("aether-desktop: cannot open a window: {error}");
            cx.quit();
            return;
        }
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
