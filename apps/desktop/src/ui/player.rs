//! The player: mpv's frames fill the window, controls appear on mouse move
//! or when paused. The position is saved to the Core while playing and the
//! remote control sees what plays.
use super::{Tone, button, theme};
use crate::{
    mpv::{self, Engine, Event},
    net::Net,
    playback::{self, ProgressSaver, Track, TrackKind},
    remote::{Action, Playing},
    state,
};
use aether_client::{Client, Progress, ProgressUpdate};
use futures::StreamExt;
use gpui::{
    App, Bounds, Context, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent,
    MouseButton, MouseDownEvent, ObjectFit, Pixels, RenderImage, SharedString, Task, Window,
    canvas, div, img, prelude::*, px,
};
use std::{
    cell::Cell,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

const CONTROLS_VISIBLE: Duration = Duration::from_secs(3);
const SEEK_STEP: f64 = 10.0;
/// How often the remote control hears the position while playing.
const REPORT_EVERY: Duration = Duration::from_secs(5);

/// What the Core said before playback starts.
pub struct Prepared {
    url: String,
    authorization: String,
    start: f64,
    progress: Option<Progress>,
}

impl Prepared {
    /// Resolves the file of a work and where to resume it. `start` (a
    /// handover from another device) wins over the saved position.
    pub async fn fetch(
        client: &Client,
        work_id: &str,
        start: Option<f64>,
    ) -> aether_client::Result<Self> {
        let media = client.media(work_id).await?;
        if !media.available {
            return Err(aether_client::Error::Server {
                status: 404,
                code: "media_unavailable".into(),
            });
        }
        let progress = client.progress(work_id).await.ok();
        let token = client.token().ok_or(aether_client::Error::SignedOut)?;
        Ok(Self {
            url: client.content_url(&media.id)?,
            authorization: format!("Bearer {}", token.access),
            start: start.unwrap_or_else(|| playback::resume_at(progress.as_ref())),
            progress,
        })
    }
}

pub enum PlayerEvent {
    /// What plays now, for the remote control.
    Report(Option<Playing>),
    Closed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Menu {
    Audio,
    Subtitles,
}

pub struct PlayerView {
    engine: Option<Engine>,
    focus: FocusHandle,
    work_id: String,
    title: String,
    client: Arc<Client>,
    net: Rc<Net>,
    position: f64,
    duration: f64,
    paused: bool,
    buffering: bool,
    video: Option<(u32, u32)>,
    tracks: Vec<Track>,
    frame: Option<Arc<RenderImage>>,
    /// Frames replaced since the last render, removed from GPUI's atlas then.
    stale: Vec<Arc<RenderImage>>,
    saver: ProgressSaver,
    clock: Instant,
    controls_until: Instant,
    last_report: Option<Instant>,
    menu: Option<Menu>,
    error: Option<String>,
    volume: f64,
    seek_bar: Rc<Cell<Option<Bounds<Pixels>>>>,
    closed: bool,
    /// A remote "stop": closing needs the window, done at the next render.
    stop_requested: bool,
    _tasks: Vec<Task<()>>,
}

impl EventEmitter<PlayerEvent> for PlayerView {}

impl Focusable for PlayerView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PlayerView {
    pub fn new(
        prepared: Prepared,
        work_id: String,
        title: String,
        client: Arc<Client>,
        net: Rc<Net>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let source = mpv::Source {
            url: prepared.url,
            authorization: Some(prepared.authorization),
            start: prepared.start,
        };
        let saver = ProgressSaver::new(prepared.progress.as_ref().map_or(0, |p| p.revision));
        let mut view = Self {
            engine: None,
            focus: cx.focus_handle(),
            work_id,
            title,
            client,
            net,
            position: prepared.start,
            duration: prepared.progress.as_ref().map_or(0.0, |p| p.duration),
            paused: false,
            buffering: true,
            video: None,
            tracks: Vec::new(),
            frame: None,
            stale: Vec::new(),
            saver,
            clock: Instant::now(),
            controls_until: Instant::now() + CONTROLS_VISIBLE,
            last_report: None,
            menu: None,
            error: None,
            volume: 100.0,
            seek_bar: Rc::new(Cell::new(None)),
            closed: false,
            stop_requested: false,
            _tasks: Vec::new(),
        };
        match Engine::start(&source, &[]) {
            Ok((engine, channels)) => {
                view.engine = Some(engine);
                view.listen(channels, window, cx);
            }
            Err(error) => view.error = Some(error),
        }
        view
    }

    fn listen(&mut self, channels: mpv::Channels, window: &Window, cx: &mut Context<Self>) {
        let mpv::Channels {
            mut events,
            mut frames,
        } = channels;
        let events = cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = events.next().await {
                if this
                    .update_in(cx, |this, window, cx| this.on_event(event, window, cx))
                    .is_err()
                {
                    return;
                }
            }
        });
        let frames = cx.spawn(async move |this, cx| {
            while frames.next().await.is_some() {
                let shown = this.update(cx, |this, cx| {
                    if let Some(frame) = this.engine.as_ref().and_then(Engine::take_frame) {
                        if let Some(old) = this.frame.replace(frame) {
                            this.stale.push(old);
                        }
                        cx.notify();
                    }
                });
                if shown.is_err() {
                    return;
                }
            }
        });
        // Hides the controls, saves and reports the position on a steady beat.
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                    return;
                }
            }
        });
        self._tasks = vec![events, frames, ticker];
    }

    fn now(&self) -> f64 {
        self.clock.elapsed().as_secs_f64()
    }

    fn on_event(&mut self, event: Event, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            Event::Loaded => self.buffering = false,
            Event::Time(position) => self.position = position,
            Event::Duration(duration) => self.duration = duration,
            Event::Paused(paused) => {
                self.paused = paused;
                self.save(true, false, cx);
                self.report(cx);
            }
            Event::Buffering(buffering) => self.buffering = buffering,
            Event::VideoSize(width, height) => self.video = Some((width, height)),
            Event::Tracks(tracks) => self.tracks = tracks,
            Event::Ended { eof, error } => {
                if let Some(error) = error {
                    self.error = Some(format!("Lecture interrompue : {error}"));
                } else {
                    if eof {
                        self.position = self.duration.max(self.position);
                        self.save(true, true, cx);
                    }
                    self.close(window, cx);
                    return;
                }
            }
        }
        cx.notify();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        if !self.paused {
            self.save(false, false, cx);
        }
        if self
            .last_report
            .is_none_or(|at| at.elapsed() >= REPORT_EVERY)
        {
            self.report(cx);
        }
        // Re-render when the controls should disappear.
        if Instant::now() < self.controls_until + Duration::from_millis(600) {
            cx.notify();
        }
    }

    fn playing(&self) -> Playing {
        Playing {
            work_id: self.work_id.clone(),
            title: (!self.title.is_empty()).then(|| self.title.clone()),
            position: self.position.max(0.0),
            duration: (self.duration > 0.0).then_some(self.duration),
            paused: self.paused,
        }
    }

    fn report(&mut self, cx: &mut Context<Self>) {
        if self.closed || self.engine.is_none() {
            return;
        }
        self.last_report = Some(Instant::now());
        cx.emit(PlayerEvent::Report(Some(self.playing())));
    }

    /// Saves the position when due; `watched` at the end of the file.
    fn save(&mut self, force: bool, watched: bool, cx: &mut Context<Self>) {
        let now = self.now();
        if !watched && !self.saver.due(now, self.position, force) {
            return;
        }
        if self.duration <= 0.0 {
            return;
        }
        self.saver.started(now, self.position);
        let update = ProgressUpdate {
            position: self.position.max(0.0),
            duration: self.duration,
            watched,
            expected_revision: self.saver.revision,
        };
        let (client, work) = (self.client.clone(), self.work_id.clone());
        let request = self
            .net
            .run(async move { client.save_progress(&work, &update).await });
        cx.spawn(async move |this, cx| match request.await {
            Some(Ok(saved)) => {
                let _ = this.update(cx, |this, _| this.saver.saved(saved.revision));
            }
            Some(Err(error)) => eprintln!("aether-desktop: position not saved: {error}"),
            None => {}
        })
        .detach();
    }

    /// Stops playback without emitting `Closed` (the application replaces
    /// or dismisses the player itself).
    pub fn shut_down(&mut self, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        self.save(true, false, cx);
        self.closed = true;
        self._tasks.clear();
        if let Some(engine) = self.engine.take() {
            // Stopping mpv can wait for the network: never on the UI thread.
            std::thread::spawn(move || drop(engine));
        }
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.is_fullscreen() {
            window.toggle_fullscreen();
        }
        self.shut_down(cx);
        cx.emit(PlayerEvent::Closed);
    }

    /// A command from another device of the account.
    pub fn remote(&mut self, action: &Action, cx: &mut Context<Self>) {
        let Some(engine) = &self.engine else { return };
        match action {
            Action::Play => engine.set_paused(false),
            Action::Pause => engine.set_paused(true),
            Action::Seek(position) => {
                engine.seek(*position);
                self.position = *position;
            }
            Action::Stop => self.stop_requested = true,
            Action::Open { .. } => {}
        }
        self.show_controls();
        cx.notify();
    }

    fn show_controls(&mut self) {
        self.controls_until = Instant::now() + CONTROLS_VISIBLE;
    }

    fn toggle_pause(&mut self) {
        if let Some(engine) = &self.engine {
            engine.set_paused(!self.paused);
        }
    }

    fn seek_to(&mut self, position: f64, cx: &mut Context<Self>) {
        let target = if self.duration > 0.0 {
            position.clamp(0.0, (self.duration - 1.0).max(0.0))
        } else {
            position.max(0.0)
        };
        if let Some(engine) = &self.engine {
            engine.seek(target);
        }
        self.position = target;
        self.save(true, false, cx);
        self.report(cx);
    }

    fn select_track(&mut self, kind: TrackKind, id: Option<i64>) {
        let Some(engine) = &self.engine else { return };
        match (kind, id) {
            (TrackKind::Audio, Some(id)) => engine.set_audio(id),
            (TrackKind::Subtitle, id) => engine.set_subtitle(id),
            _ => {}
        }
        self.menu = None;
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.show_controls();
        match event.keystroke.key.as_str() {
            "space" | "k" => self.toggle_pause(),
            "left" => self.seek_to(self.position - SEEK_STEP, cx),
            "right" => self.seek_to(self.position + SEEK_STEP, cx),
            "up" | "down" => {
                let step = if event.keystroke.key == "up" {
                    5.0
                } else {
                    -5.0
                };
                self.volume = (self.volume + step).clamp(0.0, 130.0);
                if let Some(engine) = &self.engine {
                    engine.set_volume(self.volume);
                }
            }
            "f" => window.toggle_fullscreen(),
            "escape" if self.menu.is_some() => self.menu = None,
            "escape" if window.is_fullscreen() => window.toggle_fullscreen(),
            "escape" => {
                self.close(window, cx);
                return;
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn on_seek_bar(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(bounds) = self.seek_bar.get() else {
            return;
        };
        let width = f32::from(bounds.size.width);
        if width <= 0.0 || self.duration <= 0.0 {
            return;
        }
        let x = f32::from(event.position.x - bounds.origin.x);
        let fraction = (x / width).clamp(0.0, 1.0) as f64;
        self.seek_to(fraction * self.duration, cx);
        cx.notify();
    }
}

impl Drop for PlayerView {
    fn drop(&mut self) {
        if let Some(engine) = self.engine.take() {
            std::thread::spawn(move || drop(engine));
        }
    }
}

fn icon_button(
    id: &'static str,
    label: impl Into<SharedString>,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px_3()
        .h(px(36.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .hover(|b| b.bg(theme::RAISED))
        .on_click(on_click)
        .child(label.into())
}

impl Render for PlayerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.stop_requested {
            self.stop_requested = false;
            self.close(window, cx);
        }
        for old in self.stale.drain(..) {
            let _ = window.drop_image(old);
        }
        if let Some(engine) = &self.engine {
            let viewport = window.viewport_size();
            let scale = window.scale_factor();
            let pixels = (
                (f32::from(viewport.width) * scale) as u32,
                (f32::from(viewport.height) * scale) as u32,
            );
            let (width, height) = playback::render_size(pixels, self.video);
            engine.set_target(width, height);
        }
        let controls = self.paused
            || self.menu.is_some()
            || self.error.is_some()
            || Instant::now() < self.controls_until;
        let mut root = div()
            .id("player")
            .key_context("Player")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_move(cx.listener(|this, _, _, cx| {
                let was_hidden = Instant::now() >= this.controls_until;
                this.show_controls();
                if was_hidden {
                    cx.notify();
                }
            }))
            .size_full()
            .relative()
            .bg(gpui::black())
            .when(!controls, |root| root.cursor(gpui::CursorStyle::None))
            .when_some(self.frame.clone(), |root, frame| {
                root.child(
                    div()
                        .id("video")
                        .absolute()
                        .inset_0()
                        .on_click(cx.listener(|this, event: &gpui::ClickEvent, window, cx| {
                            if event.click_count() >= 2 {
                                window.toggle_fullscreen();
                            } else {
                                this.toggle_pause();
                            }
                            cx.notify();
                        }))
                        .child(img(frame).size_full().object_fit(ObjectFit::Fill)),
                )
            });
        if self.buffering && self.error.is_none() {
            root = root.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme::MUTED)
                    .child("Chargement…"),
            );
        }
        if let Some(error) = &self.error {
            root = root.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .max_w(px(560.))
                            .text_color(theme::DANGER)
                            .child(error.clone()),
                    )
                    .child(button(
                        "player-error-close",
                        "Fermer",
                        Tone::Secondary,
                        cx.listener(|this, _, window, cx| this.close(window, cx)),
                    )),
            );
        }
        if controls {
            root = root
                .child(self.render_top(cx))
                .child(self.render_bottom(cx));
        }
        if let Some(menu) = self.menu {
            root = root.child(self.render_menu(menu, cx));
        }
        root
    }
}

