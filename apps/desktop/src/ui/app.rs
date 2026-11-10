//! The application window: sign-in, then home, libraries and works, with the
//! player above them while something plays.
use super::{
    Tone, button,
    field::{FieldEvent, TextField},
    player::{PlayerEvent, PlayerView, Prepared},
    progress_bar, theme,
};
use crate::{
    net::Net,
    remote::{Action, Incoming, RemoteDevice, RemoteLink},
    state::{self, LibraryKind, LibraryState, Loadable, Navigator, Route},
    store::{self, Settings},
};
use aether_client::{Client, Error, Pairing, RecentProgress, User, Work, WorkDetail};
use futures::StreamExt;
use gpui::{
    AnyElement, Context, Entity, FocusHandle, Focusable, FontWeight, Image, ImageFormat,
    KeyDownEvent, ObjectFit, SharedString, Subscription, Task, Window, canvas, div, fill, img,
    point, prelude::*, px, size,
};
use std::{collections::HashMap, future::Future, path::PathBuf, rc::Rc, sync::Arc, time::Duration};

/// Refresh the token when it expires within a week (the Core also ends
/// sessions idle for seven days); checked at start-up and every six hours.
const REFRESH_MARGIN: i64 = 7 * 24 * 3600;
const REFRESH_EVERY: Duration = Duration::from_secs(6 * 3600);
const PAIRING_POLL: Duration = Duration::from_secs(3);
/// Concurrent poster downloads.
const POSTER_REQUESTS: usize = 6;
const POSTER_WIDTH: f32 = 160.;
const POSTER_HEIGHT: f32 = 240.;

enum Phase {
    Starting,
    SignIn,
    Ready,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SignInMode {
    Password,
    QuickConnect,
}

struct SignIn {
    server: Entity<TextField>,
    login: Entity<TextField>,
    password: Entity<TextField>,
    mode: SignInMode,
    busy: bool,
    error: Option<String>,
    /// A saved token could not be checked (network): offer to try again.
    can_retry: bool,
    pairing: Option<Pairing>,
    pairing_task: Option<Task<()>>,
}

struct Session {
    server: String,
    client: Arc<Client>,
    user: User,
}

pub struct AetherApp {
    net: Rc<Net>,
    config_dir: Option<PathBuf>,
    settings: Settings,
    device_name: String,
    focus: FocusHandle,
    phase: Phase,
    sign_in: SignIn,
    session: Option<Session>,
    nav: Navigator,
    home: Loadable<Vec<RecentProgress>>,
    libraries: HashMap<LibraryKind, LibraryState>,
    works: HashMap<String, Loadable<WorkDetail>>,
    posters: HashMap<String, Loadable<Arc<Image>>>,
    poster_slots: Arc<tokio::sync::Semaphore>,
    player: Option<(Entity<PlayerView>, Subscription)>,
    remote: Option<RemoteLink>,
    devices: Vec<RemoteDevice>,
    notice: Option<String>,
    session_tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

fn describe(error: &Error) -> String {
    match error {
        Error::InvalidUrl => "Adresse de serveur invalide.".to_owned(),
        Error::SignedOut => "Session terminée, reconnectez-vous.".to_owned(),
        Error::PairingExpired => "Le code a expiré, demandez-en un nouveau.".to_owned(),
        Error::Server { status: 401, .. } => "Identifiants refusés ou session expirée.".to_owned(),
        Error::Server { status: 403, .. } => "Accès refusé.".to_owned(),
        Error::Server { status: 404, .. } => "Introuvable sur le serveur.".to_owned(),
        Error::Server { status: 429, .. } => "Trop de tentatives, réessayez plus tard.".to_owned(),
        Error::Server { status, code } if code.is_empty() => {
            format!("Erreur du serveur ({status}).")
        }
        Error::Server { status, code } => format!("Erreur du serveur ({status}, {code})."),
        Error::Network(_) => "Serveur injoignable.".to_owned(),
    }
}

/// Image format from its first bytes; posters are JPEG, PNG or WebP.
pub fn image_format(bytes: &[u8]) -> Option<ImageFormat> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some(ImageFormat::Png),
        [0xff, 0xd8, 0xff, ..] => Some(ImageFormat::Jpeg),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some(ImageFormat::Webp),
        [b'G', b'I', b'F', b'8', ..] => Some(ImageFormat::Gif),
        _ => None,
    }
}

