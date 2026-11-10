//! libmpv playback drawn inside the GPUI window.
//!
//! mpv decodes (hardware decoding with copy-back when available) and renders
//! each frame with its software renderer into a memory surface; the frame is
//! handed to GPUI as a texture. Two threads per player: one waits for mpv's
//! events (position, tracks, end of file), one renders frames when mpv says a
//! new one is due, at the size [`crate::playback::render_size`] chose.
pub mod ffi;

use crate::playback::Track;
use ffi::Api;
use gpui::RenderImage;
use std::{
    alloc::{Layout, alloc_zeroed, dealloc, handle_alloc_error},
    ffi::{CStr, CString, c_char, c_int, c_void},
    ptr::NonNull,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

/// What to play.
#[derive(Clone, Debug)]
pub struct Source {
    pub url: String,
    /// Sent with every HTTP request (the device token), never logged.
    pub authorization: Option<String>,
    /// Seconds.
    pub start: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Loaded,
    Time(f64),
    Duration(f64),
    Paused(bool),
    Buffering(bool),
    VideoSize(u32, u32),
    Tracks(Vec<Track>),
    /// Playback stopped; `eof` when the file played to its end.
    Ended {
        eof: bool,
        error: Option<String>,
    },
}

const OBSERVE_TIME: u64 = 1;
const OBSERVE_DURATION: u64 = 2;
const OBSERVE_PAUSE: u64 = 3;
const OBSERVE_TRACKS: u64 = 4;
const OBSERVE_WIDTH: u64 = 5;
const OBSERVE_HEIGHT: u64 = 6;
const OBSERVE_CACHE: u64 = 7;

struct Handle {
    api: Api,
    raw: NonNull<ffi::mpv_handle>,
}

// SAFETY: libmpv's client API is thread-safe for a given handle (client.h:
// "all functions are thread-safe" except the render API, which only the
// render thread uses).
unsafe impl Send for Handle {}
// SAFETY: as above.
unsafe impl Sync for Handle {}

impl Handle {
    fn error(&self, code: c_int) -> String {
        // SAFETY: mpv_error_string returns a static string for any code.
        let text = unsafe { CStr::from_ptr((self.api.error_string)(code)) };
        text.to_string_lossy().into_owned()
    }
    fn check(&self, code: c_int, what: &str) -> Result<(), String> {
        if code < 0 {
            Err(format!("{what}: {}", self.error(code)))
        } else {
            Ok(())
        }
    }
    fn set_option(&self, name: &str, value: &str) -> Result<(), String> {
        let (n, v) = (cstring(name)?, cstring(value)?);
        // SAFETY: valid handle and NUL-terminated strings mpv copies.
        let code =
            unsafe { (self.api.set_option_string)(self.raw.as_ptr(), n.as_ptr(), v.as_ptr()) };
        self.check(code, name)
    }
    fn set_property(&self, name: &str, value: &str) -> Result<(), String> {
        let (n, v) = (cstring(name)?, cstring(value)?);
        // SAFETY: valid handle and NUL-terminated strings mpv copies.
        let code =
            unsafe { (self.api.set_property_string)(self.raw.as_ptr(), n.as_ptr(), v.as_ptr()) };
        self.check(code, name)
    }
    fn property(&self, name: &str) -> Option<String> {
        let n = cstring(name).ok()?;
        // SAFETY: valid handle and name; the result is NULL or a string we own.
        let value = unsafe { (self.api.get_property_string)(self.raw.as_ptr(), n.as_ptr()) };
        if value.is_null() {
            return None;
        }
        // SAFETY: non-NULL, NUL-terminated, freed exactly once with mpv_free.
        let text = unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: allocated by mpv for this call.
        unsafe { (self.api.free)(value.cast()) };
        Some(text)
    }
    fn command(&self, args: &[&str]) -> Result<(), String> {
        let owned = args
            .iter()
            .map(|a| cstring(a))
            .collect::<Result<Vec<_>, _>>()?;
        let mut pointers: Vec<*const c_char> = owned.iter().map(|a| a.as_ptr()).collect();
        pointers.push(std::ptr::null());
        // SAFETY: a NULL-terminated array of valid strings, alive for the call.
        let code = unsafe { (self.api.command)(self.raw.as_ptr(), pointers.as_mut_ptr()) };
        self.check(code, args.first().copied().unwrap_or("command"))
    }
    fn observe(&self, id: u64, name: &str, format: c_int) {
        if let Ok(n) = cstring(name) {
            // SAFETY: valid handle and name.
            unsafe { (self.api.observe_property)(self.raw.as_ptr(), id, n.as_ptr(), format) };
        }
    }
    fn tracks(&self) -> Vec<Track> {
        let count: usize = self
            .property("track-list/count")
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        (0..count.min(256))
            .filter_map(|i| Track::from_mpv(|key| self.property(&format!("track-list/{i}/{key}"))))
            .collect()
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: last owner; the render context was freed and both threads
        // joined before the last Arc<Handle> is dropped.
        unsafe { (self.api.terminate_destroy)(self.raw.as_ptr()) };
    }
}

fn cstring(value: &str) -> Result<CString, String> {
    CString::new(value).map_err(|_| "chaîne invalide pour mpv".to_owned())
}

/// Signals between mpv's update callback, the UI and the render thread.
struct RenderShared {
    state: Mutex<RenderState>,
    wake: Condvar,
    /// Wanted frame size, `width << 32 | height`.
    target: AtomicU64,
    latest: Mutex<Option<Arc<RenderImage>>>,
}

#[derive(Default)]
struct RenderState {
    dirty: bool,
    stop: bool,
}

impl RenderShared {
    fn poke(&self, stop: bool) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.dirty = true;
        state.stop |= stop;
        self.wake.notify_one();
    }
}