impl PlayerView {
    fn render_top(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(64.))
            .px_6()
            .flex()
            .items_center()
            .gap_4()
            .bg(theme::OVERLAY)
            .child(icon_button(
                "player-close",
                "‹ Retour",
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            ))
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .truncate()
                    .child(self.title.clone()),
            )
    }

    fn render_bottom(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let fraction = state::fraction(self.position, self.duration);
        let bar = self.seek_bar.clone();
        let has_audio = self
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Audio)
            .count()
            > 1;
        let has_subtitles = self.tracks.iter().any(|t| t.kind == TrackKind::Subtitle);
        div()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .px_6()
            .pb_4()
            .pt_3()
            .flex()
            .flex_col()
            .gap_2()
            .bg(theme::OVERLAY)
            .child(
                div()
                    .id("seek-bar")
                    .relative()
                    .h(px(16.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_seek_bar))
                    .child(
                        canvas(move |bounds, _, _| bar.set(Some(bounds)), |_, _, _, _| {})
                            .absolute()
                            .inset_0(),
                    )
                    .child(super::progress_bar(fraction)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon_button(
                        "player-toggle",
                        if self.paused { "▶" } else { "❚❚" },
                        cx.listener(|this, _, _, cx| {
                            this.toggle_pause();
                            cx.notify();
                        }),
                    ))
                    .child(icon_button(
                        "player-back",
                        "−10 s",
                        cx.listener(|this, _, _, cx| this.seek_to(this.position - SEEK_STEP, cx)),
                    ))
                    .child(icon_button(
                        "player-forward",
                        "+10 s",
                        cx.listener(|this, _, _, cx| this.seek_to(this.position + SEEK_STEP, cx)),
                    ))
                    .child(div().text_sm().text_color(theme::MUTED).child(format!(
                        "{} / {}",
                        state::clock(self.position),
                        state::clock(self.duration)
                    )))
                    .child(div().flex_1())
                    .when(has_audio, |row| {
                        row.child(icon_button(
                            "player-audio",
                            "Audio",
                            cx.listener(|this, _, _, cx| {
                                this.menu = (this.menu != Some(Menu::Audio)).then_some(Menu::Audio);
                                cx.notify();
                            }),
                        ))
                    })
                    .when(has_subtitles, |row| {
                        row.child(icon_button(
                            "player-subtitles",
                            "Sous-titres",
                            cx.listener(|this, _, _, cx| {
                                this.menu =
                                    (this.menu != Some(Menu::Subtitles)).then_some(Menu::Subtitles);
                                cx.notify();
                            }),
                        ))
                    })
                    .child(icon_button(
                        "player-fullscreen",
                        "Plein écran",
                        cx.listener(|_, _, window, _| window.toggle_fullscreen()),
                    )),
            )
    }

    fn render_menu(&self, menu: Menu, cx: &mut Context<Self>) -> impl IntoElement {
        let kind = match menu {
            Menu::Audio => TrackKind::Audio,
            Menu::Subtitles => TrackKind::Subtitle,
        };
        let mut entries: Vec<(Option<i64>, String, bool)> = Vec::new();
        if kind == TrackKind::Subtitle {
            let none = !self
                .tracks
                .iter()
                .any(|t| t.kind == TrackKind::Subtitle && t.selected);
            entries.push((None, "Aucun".to_owned(), none));
        }
        entries.extend(
            self.tracks
                .iter()
                .filter(|t| t.kind == kind)
                .map(|t| (Some(t.id), t.label(), t.selected)),
        );
        div()
            .absolute()
            .right(px(24.))
            .bottom(px(110.))
            .w(px(320.))
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            .rounded_lg()
            .bg(theme::SURFACE)
            .border_1()
            .border_color(theme::BORDER)
            .children(
                entries
                    .into_iter()
                    .enumerate()
                    .map(|(index, (id, label, selected))| {
                        div()
                            .id(("track", index))
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .cursor_pointer()
                            .text_sm()
                            .hover(|row| row.bg(theme::RAISED))
                            .when(selected, |row| {
                                row.text_color(theme::ACCENT)
                                    .font_weight(FontWeight::SEMIBOLD)
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_track(kind, id);
                                cx.notify();
                            }))
                            .child(label)
                    }),
            )
    }
}