impl AetherApp {
    pub fn new(net: Rc<Net>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let env = |name: &str| std::env::var(name).ok();
        let config_dir = store::config_dir(env);
        let settings = config_dir
            .as_deref()
            .map(Settings::load)
            .unwrap_or_default();
        let server = cx.new(|cx| TextField::new(cx, "https://media.exemple.fr", false));
        let login = cx.new(|cx| TextField::new(cx, "Identifiant", false));
        let password = cx.new(|cx| TextField::new(cx, "Mot de passe", true));
        if let Some(saved) = &settings.server {
            server.update(cx, |field, cx| field.set_text(saved, cx));
        }
        let subscriptions = vec![
            cx.subscribe_in(&server, window, |this, _, event, window, cx| {
                match (event, this.sign_in.mode) {
                    (_, SignInMode::Password) => this.focus_field(Field::Login, window, cx),
                    (FieldEvent::Submit, SignInMode::QuickConnect) => {
                        this.start_pairing(window, cx)
                    }
                    (FieldEvent::Next, SignInMode::QuickConnect) => {}
                }
            }),
            cx.subscribe_in(&login, window, |this, _, _, window, cx| {
                this.focus_field(Field::Password, window, cx);
            }),
            cx.subscribe_in(
                &password,
                window,
                |this, _, event, window, cx| match event {
                    FieldEvent::Submit => this.sign_in_with_password(window, cx),
                    FieldEvent::Next => this.focus_field(Field::Server, window, cx),
                },
            ),
        ];
        let mut app = Self {
            net,
            config_dir,
            settings,
            device_name: store::device_name(env),
            focus: cx.focus_handle(),
            phase: Phase::SignIn,
            sign_in: SignIn {
                server,
                login,
                password,
                mode: SignInMode::Password,
                busy: false,
                error: None,
                can_retry: false,
                pairing: None,
                pairing_task: None,
            },
            session: None,
            nav: Navigator::default(),
            home: Loadable::Idle,
            libraries: HashMap::new(),
            works: HashMap::new(),
            posters: HashMap::new(),
            poster_slots: Arc::new(tokio::sync::Semaphore::new(POSTER_REQUESTS)),
            player: None,
            remote: None,
            devices: Vec::new(),
            notice: None,
            session_tasks: Vec::new(),
            _subscriptions: subscriptions,
        };
        app.restore(window, cx);
        app
    }