/// mpv's render update callback: never call mpv from here.
unsafe extern "C" fn on_render_update(data: *mut c_void) {
    // SAFETY: `data` is the RenderShared the render thread keeps alive until
    // mpv_render_context_free returned, after which mpv no longer calls back.
    let shared = unsafe { &*(data as *const RenderShared) };
    shared.poke(false);
}

pub struct Engine {
    handle: Arc<Handle>,
    shared: Arc<RenderShared>,
    stop: Arc<AtomicBool>,
    events: Option<JoinHandle<()>>,
    render: Option<JoinHandle<()>>,
}

/// Receivers the UI listens to.
pub struct Channels {
    pub events: futures::channel::mpsc::UnboundedReceiver<Event>,
    /// One item whenever a new frame waits in [`Engine::take_frame`].
    pub frames: futures::channel::mpsc::Receiver<()>,
}

impl Engine {
    /// Opens libmpv and starts playing. `extra` options come after the
    /// application's own (tests use `ao=null`).
    pub fn start(source: &Source, extra: &[(&str, &str)]) -> Result<(Self, Channels), String> {
        let api = Api::load()?;
        // SAFETY: no precondition; NULL on allocation failure.
        let raw = NonNull::new(unsafe { (api.create)() })
            .ok_or_else(|| "mpv_create a échoué".to_owned())?;
        let handle = Arc::new(Handle { api, raw });
        let start = format!("{:.3}", source.start.max(0.0));
        let mut options: Vec<(&str, &str)> = vec![
            ("vo", "libmpv"),
            ("config", "no"),
            ("terminal", "no"),
            ("input-default-bindings", "no"),
            ("input-vo-keyboard", "no"),
            ("osc", "no"),
            ("load-scripts", "no"),
            ("ytdl", "no"),
            // Hardware decoding where safe, frames copied back for the
            // software renderer.
            ("hwdec", "auto-copy-safe"),
            ("cache", "yes"),
            ("demuxer-max-bytes", "150MiB"),
            ("network-timeout", "30"),
            ("start", &start),
        ];
        let header;
        if let Some(authorization) = &source.authorization {
            header = format!("Authorization: {authorization}");
            options.push(("http-header-fields", &header));
        }
        options.extend_from_slice(extra);
        for (name, value) in options {
            // An option an older mpv lacks is not fatal; the header and `vo` are.
            if let Err(error) = handle.set_option(name, value)
                && matches!(name, "vo" | "http-header-fields")
            {
                return Err(error);
            }
        }
        // SAFETY: valid, not yet initialized handle.
        let code = unsafe { (handle.api.initialize)(handle.raw.as_ptr()) };
        handle.check(code, "mpv_initialize")?;
        handle.observe(OBSERVE_TIME, "time-pos", ffi::FORMAT_DOUBLE);
        handle.observe(OBSERVE_DURATION, "duration", ffi::FORMAT_DOUBLE);
        handle.observe(OBSERVE_PAUSE, "pause", ffi::FORMAT_FLAG);
        handle.observe(OBSERVE_TRACKS, "track-list", ffi::FORMAT_NONE);
        handle.observe(OBSERVE_WIDTH, "dwidth", ffi::FORMAT_INT64);
        handle.observe(OBSERVE_HEIGHT, "dheight", ffi::FORMAT_INT64);
        handle.observe(OBSERVE_CACHE, "paused-for-cache", ffi::FORMAT_FLAG);

        let shared = Arc::new(RenderShared {
            state: Mutex::new(RenderState::default()),
            wake: Condvar::new(),
            target: AtomicU64::new(pack(1280, 720)),
            latest: Mutex::new(None),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let (event_sender, events) = futures::channel::mpsc::unbounded();
        let (frame_sender, frames) = futures::channel::mpsc::channel(1);
        let (ready_sender, ready) = std::sync::mpsc::channel();
        let render = {
            let (handle, shared) = (handle.clone(), shared.clone());
            let events = event_sender.clone();
            std::thread::Builder::new()
                .name("aether-mpv-render".into())
                .spawn(move || render_loop(&handle, &shared, frame_sender, events, ready_sender))
                .map_err(|e| e.to_string())?
        };
        // The render context must exist before the file loads, or mpv picks
        // no video output.
        let mut engine = Self {
            handle: handle.clone(),
            shared,
            stop: stop.clone(),
            events: None,
            render: Some(render),
        };
        ready
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| "le rendu mpv n'a pas démarré".to_owned())??;
        engine.events = Some(
            std::thread::Builder::new()
                .name("aether-mpv-events".into())
                .spawn(move || event_loop(&handle, &stop, event_sender))
                .map_err(|e| e.to_string())?,
        );
        engine.handle.command(&["loadfile", &source.url])?;
        Ok((engine, Channels { events, frames }))
    }

    /// The newest rendered frame, if one arrived since the last call.
    pub fn take_frame(&self) -> Option<Arc<RenderImage>> {
        self.shared
            .latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }
    /// Size in pixels the next frames should have.
    pub fn set_target(&self, width: u32, height: u32) {
        let packed = pack(width, height);
        if self.shared.target.swap(packed, Ordering::Relaxed) != packed {
            self.shared.poke(false);
        }
    }
    pub fn set_paused(&self, paused: bool) {
        let _ = self
            .handle
            .set_property("pause", if paused { "yes" } else { "no" });
    }
    pub fn seek(&self, seconds: f64) {
        let _ = self
            .handle
            .command(&["seek", &format!("{:.3}", seconds.max(0.0)), "absolute"]);
    }
    pub fn set_audio(&self, id: i64) {
        let _ = self.handle.set_property("aid", &id.to_string());
    }
    /// `None` hides subtitles.
    pub fn set_subtitle(&self, id: Option<i64>) {
        let value = id.map_or_else(|| "no".to_owned(), |id| id.to_string());
        let _ = self.handle.set_property("sid", &value);
    }
    pub fn set_volume(&self, percent: f64) {
        let _ = self
            .handle
            .set_property("volume", &format!("{:.0}", percent.clamp(0.0, 130.0)));
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.shared.poke(true);
        if let Some(render) = self.render.take() {
            let _ = render.join();
        }
        // SAFETY: valid handle; makes mpv_wait_event return.
        unsafe { (self.handle.api.wakeup)(self.handle.raw.as_ptr()) };
        if let Some(events) = self.events.take() {
            let _ = events.join();
        }
        // The last Arc<Handle> goes with `self`: mpv_terminate_destroy.
    }
}

fn pack(width: u32, height: u32) -> u64 {
    ((width as u64) << 32) | height as u64
}
fn unpack(value: u64) -> (u32, u32) {
    ((value >> 32) as u32, value as u32)
}

fn event_loop(
    handle: &Handle,
    stop: &AtomicBool,
    events: futures::channel::mpsc::UnboundedSender<Event>,
) {
    let (mut width, mut height) = (0u32, 0u32);
    loop {
        // SAFETY: valid handle; the event stays valid until the next call.
        let event = unsafe { &*(handle.api.wait_event)(handle.raw.as_ptr(), -1.0) };
        if stop.load(Ordering::SeqCst) || event.event_id == ffi::EVENT_SHUTDOWN {
            return;
        }
        let message = match event.event_id {
            ffi::EVENT_FILE_LOADED => Some(Event::Loaded),
            ffi::EVENT_PLAYBACK_RESTART => Some(Event::Buffering(false)),
            ffi::EVENT_END_FILE if !event.data.is_null() => {
                // SAFETY: END_FILE carries an mpv_event_end_file.
                let end = unsafe { &*(event.data as *const ffi::mpv_event_end_file) };
                Some(Event::Ended {
                    eof: end.reason == ffi::END_FILE_REASON_EOF,
                    error: (end.reason == ffi::END_FILE_REASON_ERROR)
                        .then(|| handle.error(end.error)),
                })
            }
            ffi::EVENT_PROPERTY_CHANGE if !event.data.is_null() => {
                // SAFETY: PROPERTY_CHANGE carries an mpv_event_property.
                let property = unsafe { &*(event.data as *const ffi::mpv_event_property) };
                property_event(
                    handle,
                    event.reply_userdata,
                    property,
                    &mut width,
                    &mut height,
                )
            }
            _ => None,
        };
        if let Some(message) = message
            && events.unbounded_send(message).is_err()
        {
            return;
        }
    }
}

fn property_event(
    handle: &Handle,
    id: u64,
    property: &ffi::mpv_event_property,
    width: &mut u32,
    height: &mut u32,
) -> Option<Event> {
    if id == OBSERVE_TRACKS {
        return Some(Event::Tracks(handle.tracks()));
    }
    if property.data.is_null() {
        return None;
    }
    let data = property.data;
    // SAFETY: mpv sets `data` to a value of the observed format, checked in
    // the match below before each read.
    let double = || unsafe { *(data as *const f64) };
    // SAFETY: as above, for MPV_FORMAT_FLAG (a C int).
    let flag = || unsafe { *(data as *const c_int) } != 0;
    // SAFETY: as above, for MPV_FORMAT_INT64.
    let int = || unsafe { *(data as *const i64) };
    match (id, property.format) {
        (OBSERVE_TIME, ffi::FORMAT_DOUBLE) => Some(Event::Time(double())),
        (OBSERVE_DURATION, ffi::FORMAT_DOUBLE) => Some(Event::Duration(double())),
        (OBSERVE_PAUSE, ffi::FORMAT_FLAG) => Some(Event::Paused(flag())),
        (OBSERVE_CACHE, ffi::FORMAT_FLAG) => Some(Event::Buffering(flag())),
        (OBSERVE_WIDTH | OBSERVE_HEIGHT, ffi::FORMAT_INT64) => {
            let value = u32::try_from(int()).unwrap_or(0);
            if id == OBSERVE_WIDTH {
                *width = value;
            } else {
                *height = value;
            }
            (*width > 0 && *height > 0).then_some(Event::VideoSize(*width, *height))
        }
        _ => None,
    }
}

/// A 64-byte aligned pixel buffer, as mpv's software renderer prefers.
struct Surface {
    pointer: NonNull<u8>,
    layout: Layout,
}

impl Surface {
    fn new(bytes: usize) -> Self {
        let layout = Layout::from_size_align(bytes.max(64), 64)
            .unwrap_or_else(|_| handle_alloc_error(Layout::new::<[u8; 64]>()));
        // SAFETY: non-zero size.
        let pointer = NonNull::new(unsafe { alloc_zeroed(layout) })
            .unwrap_or_else(|| handle_alloc_error(layout));
        Self { pointer, layout }
    }
    fn bytes(&self) -> &[u8] {
        // SAFETY: `layout.size()` initialized bytes owned by this surface.
        unsafe { std::slice::from_raw_parts(self.pointer.as_ptr(), self.layout.size()) }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: allocated with this layout in `new`.
        unsafe { dealloc(self.pointer.as_ptr(), self.layout) };
    }
}

/// `bgr0` rows (with padding) → tightly packed BGRA with an opaque alpha, the
/// layout GPUI uploads.
pub fn bgr0_to_bgra(source: &[u8], stride: usize, width: usize, height: usize) -> Vec<u8> {
    let row = width * 4;
    let mut out = Vec::with_capacity(row * height);
    for y in 0..height {
        out.extend_from_slice(&source[y * stride..y * stride + row]);
    }
    for pixel in out.as_chunks_mut::<4>().0 {
        pixel[3] = 0xff;
    }
    out
}

fn render_loop(
    handle: &Handle,
    shared: &Arc<RenderShared>,
    mut frames: futures::channel::mpsc::Sender<()>,
    events: futures::channel::mpsc::UnboundedSender<Event>,
    ready: std::sync::mpsc::Sender<Result<(), String>>,
) {
    let api_type = c"sw";
    let mut params = [
        ffi::mpv_render_param {
            kind: ffi::RENDER_PARAM_API_TYPE,
            data: api_type.as_ptr() as *mut c_void,
        },
        ffi::mpv_render_param {
            kind: ffi::RENDER_PARAM_INVALID,
            data: std::ptr::null_mut(),
        },
    ];
    let mut context: *mut ffi::mpv_render_context = std::ptr::null_mut();
    // SAFETY: valid handle and a parameter list terminated by INVALID.
    let code = unsafe {
        (handle.api.render_context_create)(&mut context, handle.raw.as_ptr(), params.as_mut_ptr())
    };
    if code < 0 || context.is_null() {
        let _ = ready.send(Err(format!("rendu mpv: {}", handle.error(code))));
        return;
    }
    // SAFETY: valid context; `shared` outlives it (freed below first).
    unsafe {
        (handle.api.render_context_set_update_callback)(
            context,
            Some(on_render_update),
            Arc::as_ptr(shared) as *mut c_void,
        )
    };
    let _ = ready.send(Ok(()));

    let mut surface: Option<(Surface, (u32, u32), usize)> = None;
    let mut rendered: Option<(u32, u32)> = None;
    loop {
        {
            let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            if !state.dirty && !state.stop {
                // Also wakes regularly to pick up a resize while paused.
                state = shared
                    .wake
                    .wait_timeout(state, Duration::from_millis(100))
                    .map(|(s, _)| s)
                    .unwrap_or_else(|e| e.into_inner().0);
            }
            if state.stop {
                break;
            }
            state.dirty = false;
        }
        // SAFETY: valid context, used from this thread only.
        let flags = unsafe { (handle.api.render_context_update)(context) };
        let size = unpack(shared.target.load(Ordering::Relaxed));
        let new_frame = flags & ffi::RENDER_UPDATE_FRAME != 0;
        if !new_frame && (rendered.is_none() || rendered == Some(size)) {
            continue;
        }
        let (width, height) = (size.0.max(2), size.1.max(2));
        let stride = (width as usize * 4).next_multiple_of(64);
        if surface
            .as_ref()
            .is_none_or(|(_, s, _)| *s != (width, height))
        {
            surface = Some((
                Surface::new(stride * height as usize),
                (width, height),
                stride,
            ));
        }
        let Some((target, _, stride)) = surface.as_mut() else {
            continue;
        };
        let mut dimensions: [c_int; 2] = [width as c_int, height as c_int];
        let mut stride_value: usize = *stride;
        let format = c"bgr0";
        let mut render_params = [
            ffi::mpv_render_param {
                kind: ffi::RENDER_PARAM_SW_SIZE,
                data: dimensions.as_mut_ptr().cast(),
            },
            ffi::mpv_render_param {
                kind: ffi::RENDER_PARAM_SW_FORMAT,
                data: format.as_ptr() as *mut c_void,
            },
            ffi::mpv_render_param {
                kind: ffi::RENDER_PARAM_SW_STRIDE,
                data: (&mut stride_value as *mut usize).cast(),
            },
            ffi::mpv_render_param {
                kind: ffi::RENDER_PARAM_SW_POINTER,
                data: target.pointer.as_ptr().cast(),
            },
            ffi::mpv_render_param {
                kind: ffi::RENDER_PARAM_INVALID,
                data: std::ptr::null_mut(),
            },
        ];
        // SAFETY: the surface holds stride × height bytes, 64-byte aligned.
        let code =
            unsafe { (handle.api.render_context_render)(context, render_params.as_mut_ptr()) };
        if code < 0 {
            let _ = events.unbounded_send(Event::Ended {
                eof: false,
                error: Some(handle.error(code)),
            });
            break;
        }
        let pixels = bgr0_to_bgra(target.bytes(), *stride, width as usize, height as usize);
        let Some(buffer) = image::RgbaImage::from_raw(width, height, pixels) else {
            continue;
        };
        let frame = Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
            buffer
        )]));
        rendered = Some(size);
        *shared.latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(frame);
        // Full means the UI has not picked the previous one up yet: it will
        // take this newer frame instead.
        let _ = frames.try_send(());
    }
    // SAFETY: valid context, freed once; no callback runs after this returns.
    unsafe { (handle.api.render_context_free)(context) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[test]
    fn frames_are_repacked_opaque() {
        // 2×2 pixels, 12-byte rows (4 bytes of padding).
        let source = [
            1, 2, 3, 0, 4, 5, 6, 9, 0xaa, 0xaa, 0xaa, 0xaa, //
            7, 8, 9, 0, 10, 11, 12, 0, 0xaa, 0xaa, 0xaa, 0xaa,
        ];
        assert_eq!(
            bgr0_to_bgra(&source, 12, 2, 2),
            [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]
        );
        assert_eq!(unpack(pack(3840, 2160)), (3840, 2160));
    }

    /// Plays a generated clip through the real libmpv when it is installed
    /// (the Docker image has it); skipped otherwise.
    #[test]
    fn libmpv_plays_renders_and_stops() {
        if let Err(error) = Api::load() {
            eprintln!("skipping: {error}");
            return;
        }
        let dir = std::env::temp_dir().join(format!("aether-mpv-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let clip = dir.join("clip.mkv");
        let generated = std::process::Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=25:duration=3",
            ])
            .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=3"])
            .args([
                "-metadata:s:a:0",
                "language=fre",
                "-c:v",
                "mpeg4",
                "-c:a",
                "mp2",
            ])
            .arg(&clip)
            .status();
        if !generated.is_ok_and(|s| s.success()) {
            eprintln!("skipping: ffmpeg with lavfi is not available");
            return;
        }
        let source = Source {
            url: clip.to_string_lossy().into_owned(),
            authorization: Some("Bearer test".into()),
            start: 1.0,
        };
        let (engine, mut channels) = Engine::start(&source, &[("ao", "null")]).unwrap();
        engine.set_target(160, 120);
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let (mut loaded, mut duration, mut audio, mut size, mut time) =
            (false, 0.0, None, None, 0.0);
        let mut frame = None;
        futures::executor::block_on(async {
            while std::time::Instant::now() < deadline
                && !(loaded
                    && duration > 0.0
                    && audio.is_some()
                    && size.is_some()
                    && frame.is_some())
            {
                while let Ok(event) = channels.events.try_recv() {
                    match event {
                        Event::Loaded => loaded = true,
                        Event::Duration(d) => duration = d,
                        Event::Time(t) => time = t,
                        Event::VideoSize(w, h) => size = Some((w, h)),
                        Event::Tracks(tracks) => {
                            audio = tracks
                                .into_iter()
                                .find(|t| t.kind == crate::playback::TrackKind::Audio)
                        }
                        Event::Ended { error, .. } => panic!("ended early: {error:?}"),
                        _ => {}
                    }
                }
                if frame.is_none()
                    && let Ok(()) = channels.frames.try_recv()
                {
                    frame = engine.take_frame();
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        assert!(loaded, "file loaded");
        assert!((duration - 3.0).abs() < 0.2, "duration {duration}");
        assert!(time >= 0.9, "started at the resume position, got {time}");
        assert_eq!(size, Some((320, 240)));
        let audio = audio.expect("audio track listed");
        assert_eq!(audio.lang.as_deref(), Some("fre"));
        let frame = frame.expect("a frame was rendered");
        assert_eq!((frame.size(0).width.0, frame.size(0).height.0), (160, 120));
        assert!(
            frame
                .as_bytes(0)
                .unwrap()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[3] == 255)
        );

        engine.set_paused(true);
        let paused = futures::executor::block_on(async {
            while let Some(event) = channels.events.next().await {
                if event == Event::Paused(true) {
                    return true;
                }
            }
            false
        });
        assert!(paused);
        let started = std::time::Instant::now();
        drop(engine);
        assert!(started.elapsed() < Duration::from_secs(5), "stops promptly");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