    /// Runs `request` (from [`Net::run`]) and applies its result to the view.
    fn after<R: 'static>(
        &self,
        request: impl Future<Output = Option<R>> + 'static,
        window: &Window,
        cx: &mut Context<Self>,
        apply: impl FnOnce(&mut Self, R, &mut Window, &mut Context<Self>) + 'static,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            let Some(result) = request.await else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                apply(this, result, window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn save_settings(&self) {
        if let Some(dir) = &self.config_dir
            && let Err(error) = self.settings.save(dir)
        {
            eprintln!("aether-desktop: settings not saved: {error}");
        }
    }

    fn focus_field(&self, field: Field, window: &mut Window, cx: &mut Context<Self>) {
        let entity = match field {
            Field::Server => &self.sign_in.server,
            Field::Login => &self.sign_in.login,
            Field::Password => &self.sign_in.password,
        };
        let handle = entity.read(cx).focus_handle(cx);
        window.focus(&handle);
    }

    // ---- Sign-in ---------------------------------------------------------------

    /// Signs back in with the token saved for the last server, if any.
    fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(server) = self.settings.server.clone() else {
            self.focus_field(Field::Server, window, cx);
            return;
        };
        self.phase = Phase::Starting;
        self.sign_in.error = None;
        self.sign_in.can_retry = false;
        let credentials = cx.read_credentials(&store::keychain_url(&server));
        let device = self.device_name.clone();
        let net = self.net.clone();
        cx.spawn_in(window, async move |this, cx| {
            let token = credentials
                .await
                .ok()
                .flatten()
                .and_then(|(_, secret)| store::decode_token(&secret));
            let client = token.map(|token| {
                Client::new(&server, &device).map(|client| Arc::new(client.with_token(token)))
            });
            let client = match client {
                None => {
                    let _ =
                        this.update_in(cx, |this, window, cx| this.show_sign_in(None, window, cx));
                    return;
                }
                Some(Err(error)) => {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.show_sign_in(Some(describe(&error)), window, cx)
                    });
                    return;
                }
                Some(Ok(client)) => client,
            };
            let checking = client.clone();
            let result = net
                .run(async move {
                    let refreshed = checking.refresh_if_due(REFRESH_MARGIN).await?;
                    Ok::<_, Error>((checking.me().await?, refreshed))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Some(Ok((user, refreshed))) => {
                    if refreshed {
                        this.save_token(&server, &user, &client, cx);
                    }
                    this.signed_in(server, client, user, window, cx);
                }
                Some(Err(error)) if error.is_unauthorized() => {
                    cx.delete_credentials(&store::keychain_url(&server))
                        .detach();
                    this.show_sign_in(
                        Some("Session expirée, reconnectez-vous.".into()),
                        window,
                        cx,
                    );
                }
                Some(Err(error)) => {
                    this.sign_in.can_retry = true;
                    this.show_sign_in(Some(describe(&error)), window, cx);
                }
                None => {}
            });
        })
        .detach();
    }

    fn show_sign_in(&mut self, error: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.phase = Phase::SignIn;
        self.sign_in.busy = false;
        self.sign_in.error = error;
        let field = if self.sign_in.server.read(cx).text().is_empty() {
            Field::Server
        } else {
            Field::Login
        };
        self.focus_field(field, window, cx);
        cx.notify();
    }

    /// A client for the address typed in the sign-in form.
    fn typed_client(&mut self, cx: &mut Context<Self>) -> Option<(String, Arc<Client>)> {
        let server = store::normalize_server(self.sign_in.server.read(cx).text());
        match Client::new(&server, &self.device_name) {
            Ok(client) => Some((server, Arc::new(client))),
            Err(error) => {
                self.sign_in.error = Some(describe(&error));
                cx.notify();
                None
            }
        }
    }

    fn sign_in_with_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sign_in.busy {
            return;
        }
        let login = self.sign_in.login.read(cx).text().trim().to_owned();
        let password = self.sign_in.password.read(cx).text().to_owned();
        if login.is_empty() || password.is_empty() {
            self.sign_in.error = Some("Indiquez votre identifiant et votre mot de passe.".into());
            cx.notify();
            return;
        }
        let Some((server, client)) = self.typed_client(cx) else {
            return;
        };
        self.sign_in.busy = true;
        self.sign_in.error = None;
        cx.notify();
        let signing = client.clone();
        let request = self
            .net
            .run(async move { signing.login(&login, &password).await });
        self.after(
            request,
            window,
            cx,
            move |this, result, window, cx| match result {
                Ok(user) => {
                    this.save_token(&server, &user, &client, cx);
                    this.sign_in.password.update(cx, |f, cx| f.set_text("", cx));
                    this.signed_in(server, client, user, window, cx);
                }
                Err(error) => this.show_sign_in(Some(describe(&error)), window, cx),
            },
        );
    }

    /// Quick Connect: shows a code and a QR code, approved from a signed-in
    /// phone or the web, and polls until then.
    fn start_pairing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((server, client)) = self.typed_client(cx) else {
            return;
        };
        self.sign_in.busy = true;
        self.sign_in.error = None;
        self.sign_in.pairing = None;
        cx.notify();
        let net = self.net.clone();
        let task = cx.spawn_in(window, async move |this, cx| {
            let starting = client.clone();
            let pairing = match net.run(async move { starting.start_pairing().await }).await {
                Some(Ok(pairing)) => pairing,
                Some(Err(error)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.sign_in.busy = false;
                        this.sign_in.error = Some(describe(&error));
                        cx.notify();
                    });
                    return;
                }
                None => return,
            };
            let _ = this.update(cx, |this, cx| {
                this.sign_in.busy = false;
                this.sign_in.pairing = Some(pairing.clone());
                cx.notify();
            });
            loop {
                cx.background_executor().timer(PAIRING_POLL).await;
                let (polling, pending) = (client.clone(), pairing.clone());
                let result = net
                    .run(async move { polling.poll_pairing(&pending).await })
                    .await;
                let outcome = this.update_in(cx, |this, window, cx| match result {
                    Some(Ok(None)) => true,
                    Some(Ok(Some(user))) => {
                        this.save_token(&server, &user, &client, cx);
                        this.signed_in(server.clone(), client.clone(), user, window, cx);
                        false
                    }
                    Some(Err(error)) => {
                        this.sign_in.pairing = None;
                        this.sign_in.error = Some(describe(&error));
                        cx.notify();
                        false
                    }
                    None => false,
                });
                if !matches!(outcome, Ok(true)) {
                    return;
                }
            }
        });
        self.sign_in.pairing_task = Some(task);
    }

    fn save_token(&mut self, server: &str, user: &User, client: &Client, cx: &mut Context<Self>) {
        let Some(token) = client.token() else { return };
        let write = cx.write_credentials(
            &store::keychain_url(server),
            &user.login,
            &store::encode_token(&token),
        );
        cx.spawn(async move |_, _| {
            if let Err(error) = write.await {
                eprintln!("aether-desktop: token not saved in the keychain: {error}");
            }
        })
        .detach();
        self.settings.server = Some(server.to_owned());
        self.save_settings();
    }

    fn signed_in(
        &mut self,
        server: String,
        client: Arc<Client>,
        user: User,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sign_in.busy = false;
        self.sign_in.error = None;
        self.sign_in.can_retry = false;
        self.sign_in.pairing = None;
        // The polling loop may be the caller: let it finish on its own.
        if let Some(task) = self.sign_in.pairing_task.take() {
            task.detach();
        }
        self.phase = Phase::Ready;
        self.nav = Navigator::default();
        self.start_remote(&server, &client, window, cx);
        self.start_refresh(&server, window, cx);
        self.session = Some(Session {
            server,
            client,
            user,
        });
        self.load_home(window, cx);
        window.focus(&self.focus);
        cx.notify();
    }

    fn start_remote(
        &mut self,
        server: &str,
        client: &Arc<Client>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let device_id = self.settings.device_id();
        self.save_settings();
        let reader = client.clone();
        let token = Arc::new(move || reader.token().map(|t| t.access));
        let Some((link, mut incoming)) = RemoteLink::start(
            self.net.handle(),
            server,
            device_id,
            self.device_name.clone(),
            token,
        ) else {
            return;
        };
        self.remote = Some(link);
        let listener = cx.spawn_in(window, async move |this, cx| {
            while let Some(message) = incoming.next().await {
                let applied =
                    this.update_in(cx, |this, window, cx| this.on_remote(message, window, cx));
                if applied.is_err() {
                    return;
                }
            }
        });
        self.session_tasks.push(listener);
    }

    fn start_refresh(&mut self, server: &str, window: &Window, cx: &mut Context<Self>) {
        let server = server.to_owned();
        let net = self.net.clone();
        let task = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(REFRESH_EVERY).await;
                let Ok(Some(client)) = this.update(cx, |this, _| this.client()) else {
                    return;
                };
                let refreshing = client.clone();
                let result = net
                    .run(async move { refreshing.refresh_if_due(REFRESH_MARGIN).await })
                    .await;
                let _ = this.update_in(cx, |this, window, cx| match result {
                    Some(Ok(true)) => {
                        if let Some(user) = this.session.as_ref().map(|s| s.user.clone()) {
                            this.save_token(&server, &user, &client, cx);
                        }
                    }
                    Some(Err(error)) => this.check_unauthorized(&error, window, cx),
                    _ => {}
                });
            }
        });
        self.session_tasks.push(task);
    }

    fn sign_out(&mut self, message: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.close_player(cx);
        self.remote = None;
        self.devices.clear();
        self.session_tasks.clear();
        if let Some(session) = self.session.take() {
            let client = session.client;
            // Runs on the network runtime whether or not anyone awaits it.
            drop(self.net.run(async move { client.logout().await }));
            cx.delete_credentials(&store::keychain_url(&session.server))
                .detach();
        }
        self.home = Loadable::Idle;
        self.libraries.clear();
        self.works.clear();
        self.posters.clear();
        self.nav = Navigator::default();
        self.show_sign_in(message, window, cx);
    }

    /// Any request refused with 401: the token was revoked or expired.
    fn check_unauthorized(&mut self, error: &Error, window: &mut Window, cx: &mut Context<Self>) {
        if error.is_unauthorized() && self.session.is_some() {
            self.sign_out(
                Some("Session expirée, reconnectez-vous.".into()),
                window,
                cx,
            );
        }
    }

    // ---- Data -----------------------------------------------------------------

    fn client(&self) -> Option<Arc<Client>> {
        self.session.as_ref().map(|s| s.client.clone())
    }

    fn load_home(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(client) = self.client() else { return };
        if !matches!(self.home, Loadable::Ready(_)) {
            self.home = Loadable::Loading;
        }
        let request = self.net.run(async move { client.recent_progress().await });
        self.after(
            request,
            window,
            cx,
            |this, result, window, cx| match result {
                Ok(recent) => this.home = Loadable::Ready(state::continue_watching(recent)),
                Err(error) => {
                    this.home = Loadable::Failed(describe(&error));
                    this.check_unauthorized(&error, window, cx);
                }
            },
        );
    }

    fn load_library(&mut self, kind: LibraryKind, window: &Window, cx: &mut Context<Self>) {
        let Some(client) = self.client() else { return };
        let library = self.libraries.entry(kind).or_default();
        let Some(query) = library.next_query(kind) else {
            return;
        };
        library.start();
        let request = self.net.run(async move { client.catalog(&query).await });
        self.after(
            request,
            window,
            cx,
            move |this, result, window, cx| match result {
                Ok(page) => {
                    let posters: Vec<String> = page
                        .items
                        .iter()
                        .filter_map(|w| w.poster_url.clone())
                        .collect();
                    this.libraries.entry(kind).or_default().apply(page);
                    for path in posters {
                        this.load_poster(path, window, cx);
                    }
                }
                Err(error) => {
                    this.libraries
                        .entry(kind)
                        .or_default()
                        .fail(describe(&error));
                    this.check_unauthorized(&error, window, cx);
                }
            },
        );
        cx.notify();
    }

    fn load_work(&mut self, id: &str, window: &Window, cx: &mut Context<Self>) {
        if !self.works.get(id).is_none_or(Loadable::needs_load) {
            return;
        }
        let Some(client) = self.client() else { return };
        self.works.insert(id.to_owned(), Loadable::Loading);
        let key = id.to_owned();
        let request = self.net.run(async move { client.work(&key).await });
        let id = id.to_owned();
        self.after(
            request,
            window,
            cx,
            move |this, result, window, cx| match result {
                Ok(detail) => {
                    if let Some(path) = detail.work.poster_url.clone() {
                        this.load_poster(path, window, cx);
                    }
                    this.works.insert(id, Loadable::Ready(detail));
                }
                Err(error) => {
                    this.works.insert(id, Loadable::Failed(describe(&error)));
                    this.check_unauthorized(&error, window, cx);
                }
            },
        );
    }

    fn load_poster(&mut self, path: String, window: &Window, cx: &mut Context<Self>) {
        if !self.posters.get(&path).is_none_or(Loadable::needs_load) {
            return;
        }
        let Some(client) = self.client() else { return };
        self.posters.insert(path.clone(), Loadable::Loading);
        let slots = self.poster_slots.clone();
        let key = path.clone();
        let request = self.net.run(async move {
            let _slot = slots.acquire_owned().await;
            client.image(&key).await
        });
        self.after(request, window, cx, move |this, result, _, _| {
            let poster = match result {
                Ok(bytes) => match image_format(&bytes) {
                    Some(format) => Loadable::Ready(Arc::new(Image::from_bytes(format, bytes))),
                    None => Loadable::Failed("format".into()),
                },
                Err(error) => Loadable::Failed(describe(&error)),
            };
            this.posters.insert(path, poster);
        });
    }

    // ---- Navigation and playback ---------------------------------------------------

    fn go(&mut self, route: Route, tab: bool, window: &Window, cx: &mut Context<Self>) {
        self.notice = None;
        match &route {
            Route::Home => self.load_home(window, cx),
            Route::Library(kind) => {
                if !self.libraries.get(kind).is_some_and(|l| l.loaded_once) {
                    self.load_library(*kind, window, cx);
                }
            }
            Route::Work(id) => self.load_work(id, window, cx),
        }
        if tab {
            self.nav.tab(route);
        } else {
            self.nav.push(route);
        }
        cx.notify();
    }

    /// Plays a film or an episode; `start` overrides the saved position (a
    /// handover from another device).
    fn play(
        &mut self,
        work_id: String,
        title: String,
        start: Option<f64>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(client) = self.client() else { return };
        self.close_player(cx);
        self.notice = None;
        let net = self.net.clone();
        let preparing = client.clone();
        let id = work_id.clone();
        let request = self
            .net
            .run(async move { Prepared::fetch(&preparing, &id, start).await });
        self.after(
            request,
            window,
            cx,
            move |this, result, window, cx| match result {
                Ok(prepared) => {
                    let player = cx.new(|cx| {
                        PlayerView::new(prepared, work_id, title, client, net, window, cx)
                    });
                    let subscription = cx.subscribe_in(&player, window, Self::on_player);
                    let handle = player.read(cx).focus_handle(cx);
                    window.focus(&handle);
                    this.player = Some((player, subscription));
                }
                Err(error) => {
                    this.notice = Some(format!("Lecture impossible : {}", describe(&error)));
                    this.check_unauthorized(&error, window, cx);
                }
            },
        );
    }

    fn on_player(
        &mut self,
        _: &Entity<PlayerView>,
        event: &PlayerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            PlayerEvent::Report(playing) => {
                if let Some(remote) = &self.remote {
                    remote.report(playing.clone());
                }
            }
            PlayerEvent::Closed => {
                self.player = None;
                if let Some(remote) = &self.remote {
                    remote.report(None);
                }
                window.focus(&self.focus);
                // Positions changed: refresh the "Continue" row.
                self.load_home(window, cx);
                cx.notify();
            }
        }
    }

    fn close_player(&mut self, cx: &mut Context<Self>) {
        if let Some((player, _)) = self.player.take() {
            player.update(cx, |player, cx| player.shut_down(cx));
            if let Some(remote) = &self.remote {
                remote.report(None);
            }
        }
    }

    fn on_remote(&mut self, message: Incoming, window: &mut Window, cx: &mut Context<Self>) {
        match message {
            Incoming::Devices(devices) => self.devices = devices,
            Incoming::Command(command) => match command.action {
                Action::Open { work_id, position } => {
                    self.notice = Some(format!("Ouvert depuis {}", command.from_name));
                    self.play(work_id, String::new(), position, window, cx);
                }
                action => {
                    if let Some((player, _)) = &self.player {
                        player.update(cx, |player, cx| player.remote(&action, cx));
                    }
                }
            },
        }
        cx.notify();
    }

    fn send_to(&self, device: &str, action: Action) {
        if let Some(remote) = &self.remote {
            remote.send(device, &action);
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keys = &event.keystroke;
        let back = keys.key == "escape" || (keys.modifiers.alt && keys.key == "left");
        if back && matches!(self.phase, Phase::Ready) && self.nav.back() {
            cx.notify();
        }
    }
}

#[derive(Clone, Copy)]
enum Field {
    Server,
    Login,
    Password,
}

// ---- Rendering --------------------------------------------------------------------

impl Render for AetherApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.phase {
            Phase::Starting => centered(div().text_color(theme::MUTED).child("Connexion…")),
            Phase::SignIn => self.render_sign_in(cx),
            Phase::Ready => self.render_main(window, cx),
        };
        div()
            .id("aether")
            .key_context("Aether")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .relative()
            .bg(theme::BACKGROUND)
            .text_color(theme::TEXT)
            .font_family(".SystemUIFont")
            .child(body)
            .when_some(self.player.as_ref(), |root, (player, _)| {
                root.child(div().absolute().inset_0().child(player.clone()))
            })
    }
}

fn centered(content: impl IntoElement) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(content)
        .into_any_element()
}

fn heading(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .text_xl()
        .font_weight(FontWeight::BOLD)
        .child(text.into())
}

fn muted(text: impl Into<SharedString>) -> impl IntoElement {
    div().text_sm().text_color(theme::MUTED).child(text.into())
}

fn error_text(text: impl Into<SharedString>) -> impl IntoElement {
    div().text_sm().text_color(theme::DANGER).child(text.into())
}

/// A Quick Connect QR code, drawn module by module.
fn qr_code(pairing: &Pairing) -> impl IntoElement {
    let width = pairing.qr_width;
    let modules = pairing.qr_modules.clone();
    let side = 220.;
    div()
        .size(px(side + 24.))
        .p(px(12.))
        .rounded_md()
        .bg(gpui::white())
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    if width == 0 {
                        return;
                    }
                    let cell = f32::from(bounds.size.width) / width as f32;
                    for (index, dark) in modules.iter().enumerate() {
                        if *dark {
                            let (x, y) = ((index % width) as f32, (index / width) as f32);
                            let origin = bounds.origin + point(px(x * cell), px(y * cell));
                            window.paint_quad(fill(
                                gpui::Bounds::new(origin, size(px(cell + 0.5), px(cell + 0.5))),
                                gpui::black(),
                            ));
                        }
                    }
                },
            )
            .size(px(side)),
        )
}

impl AetherApp {
    fn render_sign_in(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let sign_in = &self.sign_in;
        let mode = sign_in.mode;
        let tab =
            |id: &'static str, label: &'static str, target: SignInMode, cx: &mut Context<Self>| {
                let active = mode == target;
                div()
                    .id(id)
                    .flex_1()
                    .py_2()
                    .flex()
                    .justify_center()
                    .rounded_md()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .when(active, |t| t.bg(theme::RAISED))
                    .text_color(if active { theme::TEXT } else { theme::MUTED })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.sign_in.mode = target;
                        this.sign_in.error = None;
                        this.sign_in.pairing = None;
                        this.sign_in.pairing_task = None;
                        this.sign_in.busy = false;
                        if target == SignInMode::Password {
                            this.focus_field(Field::Login, window, cx);
                        }
                        cx.notify();
                    }))
                    .child(label)
            };
        let mut form = div()
            .w(px(380.))
            .p_6()
            .flex()
            .flex_col()
            .gap_4()
            .rounded_lg()
            .bg(theme::SURFACE)
            .border_1()
            .border_color(theme::BORDER)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(FontWeight::BOLD)
                            .child("Aether"),
                    )
                    .child(muted("Connectez cet ordinateur à votre serveur.")),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(muted("Adresse du serveur"))
                    .child(sign_in.server.clone()),
            )
            .child(
                div()
                    .flex()
                    .p_1()
                    .gap_1()
                    .rounded_md()
                    .bg(theme::BACKGROUND)
                    .child(tab(
                        "mode-password",
                        "Identifiants",
                        SignInMode::Password,
                        cx,
                    ))
                    .child(tab(
                        "mode-quick",
                        "Quick Connect",
                        SignInMode::QuickConnect,
                        cx,
                    )),
            );
        form = match mode {
            SignInMode::Password => form
                .child(sign_in.login.clone())
                .child(sign_in.password.clone())
                .child(button(
                    "sign-in",
                    if sign_in.busy { "Connexion…" } else { "Se connecter" },
                    Tone::Primary,
                    cx.listener(|this, _, window, cx| this.sign_in_with_password(window, cx)),
                )),
            SignInMode::QuickConnect => match &sign_in.pairing {
                Some(pairing) => form
                    .child(muted(
                        "Sur un appareil déjà connecté, ouvrez Quick Connect et saisissez ce code, ou scannez le QR code.",
                    ))
                    .child(
                        div()
                            .flex()
                            .justify_center()
                            .text_3xl()
                            .font_weight(FontWeight::BOLD)
                            .child(pairing.code.clone()),
                    )
                    .child(div().flex().justify_center().child(qr_code(pairing)))
                    .child(muted("En attente de validation…")),
                None => form
                    .child(muted(
                        "Obtenez un code à valider depuis un téléphone ou le web déjà connecté.",
                    ))
                    .child(button(
                        "pairing",
                        if sign_in.busy { "Demande…" } else { "Obtenir un code" },
                        Tone::Primary,
                        cx.listener(|this, _, window, cx| this.start_pairing(window, cx)),
                    )),
            },
        };
        if let Some(error) = &sign_in.error {
            form = form.child(error_text(error.clone()));
        }
        if sign_in.can_retry {
            form = form.child(button(
                "retry",
                "Réessayer avec la session enregistrée",
                Tone::Secondary,
                cx.listener(|this, _, window, cx| this.restore(window, cx)),
            ));
        }
        centered(form)
    }

    fn render_main(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let page = match self.nav.current().clone() {
            Route::Home => self.render_home(cx),
            Route::Library(kind) => self.render_library(kind, cx),
            Route::Work(id) => self.render_work(&id, window, cx),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_top_bar(cx))
            .when_some(self.notice.clone(), |main, notice| {
                main.child(
                    div()
                        .px_6()
                        .py_2()
                        .bg(theme::RAISED)
                        .text_sm()
                        .child(notice),
                )
            })
            .child(
                div()
                    .id("page")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_6()
                    .child(page),
            )
            .into_any_element()
    }

    fn render_top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let section = self.nav.section().clone();
        let tab =
            |id: &'static str, label: &'static str, route: Route, cx: &mut Context<Self>| {
                let active = section == route;
                div()
                    .id(id)
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if active { theme::TEXT } else { theme::MUTED })
                    .when(active, |t| t.bg(theme::RAISED))
                    .hover(|t| t.text_color(theme::TEXT))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.go(route.clone(), true, window, cx)
                    }))
                    .child(label)
            };
        let user = self
            .session
            .as_ref()
            .map(|s| {
                s.user
                    .display_name
                    .clone()
                    .unwrap_or_else(|| s.user.login.clone())
            })
            .unwrap_or_default();
        div()
            .h(px(56.))
            .px_6()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(theme::BORDER)
            .when(self.nav.can_go_back(), |bar| {
                bar.child(button(
                    "back",
                    "‹ Retour",
                    Tone::Quiet,
                    cx.listener(|this, _, _, cx| {
                        this.nav.back();
                        cx.notify();
                    }),
                ))
            })
            .child(
                div()
                    .mr_4()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .child("Aether"),
            )
            .child(tab("tab-home", "Accueil", Route::Home, cx))
            .child(tab(
                "tab-films",
                "Films",
                Route::Library(LibraryKind::Films),
                cx,
            ))
            .child(tab(
                "tab-series",
                "Séries",
                Route::Library(LibraryKind::Series),
                cx,
            ))
            .child(div().flex_1())
            .child(muted(user))
            .child(button(
                "sign-out",
                "Déconnexion",
                Tone::Quiet,
                cx.listener(|this, _, window, cx| this.sign_out(None, window, cx)),
            ))
    }

    fn render_home(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut page = div().flex().flex_col().gap_6();
        page = page.child(heading("Continuer"));
        page = match &self.home {
            Loadable::Ready(items) if items.is_empty() => {
                page.child(muted("Rien en cours. Choisissez un film ou une série."))
            }
            Loadable::Ready(items) => page.child(
                div()
                    .id("continue")
                    .flex()
                    .gap_4()
                    .overflow_x_scroll()
                    .pb_2()
                    .children(
                        items
                            .iter()
                            .enumerate()
                            .map(|(i, item)| self.continue_card(i, item, cx)),
                    ),
            ),
            Loadable::Failed(error) => page.child(error_text(error.clone())),
            Loadable::Idle | Loadable::Loading => page.child(muted("Chargement…")),
        };
        if !self.devices.is_empty() {
            page = page.child(heading("Vos autres appareils"));
            page = page.children(
                self.devices
                    .iter()
                    .enumerate()
                    .map(|(i, d)| self.device_row(i, d, cx)),
            );
        }
        page.into_any_element()
    }

    fn continue_card(
        &self,
        index: usize,
        item: &RecentProgress,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (id, title) = (item.work_id.clone(), item.title.clone());
        div()
            .id(("continue", index))
            .w(px(260.))
            .flex_none()
            .p_4()
            .flex()
            .flex_col()
            .gap_2()
            .rounded_lg()
            .bg(theme::SURFACE)
            .border_1()
            .border_color(theme::BORDER)
            .cursor_pointer()
            .hover(|card| card.border_color(theme::ACCENT))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.play(id.clone(), title.clone(), None, window, cx)
            }))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .truncate()
                    .child(item.title.clone()),
            )
            .child(muted(
                state::remaining(item.position, item.duration)
                    .unwrap_or_else(|| state::clock(item.position)),
            ))
            .child(progress_bar(state::fraction(item.position, item.duration)))
            .into_any_element()
    }

    fn device_row(
        &self,
        index: usize,
        device: &RemoteDevice,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let kind = match device.kind.as_str() {
            "tv" => "Télévision",
            "web" => "Navigateur",
            "desktop" => "Ordinateur",
            _ => "Téléphone",
        };
        let status = match &device.playing {
            Some(p) => format!(
                "{} {} · {}",
                if p.paused { "En pause :" } else { "Lit" },
                p.title.clone().unwrap_or_else(|| "une vidéo".into()),
                state::clock(p.position)
            ),
            None => "Inactif".to_owned(),
        };
        let mut row = div()
            .id(("device", index))
            .p_4()
            .flex()
            .items_center()
            .gap_3()
            .rounded_lg()
            .bg(theme::SURFACE)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(device.name.clone()),
                    )
                    .child(muted(format!("{kind} · {status}"))),
            );
        if let Some(playing) = &device.playing {
            let to = device.device_id.clone();
            let pause = if playing.paused {
                Action::Play
            } else {
                Action::Pause
            };
            let label = if playing.paused { "Lecture" } else { "Pause" };
            let (stop_to, here) = (to.clone(), playing.clone());
            row = row
                .child(button(
                    ("device-toggle", index),
                    label,
                    Tone::Secondary,
                    cx.listener(move |this, _, _, _| this.send_to(&to, pause.clone())),
                ))
                .child(button(
                    ("device-stop", index),
                    "Arrêter",
                    Tone::Secondary,
                    cx.listener(move |this, _, _, _| this.send_to(&stop_to, Action::Stop)),
                ))
                .child(button(
                    ("device-here", index),
                    "Reprendre ici",
                    Tone::Primary,
                    cx.listener(move |this, _, window, cx| {
                        let title = here.title.clone().unwrap_or_default();
                        this.play(here.work_id.clone(), title, Some(here.position), window, cx)
                    }),
                ));
        }
        row.into_any_element()
    }

    fn poster(&self, path: Option<&str>, width: f32, height: f32) -> AnyElement {
        let frame = div()
            .w(px(width))
            .h(px(height))
            .flex_none()
            .rounded_md()
            .overflow_hidden()
            .bg(theme::RAISED);
        match path
            .and_then(|p| self.posters.get(p))
            .and_then(Loadable::ready)
        {
            Some(image) => frame
                .child(img(image.clone()).size_full().object_fit(ObjectFit::Cover))
                .into_any_element(),
            None => frame.into_any_element(),
        }
    }

    fn render_library(&self, kind: LibraryKind, cx: &mut Context<Self>) -> AnyElement {
        let empty = LibraryState::default();
        let library = self.libraries.get(&kind).unwrap_or(&empty);
        let mut page = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(heading(match library.total {
                Some(total) => format!("{} · {total}", kind.label()),
                None => kind.label().to_owned(),
            }));
        if library.loaded_once && library.items.is_empty() {
            page = page.child(muted("Cette bibliothèque est vide."));
        }
        page = page.child(
            div().flex().flex_wrap().gap_4().children(
                library
                    .items
                    .iter()
                    .enumerate()
                    .map(|(i, work)| self.work_card(i, work, cx)),
            ),
        );
        if let Some(error) = &library.error {
            page = page.child(error_text(error.clone()));
        }
        if library.loading {
            page = page.child(muted("Chargement…"));
        } else if library.has_more || library.error.is_some() {
            page = page.child(button(
                "more",
                "Afficher plus",
                Tone::Secondary,
                cx.listener(move |this, _, window, cx| this.load_library(kind, window, cx)),
            ));
        }
        page.into_any_element()
    }

    fn work_card(&self, index: usize, work: &Work, cx: &mut Context<Self>) -> AnyElement {
        let id = work.id.clone();
        div()
            .id(("work", index))
            .w(px(POSTER_WIDTH))
            .flex()
            .flex_col()
            .gap_1()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.go(Route::Work(id.clone()), false, window, cx)
            }))
            .child(self.poster(work.poster_url.as_deref(), POSTER_WIDTH, POSTER_HEIGHT))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .truncate()
                    .child(work.title.clone()),
            )
            .when_some(work.year, |card, year| card.child(muted(year.to_string())))
            .into_any_element()
    }

    fn render_work(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.works.get(id).is_none_or(Loadable::needs_load)
            && !matches!(self.works.get(id), Some(Loadable::Failed(_)))
        {
            self.load_work(id, window, cx);
        }
        let detail = match self.works.get(id) {
            Some(Loadable::Ready(detail)) => detail,
            Some(Loadable::Failed(error)) => return error_text(error.clone()).into_any_element(),
            _ => return muted("Chargement…").into_any_element(),
        };
        let work = &detail.work;
        let mut info = div()
            .flex_1()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_3xl()
                    .font_weight(FontWeight::BOLD)
                    .child(work.title.clone()),
            )
            .when_some(work.year, |info, year| info.child(muted(year.to_string())))
            .child(div().max_w(px(720.)).child(work.description.clone()));
        if detail.seasons.is_empty() {
            let (id, title) = (work.id.clone(), work.title.clone());
            info = info.child(div().flex().child(button(
                "play",
                "▶ Lire",
                Tone::Primary,
                cx.listener(move |this, _, window, cx| {
                    this.play(id.clone(), title.clone(), None, window, cx)
                }),
            )));
        }
        let mut page = div().flex().flex_col().gap_8().child(
            div()
                .flex()
                .gap_6()
                .child(self.poster(work.poster_url.as_deref(), 240., 360.))
                .child(info),
        );
        for (s, season) in detail.seasons.iter().enumerate() {
            let name = if season.title.is_empty() {
                season
                    .number
                    .map_or_else(|| "Saison".to_owned(), |n| format!("Saison {n}"))
            } else {
                season.title.clone()
            };
            let mut list = div().flex().flex_col().gap_2().child(heading(name));
            for (e, episode) in season.episodes.iter().enumerate() {
                let (id, title) = (episode.id.clone(), episode.title.clone());
                let number = episode.number.map(|n| format!("{n}. ")).unwrap_or_default();
                list = list.child(
                    div()
                        .id(("episode", s * 1000 + e))
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .rounded_md()
                        .bg(theme::SURFACE)
                        .cursor_pointer()
                        .hover(|row| row.bg(theme::RAISED))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.play(id.clone(), title.clone(), None, window, cx)
                        }))
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("{number}{}", episode.title)),
                        )
                        .when(!episode.description.is_empty(), |row| {
                            row.child(
                                div()
                                    .text_sm()
                                    .text_color(theme::MUTED)
                                    .line_clamp(2)
                                    .child(episode.description.clone()),
                            )
                        }),
                );
            }
            page = page.child(list);
        }
        page.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poster_formats_are_sniffed() {
        assert_eq!(image_format(b"\x89PNG\r\n"), Some(ImageFormat::Png));
        assert_eq!(
            image_format(&[0xff, 0xd8, 0xff, 0xe0]),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(
            image_format(b"RIFF\0\0\0\0WEBPVP8 "),
            Some(ImageFormat::Webp)
        );
        assert_eq!(image_format(b"<svg"), None);
        assert_eq!(image_format(b""), None);
    }

    #[test]
    fn errors_are_explained_in_french() {
        assert_eq!(describe(&Error::InvalidUrl), "Adresse de serveur invalide.");
        assert_eq!(
            describe(&Error::Server {
                status: 500,
                code: "boom".into()
            }),
            "Erreur du serveur (500, boom)."
        );
    }
}
